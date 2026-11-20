//! Evidence preservation — behaviour-preserving port of ARTEX `evidence/`.
//!
//! The Go package keeps finding evidence independent of the disposable traffic
//! store. It has three layers:
//!
//! 1. A content-addressed blob store (`Store.writeBody`, `hashPath`,
//!    `verifyFile`): SHA-256 → `blobs/<hash[:2]>/<hash>.bin`, written through a
//!    staged temp file so a body becomes visible only after a durable write.
//! 2. The Postgres metadata layer (`db/finding_traffic.go`): immutable
//!    `traffic_evidence_snapshots` rows plus per-finding
//!    `finding_traffic_bindings`, all under a process-wide advisory lock.
//! 3. GC (`Store.Collect`): snapshots no longer bound are marked, then deleted
//!    after a 24h grace period; blob files that no snapshot references are
//!    reaped after the same grace period.
//!
//! Layout under `dir` (the evidence directory):
//! - `<hash[:2]>/<hash>.bin` — content-addressed request/response bodies.
//! - `.staging/` — short-lived temp files from [`EvidenceBlob::put`].
//!
//! Deliberate deviations from the Go source (see the crate report):
//! - Go nests blobs under `<dir>/blobs/`; this port stores them directly under
//!   the evidence directory, as specified by the crate contract.
//! - Go seals `req_head`/`resp_head` with AES-GCM before INSERT and decrypts on
//!   read. Secret handling lives outside this crate, so heads are stored in
//!   clear here.
//! - Go's `Store.Record`/`Bind` read bodies from the live traffic store;
//!   this crate takes already-read snapshots ([`PreparedTrafficEvidence`]),
//!   keeping the traffic dependency out of the evidence layer.
//! - Go strings may carry invalid UTF-8; Rust `String` cannot, so the
//!   `utf8Clean` step only strips NUL bytes here.

use chrono::{DateTime, Utc};
use database::Db;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{Postgres, Row};
use std::collections::HashSet;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Process-wide advisory lock covering the evidence filesystem *and* its SQL
/// references (Go `findingEvidenceLockKey`).
pub const EVIDENCE_LOCK_KEY: i64 = 7_337_741_004;

/// How long an unreferenced snapshot row or blob file survives before GC reaps
/// it (Go `24*time.Hour`).
pub const GC_GRACE: Duration = Duration::from_secs(24 * 60 * 60);

/// Traffic binding roles (Go `ValidTrafficRole`).
pub const ROLE_BASELINE: &str = "baseline";
pub const ROLE_PROOF: &str = "proof";
pub const ROLE_VERIFICATION: &str = "verification";
pub const ROLE_SUPPORTING: &str = "supporting";

/// Default role applied by [`normalize_traffic_refs`] (Go `NormalizeTrafficRefs`).
pub const DEFAULT_TRAFFIC_ROLE: &str = ROLE_SUPPORTING;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors from the evidence layer.
#[derive(Debug, thiserror::Error)]
pub enum EvidenceError {
    #[error("sql: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("io: {0}")]
    Io(#[from] io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("not found")]
    NotFound,
    #[error("invalid: {0}")]
    Invalid(String),
}

// ---------------------------------------------------------------------------
// Traffic references
// ---------------------------------------------------------------------------

/// One traffic capture the caller wants bound to a finding (Go `db.TrafficRef`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TrafficRef {
    pub traffic_id: String,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub note: String,
}

/// Reports whether `role` is one of the four accepted binding roles.
pub fn valid_traffic_role(role: &str) -> bool {
    matches!(
        role,
        ROLE_BASELINE | ROLE_PROOF | ROLE_VERIFICATION | ROLE_SUPPORTING
    )
}

/// Trim ids, default the role to `supporting`, validate it, and drop duplicate
/// traffic ids (first occurrence wins). Port of Go `db.NormalizeTrafficRefs`.
pub fn normalize_traffic_refs(refs: &[TrafficRef]) -> Result<Vec<TrafficRef>, EvidenceError> {
    let mut out = Vec::with_capacity(refs.len());
    let mut seen = HashSet::new();
    for raw in refs {
        let mut r = raw.clone();
        r.traffic_id = r.traffic_id.trim().to_string();
        if r.traffic_id.is_empty() {
            return Err(EvidenceError::Invalid(
                "traffic_id must not be empty".to_string(),
            ));
        }
        if r.role.is_empty() {
            r.role = DEFAULT_TRAFFIC_ROLE.to_string();
        }
        if !valid_traffic_role(&r.role) {
            return Err(EvidenceError::Invalid(format!(
                "invalid traffic role {:?}",
                r.role
            )));
        }
        if seen.insert(r.traffic_id.clone()) {
            out.push(r);
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Snapshot model
// ---------------------------------------------------------------------------

/// An immutable copy of one captured exchange's metadata (Go
/// `db.TrafficEvidenceSnapshot`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrafficEvidenceSnapshot {
    pub id: String,
    pub source_traffic_id: String,
    pub captured_at: i64,
    pub url: String,
    pub method: String,
    pub status: i32,
    pub content_type: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub req_head: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub resp_head: String,
    pub req_hash: String,
    pub resp_hash: String,
    pub req_len: i64,
    pub resp_len: i64,
    #[serde(with = "go_time")]
    pub created_at: DateTime<Utc>,
}

impl TrafficEvidenceSnapshot {
    /// Apply the text normalization Go performs before hashing and storing
    /// (`Normalize`). Rust strings are always valid UTF-8, so this strips NUL
    /// bytes only.
    pub fn normalized(&self) -> Self {
        let mut s = self.clone();
        s.source_traffic_id = utf8_clean(&s.source_traffic_id);
        s.url = utf8_clean(&s.url);
        s.method = utf8_clean(&s.method);
        s.content_type = utf8_clean(&s.content_type);
        s.req_head = utf8_clean(&s.req_head);
        s.resp_head = utf8_clean(&s.resp_head);
        s
    }

    /// The content-derived primary key (Go `db.TrafficSnapshotID`): SHA-256
    /// over the Go-compatible JSON of the normalized snapshot with `id` blank
    /// and `created_at` zeroed.
    pub fn computed_id(&self) -> String {
        snapshot_id(self)
    }
}

/// Remove NUL bytes (Go `utf8Clean`, minus the invalid-UTF-8 replacement that
/// cannot occur in a Rust `String`).
fn utf8_clean(s: &str) -> String {
    s.chars().filter(|c| *c != '\0').collect()
}

/// Content-derived snapshot id, byte-compatible with Go's `TrafficSnapshotID`.
pub fn snapshot_id(snapshot: &TrafficEvidenceSnapshot) -> String {
    let s = snapshot.normalized();
    let mut json = String::from("{\"id\":\"\",\"source_traffic_id\":");
    go_json_string(&s.source_traffic_id, &mut json);
    json.push_str(",\"captured_at\":");
    json.push_str(&s.captured_at.to_string());
    json.push_str(",\"url\":");
    go_json_string(&s.url, &mut json);
    json.push_str(",\"method\":");
    go_json_string(&s.method, &mut json);
    json.push_str(",\"status\":");
    json.push_str(&s.status.to_string());
    json.push_str(",\"content_type\":");
    go_json_string(&s.content_type, &mut json);
    if !s.req_head.is_empty() {
        json.push_str(",\"req_head\":");
        go_json_string(&s.req_head, &mut json);
    }
    if !s.resp_head.is_empty() {
        json.push_str(",\"resp_head\":");
        go_json_string(&s.resp_head, &mut json);
    }
    json.push_str(",\"req_hash\":");
    go_json_string(&s.req_hash, &mut json);
    json.push_str(",\"resp_hash\":");
    go_json_string(&s.resp_hash, &mut json);
    json.push_str(",\"req_len\":");
    json.push_str(&s.req_len.to_string());
    json.push_str(",\"resp_len\":");
    json.push_str(&s.resp_len.to_string());
    json.push_str(",\"created_at\":\"0001-01-01T00:00:00Z\"}");
    sha256_hex(json.as_bytes())
}

/// Serialize a string the way Go's `encoding/json` does with HTML escaping on
/// (the default): `<`, `>`, `&`, U+2028 and U+2029 become `\uXXXX`, control
/// characters use the short escapes where Go has them.
fn go_json_string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '<' => out.push_str("\\u003c"),
            '>' => out.push_str("\\u003e"),
            '&' => out.push_str("\\u0026"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Lowercase hex SHA-256.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    hex_encode(&digest)
}

/// Lowercase hex of a digest (no re-hashing).
fn hex_encode(digest: &[u8]) -> String {
    let mut out = String::with_capacity(digest.len() * 2);
    for b in digest {
        out.push(char::from_digit((b >> 4) as u32, 16).unwrap_or('0'));
        out.push(char::from_digit((b & 0x0f) as u32, 16).unwrap_or('0'));
    }
    out
}

/// Format a UTC timestamp the way Go's `time.Time` JSON encoder does
/// (RFC3339 with trailing-zero-trimmed nanoseconds).
fn go_time_string(t: DateTime<Utc>) -> String {
    let base = t.format("%Y-%m-%dT%H:%M:%S").to_string();
    let nanos = t.timestamp_subsec_nanos();
    if nanos == 0 {
        return format!("{base}Z");
    }
    let mut frac = format!("{nanos:09}");
    while frac.ends_with('0') {
        frac.pop();
    }
    format!("{base}.{frac}Z")
}

/// serde (de)serializer for `created_at` using Go's RFC3339Nano shape.
mod go_time {
    use super::*;
    use serde::Deserializer;

    pub fn serialize<S>(t: &DateTime<Utc>, s: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        s.serialize_str(&go_time_string(*t))
    }

    pub fn deserialize<'de, D>(d: D) -> Result<DateTime<Utc>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(d)?;
        DateTime::parse_from_rfc3339(&raw)
            .map(|t| t.with_timezone(&Utc))
            .map_err(serde::de::Error::custom)
    }
}

/// serde helper: i64 rendered as a JSON string (Go `json:",string"`).
mod string_id {
    use super::*;
    use serde::Deserializer;

    pub fn serialize<S>(v: &i64, s: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        s.serialize_str(&v.to_string())
    }

    pub fn deserialize<'de, D>(d: D) -> Result<i64, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Id {
            Num(i64),
            Str(String),
        }
        Ok(match Id::deserialize(d)? {
            Id::Num(n) => n,
            Id::Str(s) => s
                .parse()
                .map_err(|_| serde::de::Error::custom("invalid i64 string"))?,
        })
    }
}

/// A binding plus the snapshot it points at (Go `db.FindingTrafficBinding`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FindingTrafficBinding {
    #[serde(with = "string_id")]
    pub id: i64,
    #[serde(with = "string_id")]
    pub finding_id: i64,
    pub snapshot_id: String,
    pub role: String,
    pub note: String,
    pub position: i32,
    #[serde(with = "go_time")]
    pub created_at: DateTime<Utc>,
    pub snapshot: TrafficEvidenceSnapshot,
}

/// All bindings of a finding plus its evidence versions (Go `db.FindingTraffic`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FindingTraffic {
    #[serde(with = "string_id")]
    pub finding_id: i64,
    pub version: i64,
    pub report_version: i64,
    pub bindings: Vec<FindingTrafficBinding>,
}

/// A traffic reference paired with the snapshot captured for it (Go
/// `db.PreparedTrafficEvidence`).
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedTrafficEvidence {
    pub traffic_ref: TrafficRef,
    pub snapshot: TrafficEvidenceSnapshot,
}

// ---------------------------------------------------------------------------
// Filesystem blob store
// ---------------------------------------------------------------------------

static STAGING_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Content-addressed evidence blobs (Go `hashPath`/`writeBody`/`verifyFile`).
#[derive(Debug, Clone)]
pub struct EvidenceBlob {
    root: PathBuf,
}

impl EvidenceBlob {
    /// Create a store rooted at `root` (no filesystem access yet).
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The store root.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Validate `hash` and return its path:
    /// `<root>/<hash[:2]>/<hash>.bin`.
    pub fn path(&self, hash: &str) -> Result<PathBuf, EvidenceError> {
        validate_hash(hash)?;
        Ok(self.root.join(&hash[..2]).join(format!("{hash}.bin")))
    }

    /// Store `bytes` and return their SHA-256 hex digest. The write is staged
    /// and fsynced before the atomic rename, so a reader never observes a
    /// partial body. Re-putting the same bytes is a no-op that only refreshes
    /// the file's mtime (the GC grace period).
    pub fn put(&self, bytes: &[u8]) -> Result<String, EvidenceError> {
        let hash = sha256_hex(bytes);
        create_dir_private(&self.root)?;
        let staging = self.root.join(".staging");
        create_dir_private(&staging)?;

        let seq = STAGING_COUNTER.fetch_add(1, Ordering::SeqCst);
        let tmp = staging.join(format!(
            "body-{}-{seq}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        {
            let mut f = open_private_new(&tmp)?;
            f.write_all(bytes)?;
            f.sync_all()?;
        }

        let result = (|| -> Result<(), EvidenceError> {
            let dest = self.path(&hash)?;
            if dest.exists() {
                // A restored or concurrently written body must be intact, and
                // its grace period restarts.
                self.verify(&hash, bytes.len() as i64)?;
                fs::File::open(&dest)?.set_modified(SystemTime::now())?;
                return Ok(());
            }
            if let Some(parent) = dest.parent() {
                create_dir_private(parent)?;
            }
            fs::rename(&tmp, &dest)?;
            if let Some(parent) = dest.parent() {
                fs::File::open(parent)?.sync_all()?;
            }
            Ok(())
        })();
        let _ = fs::remove_file(&tmp);
        result?;
        Ok(hash)
    }

    /// Read a blob by hash. `Ok(None)` when the blob is absent; an invalid hash
    /// is [`EvidenceError::Invalid`].
    pub fn get(&self, hash: &str) -> Result<Option<Vec<u8>>, EvidenceError> {
        let path = self.path(hash)?;
        match fs::read(&path) {
            Ok(b) => Ok(Some(b)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Whether a blob exists. Invalid hashes simply report `false`.
    pub fn has(&self, hash: &str) -> bool {
        self.path(hash).map(|p| p.exists()).unwrap_or(false)
    }

    /// Verify that the stored body still hashes to `hash` and is `length`
    /// bytes long (Go `verifyFile`).
    pub fn verify(&self, hash: &str, length: i64) -> Result<(), EvidenceError> {
        let path = self.path(hash)?;
        let mut f = match fs::File::open(&path) {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return Err(EvidenceError::NotFound);
            }
            Err(e) => return Err(e.into()),
        };
        let mut hasher = Sha256::new();
        let n = io::copy(&mut f, &mut hasher)?;
        let digest = hasher.finalize();
        if n != length as u64 || hex_encode(&digest) != hash {
            return Err(EvidenceError::Invalid(format!(
                "evidence body verification failed: {hash}"
            )));
        }
        Ok(())
    }
}

/// Validate a lowercase 64-char hex SHA-256 digest (Go `hashPath`).
fn validate_hash(hash: &str) -> Result<(), EvidenceError> {
    let ok = hash.len() == 64
        && hash
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    if ok {
        Ok(())
    } else {
        Err(EvidenceError::Invalid(format!(
            "invalid evidence hash {hash:?}"
        )))
    }
}

#[cfg(unix)]
fn create_dir_private(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    let mut b = fs::DirBuilder::new();
    b.recursive(true).mode(0o700);
    b.create(path)
}

#[cfg(not(unix))]
fn create_dir_private(path: &Path) -> io::Result<()> {
    fs::create_dir_all(path)
}

#[cfg(unix)]
fn open_private_new(path: &Path) -> io::Result<fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}

#[cfg(not(unix))]
fn open_private_new(path: &Path) -> io::Result<fs::File> {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
}

// ---------------------------------------------------------------------------
// DB-backed evidence store
// ---------------------------------------------------------------------------

const SNAPSHOT_COLUMNS: &str = "id, source_traffic_id, captured_at, url, method, status, \
content_type, req_head, resp_head, req_hash, resp_hash, req_len, resp_len, created_at";

const BINDING_SELECT: &str = "SELECT b.id AS binding_id, b.finding_id, b.snapshot_id, \
b.role, b.note, b.position, b.created_at AS binding_created_at, \
s.id AS snap_id, s.source_traffic_id, s.captured_at, s.url, s.method, s.status, \
s.content_type, s.req_head, s.resp_head, s.req_hash, s.resp_hash, s.req_len, s.resp_len, \
s.created_at AS snap_created_at \
FROM finding_traffic_bindings b \
JOIN traffic_evidence_snapshots s ON s.id=b.snapshot_id \
WHERE b.finding_id=$1 ORDER BY b.position, b.id";

/// Result of a GC pass (Go `Store.Collect`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GcStats {
    /// Snapshot rows newly marked `unreferenced_at`.
    pub marked: u64,
    /// Snapshot rows deleted after the grace period.
    pub snapshots_deleted: u64,
    /// Blob/staging files deleted after the grace period.
    pub blobs_deleted: u64,
}

/// DB-backed evidence metadata store (Go `evidence.Store` minus the live
/// traffic reader and the HTTP/archive plumbing).
#[derive(Clone)]
pub struct EvidenceStore {
    db: Db,
    blobs: EvidenceBlob,
}

impl EvidenceStore {
    /// Create a store backed by `db` with blobs under `dir`.
    pub fn new(db: Db, dir: impl Into<PathBuf>) -> Self {
        Self {
            db,
            blobs: EvidenceBlob::new(dir),
        }
    }

    /// Wrap an existing blob store.
    pub fn with_blobs(db: Db, blobs: EvidenceBlob) -> Self {
        Self { db, blobs }
    }

    /// The database handle.
    pub fn db(&self) -> &Db {
        &self.db
    }

    /// The blob store.
    pub fn blobs(&self) -> &EvidenceBlob {
        &self.blobs
    }

    /// The evidence directory.
    pub fn dir(&self) -> &Path {
        self.blobs.root()
    }

    /// Read one side of a snapshot and verify it against the stored hash and
    /// length (Go `Store.OpenBody`). `Side::Request`/`Side::Response` only.
    pub fn read_body(
        &self,
        snapshot: &TrafficEvidenceSnapshot,
        side: Side,
    ) -> Result<Vec<u8>, EvidenceError> {
        let (hash, length) = match side {
            Side::Request => (&snapshot.req_hash, snapshot.req_len),
            Side::Response => (&snapshot.resp_hash, snapshot.resp_len),
        };
        self.blobs.verify(hash, length)?;
        self.blobs.get(hash)?.ok_or(EvidenceError::NotFound)
    }

    // -- snapshots ---------------------------------------------------------

    /// Insert a snapshot (Go `InsertEvidenceSnapshotTx`). The id is derived
    /// from the normalized metadata; a non-empty mismatched id is rejected and
    /// an empty id is filled in. Conflicting ids are a no-op, as in Go.
    pub async fn create_snapshot(
        &self,
        snapshot: &TrafficEvidenceSnapshot,
    ) -> Result<TrafficEvidenceSnapshot, EvidenceError> {
        let mut tx = self.db.pool().begin().await?;
        let out = self.insert_snapshot_tx(&mut tx, snapshot).await?;
        tx.commit().await?;
        Ok(out)
    }

    /// Fetch one snapshot by id.
    pub async fn get_snapshot(&self, id: &str) -> Result<TrafficEvidenceSnapshot, EvidenceError> {
        let row = sqlx::query(&format!(
            "SELECT {SNAPSHOT_COLUMNS} FROM traffic_evidence_snapshots WHERE id=$1"
        ))
        .bind(id)
        .fetch_optional(self.db.pool())
        .await?
        .ok_or(EvidenceError::NotFound)?;
        Ok(row_to_snapshot(&row))
    }

    /// List every snapshot, newest first (Go has no direct list; this is the
    /// natural read for the port's CRUD surface).
    pub async fn list_snapshots(&self) -> Result<Vec<TrafficEvidenceSnapshot>, EvidenceError> {
        let rows = sqlx::query(&format!(
            "SELECT {SNAPSHOT_COLUMNS} FROM traffic_evidence_snapshots \
             ORDER BY created_at DESC, id"
        ))
        .fetch_all(self.db.pool())
        .await?;
        Ok(rows.iter().map(row_to_snapshot).collect())
    }

    /// Delete one snapshot row. Returns the number of rows removed (0 when the
    /// id is unknown). Bindings referencing it block the delete via the FK.
    pub async fn delete_snapshot(&self, id: &str) -> Result<u64, EvidenceError> {
        let res = sqlx::query("DELETE FROM traffic_evidence_snapshots WHERE id=$1")
            .bind(id)
            .execute(self.db.pool())
            .await?;
        Ok(res.rows_affected())
    }

    // -- bindings ----------------------------------------------------------

    /// Add traffic snapshots to a finding and bump its evidence version when
    /// anything changed (Go `AddFindingTrafficTx` + `FindingTrafficTx`).
    /// Re-binding an existing snapshot is idempotent: its note/role are not
    /// overwritten and the version does not move.
    pub async fn bind(
        &self,
        finding_id: i64,
        prepared: &[PreparedTrafficEvidence],
    ) -> Result<FindingTraffic, EvidenceError> {
        let mut tx = self.db.pool().begin().await?;
        lock_evidence(&mut tx).await?;

        let exists: Option<i64> =
            sqlx::query_scalar("SELECT evidence_version FROM findings WHERE id=$1")
                .bind(finding_id)
                .fetch_optional(&mut *tx)
                .await?;
        if exists.is_none() {
            return Err(EvidenceError::NotFound);
        }

        let mut position: i32 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(position)+1,0) FROM finding_traffic_bindings WHERE finding_id=$1",
        )
        .bind(finding_id)
        .fetch_one(&mut *tx)
        .await?;

        let mut changed = false;
        for item in prepared {
            let snapshot = self.insert_snapshot_tx(&mut tx, &item.snapshot).await?;
            sqlx::query("UPDATE traffic_evidence_snapshots SET unreferenced_at=NULL WHERE id=$1")
                .bind(&snapshot.id)
                .execute(&mut *tx)
                .await?;
            let res = sqlx::query(
                "INSERT INTO finding_traffic_bindings(finding_id,snapshot_id,role,note,position) \
                 VALUES($1,$2,$3,$4,$5) ON CONFLICT(finding_id,snapshot_id) DO NOTHING",
            )
            .bind(finding_id)
            .bind(&snapshot.id)
            .bind(&item.traffic_ref.role)
            .bind(&item.traffic_ref.note)
            .bind(position)
            .execute(&mut *tx)
            .await?;
            if res.rows_affected() > 0 {
                changed = true;
                position += 1;
            }
        }
        if changed {
            sqlx::query("UPDATE findings SET evidence_version=evidence_version+1 WHERE id=$1")
                .bind(finding_id)
                .execute(&mut *tx)
                .await?;
        }

        let out = self.finding_traffic_tx(&mut tx, finding_id).await?;
        tx.commit().await?;
        Ok(out)
    }

    /// Remove one binding and bump the finding's evidence version (Go
    /// `EditFindingTraffic` remove path).
    pub async fn unbind(&self, finding_id: i64, binding_id: i64) -> Result<(), EvidenceError> {
        let mut tx = self.db.pool().begin().await?;
        lock_evidence(&mut tx).await?;

        let exists: Option<i64> =
            sqlx::query_scalar("SELECT evidence_version FROM findings WHERE id=$1")
                .bind(finding_id)
                .fetch_optional(&mut *tx)
                .await?;
        if exists.is_none() {
            return Err(EvidenceError::NotFound);
        }

        let res = sqlx::query("DELETE FROM finding_traffic_bindings WHERE finding_id=$1 AND id=$2")
            .bind(finding_id)
            .bind(binding_id)
            .execute(&mut *tx)
            .await?;
        if res.rows_affected() == 0 {
            return Err(EvidenceError::NotFound);
        }
        sqlx::query("UPDATE findings SET evidence_version=evidence_version+1 WHERE id=$1")
            .bind(finding_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    /// All bindings of a finding with their snapshots (Go
    /// `db.FindingTrafficTx`).
    pub async fn list_for_finding(&self, finding_id: i64) -> Result<FindingTraffic, EvidenceError> {
        let mut tx = self.db.pool().begin().await?;
        lock_evidence(&mut tx).await?;
        let out = self.finding_traffic_tx(&mut tx, finding_id).await?;
        tx.commit().await?;
        Ok(out)
    }

    /// One binding of a finding (Go `Store.Binding`); unknown ids are
    /// [`EvidenceError::NotFound`].
    pub async fn get_binding(
        &self,
        finding_id: i64,
        binding_id: i64,
    ) -> Result<FindingTrafficBinding, EvidenceError> {
        let list = self.list_for_finding(finding_id).await?;
        list.bindings
            .into_iter()
            .find(|b| b.id == binding_id)
            .ok_or(EvidenceError::NotFound)
    }

    // -- GC ----------------------------------------------------------------

    /// Mark unreferenced snapshots, delete those past the 24h grace period, and
    /// reap unreferenced blob files (Go `Store.Collect` at the current time).
    pub async fn gc_orphans(&self) -> Result<GcStats, EvidenceError> {
        self.collect(Utc::now()).await
    }

    /// GC as of `now`. Splitting the timestamp out mirrors Go's `Collect(ctx,
    /// now)` and makes the grace period testable.
    pub async fn collect(&self, now: DateTime<Utc>) -> Result<GcStats, EvidenceError> {
        let mut tx = self.db.pool().begin().await?;
        lock_evidence(&mut tx).await?;

        let marked = sqlx::query(
            "UPDATE traffic_evidence_snapshots s SET unreferenced_at=$1 \
             WHERE unreferenced_at IS NULL \
             AND NOT EXISTS(SELECT 1 FROM finding_traffic_bindings b WHERE b.snapshot_id=s.id)",
        )
        .bind(now)
        .execute(&mut *tx)
        .await?
        .rows_affected();

        let cutoff = now - chrono::Duration::hours(24);
        let snapshots_deleted = sqlx::query(
            "DELETE FROM traffic_evidence_snapshots s WHERE unreferenced_at<$1 \
             AND NOT EXISTS(SELECT 1 FROM finding_traffic_bindings b WHERE b.snapshot_id=s.id)",
        )
        .bind(cutoff)
        .execute(&mut *tx)
        .await?
        .rows_affected();

        let rows = sqlx::query(
            "SELECT req_hash FROM traffic_evidence_snapshots \
             UNION SELECT resp_hash FROM traffic_evidence_snapshots",
        )
        .fetch_all(&mut *tx)
        .await?;
        let refs: HashSet<String> = rows
            .iter()
            .map(|r| r.get::<String, _>("req_hash"))
            .collect();
        tx.commit().await?;

        let blobs_deleted = reap_files(self.blobs.root(), &refs, now)?;
        Ok(GcStats {
            marked,
            snapshots_deleted,
            blobs_deleted,
        })
    }

    // -- internals ---------------------------------------------------------

    async fn insert_snapshot_tx(
        &self,
        tx: &mut sqlx::Transaction<'_, Postgres>,
        input: &TrafficEvidenceSnapshot,
    ) -> Result<TrafficEvidenceSnapshot, EvidenceError> {
        let mut s = input.normalized();
        let computed = snapshot_id(&s);
        if s.id.is_empty() {
            s.id = computed;
        } else if s.id != computed {
            return Err(EvidenceError::Invalid(
                "evidence snapshot metadata hash mismatch".to_string(),
            ));
        }
        if s.created_at.timestamp() == 0 {
            s.created_at = Utc::now();
        }
        sqlx::query(
            "INSERT INTO traffic_evidence_snapshots \
             (id,source_traffic_id,captured_at,url,method,status,content_type,req_head,resp_head,\
             req_hash,resp_hash,req_len,resp_len,created_at) \
             VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14) \
             ON CONFLICT(id) DO NOTHING",
        )
        .bind(&s.id)
        .bind(&s.source_traffic_id)
        .bind(s.captured_at)
        .bind(&s.url)
        .bind(&s.method)
        .bind(s.status)
        .bind(&s.content_type)
        .bind(&s.req_head)
        .bind(&s.resp_head)
        .bind(&s.req_hash)
        .bind(&s.resp_hash)
        .bind(s.req_len)
        .bind(s.resp_len)
        .bind(s.created_at)
        .execute(&mut **tx)
        .await?;
        Ok(s)
    }

    async fn finding_traffic_tx(
        &self,
        tx: &mut sqlx::Transaction<'_, Postgres>,
        finding_id: i64,
    ) -> Result<FindingTraffic, EvidenceError> {
        let row = sqlx::query(
            "SELECT evidence_version, report_evidence_version FROM findings WHERE id=$1",
        )
        .bind(finding_id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or(EvidenceError::NotFound)?;
        let version: i64 = row.get("evidence_version");
        let report_version: i64 = row.get("report_evidence_version");

        let rows = sqlx::query(BINDING_SELECT)
            .bind(finding_id)
            .fetch_all(&mut **tx)
            .await?;
        let bindings = rows.iter().map(row_to_binding).collect();
        Ok(FindingTraffic {
            finding_id,
            version,
            report_version,
            bindings,
        })
    }
}

async fn lock_evidence(tx: &mut sqlx::Transaction<'_, Postgres>) -> Result<(), EvidenceError> {
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(EVIDENCE_LOCK_KEY)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

fn row_to_snapshot(row: &sqlx::postgres::PgRow) -> TrafficEvidenceSnapshot {
    TrafficEvidenceSnapshot {
        id: row.get("id"),
        source_traffic_id: row.get("source_traffic_id"),
        captured_at: row.get("captured_at"),
        url: row.get("url"),
        method: row.get("method"),
        status: row.get("status"),
        content_type: row.get("content_type"),
        req_head: row.get("req_head"),
        resp_head: row.get("resp_head"),
        req_hash: row.get("req_hash"),
        resp_hash: row.get("resp_hash"),
        req_len: row.get("req_len"),
        resp_len: row.get("resp_len"),
        created_at: row.get("created_at"),
    }
}

fn row_to_binding(row: &sqlx::postgres::PgRow) -> FindingTrafficBinding {
    FindingTrafficBinding {
        id: row.get("binding_id"),
        finding_id: row.get("finding_id"),
        snapshot_id: row.get("snapshot_id"),
        role: row.get("role"),
        note: row.get("note"),
        position: row.get("position"),
        created_at: row.get("binding_created_at"),
        snapshot: TrafficEvidenceSnapshot {
            id: row.get("snap_id"),
            source_traffic_id: row.get("source_traffic_id"),
            captured_at: row.get("captured_at"),
            url: row.get("url"),
            method: row.get("method"),
            status: row.get("status"),
            content_type: row.get("content_type"),
            req_head: row.get("req_head"),
            resp_head: row.get("resp_head"),
            req_hash: row.get("req_hash"),
            resp_hash: row.get("resp_hash"),
            req_len: row.get("req_len"),
            resp_len: row.get("resp_len"),
            created_at: row.get("snap_created_at"),
        },
    }
}

/// Reap every file under `root` that is not in `refs` and older than the grace
/// period (Go's `Collect` filesystem walk over `blobs` and `.staging`).
fn reap_files(
    root: &Path,
    refs: &HashSet<String>,
    now: DateTime<Utc>,
) -> Result<u64, EvidenceError> {
    let now = to_system_time(now);
    let mut deleted = 0u64;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(e) => e,
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e.into()),
        };
        for entry in entries {
            let entry = entry?;
            let file_type = entry.file_type()?;
            let path = entry.path();
            if file_type.is_dir() {
                stack.push(path);
                continue;
            }
            if !file_type.is_file() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let key = name.strip_suffix(".bin").unwrap_or(&name);
            if refs.contains(key) {
                continue;
            }
            let modified = entry.metadata()?.modified()?;
            let age = now.duration_since(modified).unwrap_or_default();
            if age >= GC_GRACE {
                fs::remove_file(&path)?;
                deleted += 1;
            }
        }
    }
    Ok(deleted)
}

fn to_system_time(t: DateTime<Utc>) -> SystemTime {
    let secs = t.timestamp();
    let nanos = t.timestamp_subsec_nanos();
    if secs >= 0 {
        UNIX_EPOCH + Duration::new(secs as u64, nanos)
    } else {
        UNIX_EPOCH - Duration::new((-secs) as u64, 0) + Duration::new(0, nanos)
    }
}

// ---------------------------------------------------------------------------
// Evidence rendering
// ---------------------------------------------------------------------------

/// Which half of an exchange a body belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Request,
    Response,
}

impl Side {
    /// `"request"` / `"response"`.
    pub fn as_str(self) -> &'static str {
        match self {
            Side::Request => "request",
            Side::Response => "response",
        }
    }

    /// Parse a side name (Go accepts exactly `request` and `response`).
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "request" => Some(Side::Request),
            "response" => Some(Side::Response),
            _ => None,
        }
    }
}

/// Evidence rendering helpers (Markdown report lines and HTTP quotes).
pub struct Evidence;

impl Evidence {
    /// Render one side of an exchange as the HTTP message written to
    /// `evidence/<finding>/<binding>/<side>.http` in Go exports: the head with
    /// trailing CRLFs trimmed, a blank line, then the body bytes.
    pub fn quote(snapshot: &TrafficEvidenceSnapshot, side: Side, body: &[u8]) -> String {
        let head = match side {
            Side::Request => &snapshot.req_head,
            Side::Response => &snapshot.resp_head,
        };
        let mut out = String::with_capacity(head.len() + body.len() + 2);
        out.push_str(head.trim_end_matches(['\r', '\n']));
        out.push_str("\n\n");
        out.push_str(&String::from_utf8_lossy(body));
        out
    }

    /// Render the Markdown line for one binding, port of the per-binding output
    /// of Go `report.findingTrafficMarkdown`:
    ///
    /// ```text
    /// 1. **Evidence #7 · proof** — `GET http://x/`, status 200
    ///    a note line
    /// ```
    pub fn render_binding(index: usize, binding: &FindingTrafficBinding) -> String {
        let mut out = format!(
            "{}. **Evidence #{} · {}** — `{} {}`, status {}\n",
            index + 1,
            binding.id,
            binding.role,
            binding.snapshot.method,
            binding.snapshot.url.replace('`', "%60"),
            binding.snapshot.status
        );
        if !binding.note.is_empty() {
            out.push_str("   ");
            out.push_str(&binding.note.replace('\n', "\n   "));
            out.push('\n');
        }
        out
    }

    /// Render the full `## Related Traffic Evidence` section for a finding
    /// (Go `report.findingTrafficMarkdown`). `attachments` adds relative links
    /// to the exported `request.http` / `response.http` files.
    pub fn render(list: &FindingTraffic, attachments: bool) -> String {
        if list.bindings.is_empty() && list.version == list.report_version {
            return String::new();
        }
        let mut out = String::from("\n## Related Traffic Evidence\n\n");
        out.push_str(&format!(
            "Evidence version: {}; bindings: {}.\n\n",
            list.version,
            list.bindings.len()
        ));
        if list.version != list.report_version {
            out.push_str("The evidence has changed; the detailed report needs updating.\n\n");
        }
        for (i, b) in list.bindings.iter().enumerate() {
            out.push_str(&Self::render_binding(i, b));
            if attachments {
                out.push_str(&format!(
                    "   [Request message](evidence/{}/{}/request.http) · \
                     [Response message](evidence/{}/{}/response.http)\n",
                    list.finding_id, b.id, list.finding_id, b.id
                ));
            }
        }
        out.push('\n');
        out
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU64;

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let n = COUNTER.fetch_add(1, Ordering::SeqCst);
            let mut p = std::env::temp_dir();
            p.push(format!(
                "evidence-unit-{tag}-{}-{nanos}-{n}",
                std::process::id()
            ));
            fs::create_dir_all(&p).expect("create temp dir");
            TempDir(p)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn snap() -> TrafficEvidenceSnapshot {
        TrafficEvidenceSnapshot {
            id: String::new(),
            source_traffic_id: "t1".into(),
            captured_at: 1_700_000_000,
            url: "http://x/?a=1&b=2".into(),
            method: "GET".into(),
            status: 200,
            content_type: "text/html".into(),
            req_head: "POST / HTTP/1.1\r\nHost: x\r\n".into(),
            resp_head: "HTTP/1.1 200 OK\r\n\r\n".into(),
            req_hash: "aa".into(),
            resp_hash: "bb".into(),
            req_len: 2,
            resp_len: 5,
            created_at: DateTime::<Utc>::from_timestamp(0, 0).unwrap(),
        }
    }

    #[test]
    fn blob_put_get_dedup_and_verify() {
        let dir = TempDir::new("blob");
        let blob = EvidenceBlob::new(dir.0.join("evidence"));
        let body = b"\x00\x01\x02hello\xff";
        let hash = blob.put(body).expect("put");
        assert_eq!(hash.len(), 64);
        assert_eq!(blob.get(&hash).expect("get").as_deref(), Some(&body[..]));
        assert!(blob.has(&hash));
        assert!(!blob.has(&"0".repeat(64)));
        assert_eq!(blob.get(&"0".repeat(64)).expect("missing"), None);

        // Dedup: same bytes → same path, one file.
        let hash2 = blob.put(body).expect("re-put");
        assert_eq!(hash, hash2);
        let mut files = Vec::new();
        for entry in fs::read_dir(blob.root()).expect("root") {
            let e = entry.expect("entry");
            if e.path().is_dir() && e.file_name() != ".staging" {
                files.extend(
                    fs::read_dir(e.path())
                        .expect("shard")
                        .map(|x| x.expect("f")),
                );
            }
        }
        assert_eq!(files.len(), 1, "duplicate put created extra files");
        blob.verify(&hash, body.len() as i64).expect("verify");
        assert!(blob.verify(&hash, body.len() as i64 + 1).is_err());
    }

    #[test]
    fn blob_rejects_bad_hash() {
        let dir = TempDir::new("badhash");
        let blob = EvidenceBlob::new(dir.0.join("evidence"));
        assert!(matches!(
            blob.get("not-a-hash"),
            Err(EvidenceError::Invalid(_))
        ));
        assert!(matches!(
            blob.get(&"A".repeat(64)),
            Err(EvidenceError::Invalid(_))
        ));
        assert!(!blob.has("nope"));
    }

    #[test]
    fn snapshot_id_matches_go_vectors() {
        let cases = [
            (
                snap(),
                "46133c17c180c76df92bfec6765b6d7884c7d1f4441395709addeedc13599f1b",
            ),
            (
                TrafficEvidenceSnapshot {
                    id: String::new(),
                    source_traffic_id: "t2".into(),
                    captured_at: 0,
                    url: "https://<tag>.example/ü".into(),
                    method: "POST".into(),
                    status: 404,
                    content_type: String::new(),
                    req_head: String::new(),
                    resp_head: String::new(),
                    req_hash: String::new(),
                    resp_hash: String::new(),
                    req_len: 0,
                    resp_len: 0,
                    created_at: DateTime::<Utc>::from_timestamp(0, 0).unwrap(),
                },
                "e9a49c8cadbdfadcda0d8542af8a8b035565218db5a429658fd5285fb1d2fa2a",
            ),
            (
                TrafficEvidenceSnapshot {
                    id: String::new(),
                    source_traffic_id: "t3".into(),
                    captured_at: -1,
                    url: "http://a".into(),
                    method: "GET".into(),
                    status: 200,
                    content_type: "text/plain".into(),
                    req_head: "A <b> & \"q\" \\ \u{1} \u{2028} end".into(),
                    resp_head: "h".into(),
                    req_hash: "h1".into(),
                    resp_hash: "h2".into(),
                    req_len: 1,
                    resp_len: 2,
                    created_at: DateTime::<Utc>::from_timestamp(0, 0).unwrap(),
                },
                "ceccae52daeff2bd11a5ff9af830aa9ee23380e625dab020faf018fcbeb2cd19",
            ),
        ];
        for (s, want) in cases {
            assert_eq!(snapshot_id(&s), want);
            assert_eq!(s.computed_id(), want);
        }
    }

    #[test]
    fn snapshot_id_ignores_id_created_at_and_normalizes_nul() {
        let a = snap();
        let mut b = a.clone();
        b.id = "whatever".into();
        b.created_at = DateTime::<Utc>::from_timestamp(1_700_000_123, 0).unwrap();
        assert_eq!(a.computed_id(), b.computed_id());

        let mut c = a.clone();
        c.url = "http://x/\0y".into();
        let mut d = a.clone();
        d.url = "http://x/y".into();
        assert_eq!(c.computed_id(), d.computed_id());
    }

    #[test]
    fn quote_renders_http_message() {
        let s = snap();
        assert_eq!(
            Evidence::quote(&s, Side::Request, b"{\"a\":1}"),
            "POST / HTTP/1.1\r\nHost: x\n\n{\"a\":1}"
        );
        assert_eq!(
            Evidence::quote(&s, Side::Response, b"OK"),
            "HTTP/1.1 200 OK\n\nOK"
        );
        assert_eq!(Side::parse("response"), Some(Side::Response));
        assert_eq!(Side::parse("body"), None);
    }

    #[test]
    fn render_matches_go_markdown() {
        let mut binding = FindingTrafficBinding {
            id: 7,
            finding_id: 3,
            snapshot_id: "snap".into(),
            role: ROLE_PROOF.into(),
            note: "line1\nline2".into(),
            position: 0,
            created_at: DateTime::<Utc>::from_timestamp(0, 0).unwrap(),
            snapshot: snap(),
        };
        let list = FindingTraffic {
            finding_id: 3,
            version: 3,
            report_version: 3,
            bindings: vec![binding.clone()],
        };
        assert_eq!(
            Evidence::render(&list, false),
            "\n## Related Traffic Evidence\n\nEvidence version: 3; bindings: 1.\n\n\
             1. **Evidence #7 · proof** — `GET http://x/?a=1&b=2`, status 200\n\
             \x20  line1\n   line2\n\n"
        );

        // Stale versions add the notice; attachments add the links.
        let mut list = list;
        list.version = 2;
        let stale = Evidence::render(&list, true);
        assert!(stale.contains("The evidence has changed; the detailed report needs updating.\n\n"));
        assert!(stale.contains("[Request message](evidence/3/7/request.http)"));
        assert!(stale.contains("[Response message](evidence/3/7/response.http)"));

        // Empty and up-to-date sections render nothing.
        let empty = FindingTraffic {
            finding_id: 3,
            version: 0,
            report_version: 0,
            bindings: Vec::new(),
        };
        assert!(Evidence::render(&empty, false).is_empty());

        binding.snapshot.url = "http://a/b`c".into();
        binding.note.clear();
        let only = FindingTraffic {
            finding_id: 3,
            version: 1,
            report_version: 1,
            bindings: vec![binding],
        };
        assert_eq!(
            Evidence::render(&only, false),
            "\n## Related Traffic Evidence\n\nEvidence version: 1; bindings: 1.\n\n\
             1. **Evidence #7 · proof** — `GET http://a/b%60c`, status 200\n\n"
        );
    }

    #[test]
    fn normalize_refs_defaults_dedups_and_validates() {
        let refs = vec![
            TrafficRef {
                traffic_id: " a ".into(),
                role: String::new(),
                note: "n".into(),
            },
            TrafficRef {
                traffic_id: "a".into(),
                role: ROLE_BASELINE.into(),
                note: "ignored".into(),
            },
            TrafficRef {
                traffic_id: "b".into(),
                role: ROLE_PROOF.into(),
                note: String::new(),
            },
        ];
        let out = normalize_traffic_refs(&refs).expect("normalize");
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].traffic_id, "a");
        assert_eq!(out[0].role, ROLE_SUPPORTING);
        assert_eq!(out[0].note, "n");
        assert_eq!(out[1].traffic_id, "b");

        let bad = vec![TrafficRef {
            traffic_id: "x".into(),
            role: "bogus".into(),
            note: String::new(),
        }];
        assert!(normalize_traffic_refs(&bad).is_err());
        let empty = vec![TrafficRef::default()];
        assert!(normalize_traffic_refs(&empty).is_err());
    }

    #[test]
    fn go_time_format_matches_rfc3339nano() {
        let t = DateTime::<Utc>::from_timestamp(1_700_000_000, 500_000_000).unwrap();
        assert_eq!(go_time_string(t), "2023-11-14T22:13:20.5Z");
        let t = DateTime::<Utc>::from_timestamp(1_700_000_000, 0).unwrap();
        assert_eq!(go_time_string(t), "2023-11-14T22:13:20Z");
    }
}
