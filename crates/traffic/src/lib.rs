//! Traffic recording store — behaviour-preserving port of ARTEX `traffic/`.
//!
//! This crate implements the request-recording **store** only: a content-addressed
//! blob store plus a sidecar SQLite (FTS5) index. It is a port of the parts of
//! `ARTEX/traffic/traffic.go`, `traffic/external.go` and the `/api/traffic/ingest`
//! handler in `server/traffic_ingest.go`.
//!
//! The MITM proxy itself (go-mitmproxy, TLS interception, the transparent
//! passthrough machinery, `archive.go` export/import) lives in a separate crate
//! and is out of scope here.
//!
//! Layout under `dir`:
//! - `blobs/<hash[:2]>/<hash>.bin` — content-addressed (SHA-256) spilled bodies.
//! - `_index/index.sqlite` — SQLite index (WAL, incremental auto_vacuum, FTS5
//!   trigram full-text index).
//!
//! Bodies up to 256 KiB are stored inline in SQLite; larger ones spill to the
//! blob store and keep a short inline preview. Text bodies are indexed in full
//! (up to 4 MiB) regardless of where their bytes live.

use rusqlite::{params, params_from_iter, Connection};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use walkdir::WalkDir;

/// Cutoff between an inline body (SQLite column) and one spilled to the blob store.
const MAX_INLINE_BODY: usize = 256 * 1024;
/// How much of a spilled body stays inline as a preview.
const BLOB_PREVIEW: usize = 8 * 1024;
/// Cap on a single body's contribution to the full-text index.
const MAX_INDEX_BODY: usize = 4 * 1024 * 1024;
/// Shortest term the trigram tokenizer can match.
const MIN_TRIGRAM: usize = 3;
/// Maximum exchanges accepted in one `/api/traffic/ingest` batch.
pub const MAX_INGEST_BATCH: usize = 50;
/// Per-body truncation cap applied by the ingest path.
pub const MAX_INGEST_BODY_SIZE: usize = 256 << 10;

const INDEX_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS exchanges (
  id           TEXT PRIMARY KEY,
  ts           INTEGER,
  host         TEXT,
  method       TEXT,
  url_template TEXT,
  url          TEXT,
  status       INTEGER,
  content_type TEXT,
  req_len      INTEGER,
  resp_len     INTEGER,
  path         TEXT
);
CREATE INDEX IF NOT EXISTS idx_ex_host ON exchanges(host);
CREATE INDEX IF NOT EXISTS idx_ex_tmpl ON exchanges(host, url_template);
CREATE INDEX IF NOT EXISTS idx_ex_ts   ON exchanges(ts);
CREATE INDEX IF NOT EXISTS idx_ex_status ON exchanges(status);
CREATE INDEX IF NOT EXISTS idx_ex_resp   ON exchanges(resp_len);

CREATE TABLE IF NOT EXISTS exchange_bodies (
  id        TEXT PRIMARY KEY,
  req_head  TEXT NOT NULL DEFAULT '',
  req_body  BLOB,
  req_blob  TEXT,
  resp_head TEXT NOT NULL DEFAULT '',
  resp_body BLOB,
  resp_blob TEXT
);

CREATE TABLE IF NOT EXISTS blob_refs (
  hash        TEXT NOT NULL,
  exchange_id TEXT NOT NULL,
  PRIMARY KEY (hash, exchange_id)
);
CREATE INDEX IF NOT EXISTS idx_blob_refs_ex ON blob_refs(exchange_id);
"#;

// A driver build without FTS5 must degrade to "no full-text search" rather than
// take the whole recorder down. trigram (not unicode61) matches arbitrary
// substrings and handles CJK. Contentless (content='') keeps only the index;
// contentless_delete lets rows be deleted without replaying the original text.
const FTS_SCHEMA: &str = "CREATE VIRTUAL TABLE IF NOT EXISTS ex_fts USING fts5(\
  content, tokenize='trigram', content='', contentless_delete=1\
);";

/// Content-type prefixes whose bodies are never worth indexing or previewing as text.
const BINARY_TYPES: [&str; 17] = [
    "image/",
    "audio/",
    "video/",
    "font/",
    "application/octet-stream",
    "application/zip",
    "application/gzip",
    "application/x-gzip",
    "application/x-tar",
    "application/x-7z-compressed",
    "application/x-rar",
    "application/pdf",
    "application/x-msdownload",
    "application/vnd.android.package-archive",
    "application/java-archive",
    "application/wasm",
    "application/x-shockwave-flash",
];

/// Errors returned by [`TrafficStore`].
#[derive(Debug, thiserror::Error)]
pub enum TrafficError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid: {0}")]
    Invalid(String),
}

/// One recorded request/response exchange, matching Go's stored exchange shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Exchange {
    pub method: String,
    pub url: String,
    pub status: u16,
    pub request_headers: BTreeMap<String, String>,
    pub request_body: Vec<u8>,
    pub response_headers: BTreeMap<String, String>,
    pub response_body: Vec<u8>,
    pub host: String,
    pub path: String,
    pub timestamp: i64,
}

/// One captured exchange handed in from outside the MITM proxy (the browser
/// extension ingest shape). Header maps are single-valued; any casing accepted.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalExchange {
    pub method: String,
    pub url: String,
    pub status: u16,
    #[serde(rename = "requestHeaders")]
    pub request_headers: BTreeMap<String, String>,
    #[serde(rename = "requestBody")]
    pub request_body: String,
    #[serde(rename = "responseHeaders")]
    pub response_headers: BTreeMap<String, String>,
    #[serde(rename = "responseBody")]
    pub response_body: String,
}

impl ExternalExchange {
    /// Validate and convert into a storable [`Exchange`].
    ///
    /// Port of `Traffic.RecordExternal`'s parsing: the URL must parse with a
    /// host, the method must be non-empty (upper-cased), and a zero status
    /// defaults to 200. The templated `path` is derived from the URL path.
    pub fn into_exchange(self) -> Result<Exchange, TrafficError> {
        let url = url::Url::parse(&self.url)
            .map_err(|_| TrafficError::Invalid(format!("invalid url {:?}", self.url)))?;
        if url.host_str().is_none() {
            return Err(TrafficError::Invalid(format!("invalid url {:?}", self.url)));
        }
        let method = self.method.trim().to_uppercase();
        if method.is_empty() {
            return Err(TrafficError::Invalid(format!(
                "empty method for {:?}",
                self.url
            )));
        }
        let status = if self.status == 0 { 200 } else { self.status };
        let host = url.host_str().unwrap_or("").to_string();
        let path = template_path(url.path());
        Ok(Exchange {
            method,
            url: url.to_string(),
            status,
            request_headers: self.request_headers,
            request_body: self.request_body.into_bytes(),
            response_headers: self.response_headers,
            response_body: self.response_body.into_bytes(),
            host,
            path,
            timestamp: now_unix(),
        })
    }
}

/// One row of the search index (returned by [`TrafficStore::search`]). Bodies
/// are never included.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExchangeMeta {
    pub id: String,
    pub ts: i64,
    pub host: String,
    pub method: String,
    pub url_template: String,
    pub url: String,
    pub status: i64,
    pub content_type: String,
    pub resp_len: i64,
    pub path: String,
}

/// One distinct recorded host plus its exchange count.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostCount {
    pub host: String,
    pub count: i64,
}

/// The record store: a file tree of blobs plus a SQLite FTS index.
pub struct TrafficStore {
    dir: PathBuf,
    conn: Mutex<Connection>,
    seq: AtomicI64,
    fts: bool,
}

struct StoredBody {
    inline: Vec<u8>,
    hash: Option<String>,
    index: String,
}

impl TrafficStore {
    /// Open (creating if needed) the traffic tree under `dir`.
    ///
    /// Creates the blob directory and `_index/index.sqlite`, applying the Go
    /// schema (WAL, `auto_vacuum=incremental` on a fresh database, FTS5 trigram
    /// index). A missing FTS5 build degrades to metadata-only search.
    pub fn open(dir: PathBuf) -> Result<Self, TrafficError> {
        for d in [
            dir.as_path(),
            dir.join("_index").as_path(),
            dir.join("blobs").as_path(),
        ] {
            fs::create_dir_all(d)?;
        }
        let index_path = dir.join("_index").join("index.sqlite");
        let conn = Connection::open(index_path)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
        let tables: i64 = conn.query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table'",
            [],
            |r| r.get(0),
        )?;
        if tables == 0 {
            conn.execute_batch("PRAGMA auto_vacuum=incremental; VACUUM;")?;
        }
        conn.execute_batch(INDEX_SCHEMA)?;
        let fts = conn.execute_batch(FTS_SCHEMA).is_ok();
        Ok(TrafficStore {
            dir,
            conn: Mutex::new(conn),
            seq: AtomicI64::new(0),
            fts,
        })
    }

    /// The store's root directory.
    pub fn dir(&self) -> &PathBuf {
        &self.dir
    }

    /// Whether the full-text index is available.
    pub fn fts_enabled(&self) -> bool {
        self.fts
    }

    /// Persist one exchange: write any spilled blob, then the index/body/FTS rows.
    ///
    /// Returns the generated exchange id. Mirrors `Traffic.record` /
    /// `Traffic.RecordExternal` + `insertExchange` storage layout.
    pub fn record(&self, exchange: Exchange) -> Result<String, TrafficError> {
        let host = if exchange.host.trim().is_empty() {
            host_from_url(&exchange.url)
        } else {
            exchange.host.clone()
        };
        let path = if exchange.path.trim().is_empty() {
            template_path(&url_path(&exchange.url))
        } else {
            exchange.path.clone()
        };
        let req_ct = header_get(&exchange.request_headers, "content-type");
        let resp_ct = header_get(&exchange.response_headers, "content-type");
        let req_len = exchange.request_body.len() as i64;
        let resp_len = exchange.response_body.len() as i64;

        let req_head = build_request_head(
            &exchange.method,
            &exchange.url,
            &host,
            &exchange.request_headers,
        );
        let resp_head = build_response_head(exchange.status, &exchange.response_headers);

        let req_spill = self.spill(&exchange.request_body, &req_ct);
        let resp_spill = self.spill(&exchange.response_body, &resp_ct);

        let now = exchange.timestamp;
        let n = self.seq.fetch_add(1, Ordering::SeqCst) + 1;
        let id = format!("{now}-{:04}", n % 10000);

        let fts_text = format!(
            "{}\n{}\n{}\n{}\n{}",
            exchange.url, req_head, req_spill.index, resp_head, resp_spill.index
        );

        let mut conn = self
            .conn
            .lock()
            .map_err(|_| TrafficError::Invalid("index lock poisoned".into()))?;
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT OR REPLACE INTO exchanges(id,ts,host,method,url_template,url,status,content_type,req_len,resp_len,path)\
             VALUES(?,?,?,?,?,?,?,?,?,?,'')",
            params![
                id,
                now,
                host,
                exchange.method,
                path,
                exchange.url,
                exchange.status as i64,
                resp_ct,
                req_len,
                resp_len
            ],
        )?;
        let rowid = tx.last_insert_rowid();
        tx.execute(
            "INSERT OR REPLACE INTO exchange_bodies(id,req_head,req_body,req_blob,resp_head,resp_body,resp_blob)\
             VALUES(?,?,?,?,?,?,?)",
            params![
                id,
                req_head,
                req_spill.inline,
                req_spill.hash,
                resp_head,
                resp_spill.inline,
                resp_spill.hash
            ],
        )?;
        for h in [req_spill.hash.as_ref(), resp_spill.hash.as_ref()]
            .into_iter()
            .flatten()
        {
            tx.execute(
                "INSERT OR IGNORE INTO blob_refs(hash,exchange_id) VALUES(?,?)",
                params![h, id],
            )?;
        }
        if self.fts {
            tx.execute(
                "INSERT INTO ex_fts(rowid,content) VALUES(?,?)",
                params![rowid, fts_text],
            )?;
        }
        tx.commit()?;
        Ok(id)
    }

    /// Full-text / metadata search, newest first (port of `Traffic.Page`'s broad
    /// query box). `limit` defaults to 100 and is capped at 500; `offset` is the
    /// row offset. Terms shorter than three characters fall back to metadata.
    pub fn search(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<ExchangeMeta>, TrafficError> {
        let size = if limit == 0 || limit > 500 {
            100
        } else {
            limit
        };
        let mut values: Vec<rusqlite::types::Value> = Vec::new();
        let mut cond = String::new();
        let term = query.trim();
        if !term.is_empty() {
            const META: &str = "host LIKE ? OR url LIKE ? OR url_template LIKE ? OR method LIKE ? OR content_type LIKE ? OR CAST(status AS TEXT) LIKE ?";
            let like = format!("%{term}%");
            if let Some(arg) = self.fts_arg(term) {
                cond = format!(
                    " WHERE (({META}) OR rowid IN (SELECT rowid FROM ex_fts WHERE ex_fts MATCH ?))"
                );
                for _ in 0..6 {
                    values.push(like.clone().into());
                }
                values.push(arg.into());
            } else {
                cond = format!(" WHERE ({META})");
                for _ in 0..6 {
                    values.push(like.clone().into());
                }
            }
        }
        values.push((size as i64).into());
        values.push((offset as i64).into());
        let sql = format!(
            "SELECT id,ts,host,method,url_template,url,status,content_type,resp_len,path FROM exchanges{cond} \
             ORDER BY ts DESC, id DESC LIMIT ? OFFSET ?"
        );
        let conn = self
            .conn
            .lock()
            .map_err(|_| TrafficError::Invalid("index lock poisoned".into()))?;
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(values), |r| {
            Ok(ExchangeMeta {
                id: r.get(0)?,
                ts: r.get(1)?,
                host: r.get(2)?,
                method: r.get(3)?,
                url_template: r.get(4)?,
                url: r.get(5)?,
                status: r.get(6)?,
                content_type: r.get(7)?,
                resp_len: r.get(8)?,
                path: r.get(9)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Return one exchange by id, reading spilled bodies from the blob store.
    pub fn get(&self, id: &str) -> Result<Exchange, TrafficError> {
        let row = {
            let conn = self
                .conn
                .lock()
                .map_err(|_| TrafficError::Invalid("index lock poisoned".into()))?;
            conn.query_row(
                "SELECT e.url,e.host,e.method,e.url_template,e.status,b.req_head,b.req_body,b.req_blob,b.resp_head,b.resp_body,b.resp_blob,e.ts FROM exchange_bodies b JOIN exchanges e ON e.id=b.id WHERE b.id=?",
                params![id],
                |r| {
                    Ok(GetRow {
                        url: r.get(0)?,
                        host: r.get(1)?,
                        method: r.get(2)?,
                        path: r.get(3)?,
                        status: r.get::<_, i64>(4)? as u16,
                        req_head: r.get(5)?,
                        req_body: r.get(6)?,
                        req_blob: r.get(7)?,
                        resp_head: r.get(8)?,
                        resp_body: r.get(9)?,
                        resp_blob: r.get(10)?,
                        ts: r.get(11)?,
                    })
                },
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    TrafficError::Invalid(format!("exchange {id} not found"))
                }
                other => TrafficError::Sqlite(other),
            })?
        };

        let req_body = match row.req_blob.as_deref() {
            Some(h) if !h.is_empty() => self.get_blob(h)?,
            _ => row.req_body,
        };
        let resp_body = match row.resp_blob.as_deref() {
            Some(h) if !h.is_empty() => self.get_blob(h)?,
            _ => row.resp_body,
        };

        Ok(Exchange {
            method: row.method,
            url: row.url,
            status: row.status,
            request_headers: parse_headers(&row.req_head),
            request_body: req_body,
            response_headers: parse_headers(&row.resp_head),
            response_body: resp_body,
            host: row.host,
            path: row.path,
            timestamp: row.ts,
        })
    }

    /// Distinct recorded hosts with exchange counts, most recent first
    /// (port of `Traffic.Hosts`).
    pub fn hosts(&self) -> Result<Vec<HostCount>, TrafficError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| TrafficError::Invalid("index lock poisoned".into()))?;
        let mut stmt = conn.prepare(
            "SELECT host, COUNT(*) AS n, MAX(ts) AS last FROM exchanges GROUP BY host ORDER BY last DESC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(HostCount {
                host: r.get(0)?,
                count: r.get(1)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Total recorded exchanges (port of `Traffic.Count`).
    pub fn count(&self) -> Result<i64, TrafficError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| TrafficError::Invalid("index lock poisoned".into()))?;
        let n = conn.query_row("SELECT COUNT(*) FROM exchanges", [], |r| r.get(0))?;
        Ok(n)
    }

    /// Delete every exchange whose host contains `host` (substring), then garbage-
    /// collect unreferenced blobs. Returns rows deleted (port of `DeleteHost`).
    pub fn delete_host(&self, host: &str) -> Result<u64, TrafficError> {
        let like = format!("%{host}%");
        let n = {
            let mut conn = self
                .conn
                .lock()
                .map_err(|_| TrafficError::Invalid("index lock poisoned".into()))?;
            let tx = conn.transaction()?;
            if self.fts {
                tx.execute(
                    "DELETE FROM ex_fts WHERE rowid IN (SELECT rowid FROM exchanges WHERE host LIKE ?)",
                    params![like],
                )?;
            }
            tx.execute(
                "DELETE FROM exchange_bodies WHERE id IN (SELECT id FROM exchanges WHERE host LIKE ?)",
                params![like],
            )?;
            tx.execute(
                "DELETE FROM blob_refs WHERE exchange_id IN (SELECT id FROM exchanges WHERE host LIKE ?)",
                params![like],
            )?;
            let n = tx.execute("DELETE FROM exchanges WHERE host LIKE ?", params![like])?;
            tx.commit()?;
            n as u64
        };
        self.gc_blobs()?;
        Ok(n)
    }

    /// Delete every recorded exchange, garbage-collect blobs and compact the
    /// index (port of `Traffic.DeleteAll`). Returns rows deleted.
    pub fn delete_all(&self) -> Result<u64, TrafficError> {
        let deleted = {
            let mut conn = self
                .conn
                .lock()
                .map_err(|_| TrafficError::Invalid("index lock poisoned".into()))?;
            let tx = conn.transaction()?;
            if self.fts {
                tx.execute(
                    "DELETE FROM ex_fts WHERE rowid IN (SELECT rowid FROM exchanges)",
                    [],
                )?;
            }
            tx.execute("DELETE FROM exchange_bodies", [])?;
            tx.execute("DELETE FROM blob_refs", [])?;
            let n = tx.execute("DELETE FROM exchanges", [])?;
            tx.commit()?;
            n as u64
        };
        self.gc_blobs()?;
        {
            let conn = self
                .conn
                .lock()
                .map_err(|_| TrafficError::Invalid("index lock poisoned".into()))?;
            let _ = conn.execute_batch("VACUUM; PRAGMA wal_checkpoint(TRUNCATE);");
        }
        Ok(deleted)
    }

    /// Ingest a batch of externally captured exchanges (the `/api/traffic/ingest`
    /// path). Rejects empty and oversized (> 50) batches; truncates each body to
    /// 256 KiB; skips binary (image/font/audio/video/octet-stream) responses.
    /// Returns `(ingested, failed)`.
    pub fn ingest_external(
        &self,
        exchanges: Vec<ExternalExchange>,
    ) -> Result<(usize, usize), TrafficError> {
        if exchanges.is_empty() {
            return Err(TrafficError::Invalid("empty exchanges".into()));
        }
        if exchanges.len() > MAX_INGEST_BATCH {
            return Err(TrafficError::Invalid(
                "too many exchanges in one batch".into(),
            ));
        }
        let mut ingested = 0usize;
        let mut failed = 0usize;
        for e in exchanges {
            let mut e = e;
            e.request_body = truncate_body(&e.request_body);
            e.response_body = truncate_body(&e.response_body);
            let resp_ct = header_get(&e.response_headers, "content-type");
            if is_binary_mime(&resp_ct) {
                failed += 1;
                continue;
            }
            match e.into_exchange().and_then(|ex| self.record(ex).map(|_| ())) {
                Ok(()) => ingested += 1,
                Err(_) => failed += 1,
            }
        }
        Ok((ingested, failed))
    }

    /// Content-addressed blob write (SHA-256 → `blobs/<hash[:2]>/<hash>.bin`).
    /// Idempotent: writing the same bytes twice yields the same hash/path.
    pub fn put_blob(&self, data: &[u8]) -> Result<String, TrafficError> {
        let hash = sha256_hex(data);
        let path = self.blob_path(&hash)?;
        if !path.exists() {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&path, data)?;
        }
        Ok(hash)
    }

    /// Read a spilled body by its content hash.
    pub fn get_blob(&self, hash: &str) -> Result<Vec<u8>, TrafficError> {
        let path = self.blob_path(hash)?;
        Ok(fs::read(&path)?)
    }

    // --- internals ---------------------------------------------------------

    fn fts_arg(&self, term: &str) -> Option<String> {
        if !self.fts {
            return None;
        }
        if term.chars().count() < MIN_TRIGRAM {
            return None;
        }
        Some(fts_quote(term))
    }

    fn blob_path(&self, hash: &str) -> Result<PathBuf, TrafficError> {
        if hash.len() != 64 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(TrafficError::Invalid(format!("invalid blob hash {hash:?}")));
        }
        Ok(self
            .dir
            .join("blobs")
            .join(&hash[..2])
            .join(format!("{hash}.bin")))
    }

    fn spill(&self, body: &[u8], content_type: &str) -> StoredBody {
        if body.is_empty() {
            return StoredBody {
                inline: Vec::new(),
                hash: None,
                index: String::new(),
            };
        }
        let text = !is_binary_body(content_type, body);
        let index_text = || -> String {
            if text {
                String::from_utf8_lossy(clip(body, MAX_INDEX_BODY)).into_owned()
            } else {
                String::new()
            }
        };
        if body.len() <= MAX_INLINE_BODY {
            return StoredBody {
                inline: body.to_vec(),
                hash: None,
                index: index_text(),
            };
        }
        match self.put_blob(body) {
            Ok(hash) => {
                let inline = if text {
                    truncate_utf8(body, BLOB_PREVIEW).into_bytes()
                } else {
                    binary_tag(content_type, body).into_bytes()
                };
                StoredBody {
                    inline,
                    hash: Some(hash),
                    index: index_text(),
                }
            }
            Err(_) => StoredBody {
                inline: clip(body, BLOB_PREVIEW).to_vec(),
                hash: None,
                index: index_text(),
            },
        }
    }

    fn gc_blobs(&self) -> Result<(), TrafficError> {
        let refs: HashSet<String> = {
            let conn = self
                .conn
                .lock()
                .map_err(|_| TrafficError::Invalid("index lock poisoned".into()))?;
            let mut stmt = conn.prepare("SELECT DISTINCT hash FROM blob_refs")?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
            let mut set = HashSet::new();
            for h in rows {
                set.insert(h?);
            }
            set
        };

        let root = self.dir.join("blobs");
        if !root.exists() {
            return Ok(());
        }
        for entry in WalkDir::new(&root) {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            if entry.file_type().is_file() {
                let name = entry.file_name().to_string_lossy().to_string();
                let hash = name.strip_suffix(".bin").unwrap_or(&name);
                if !refs.contains(hash) {
                    let _ = fs::remove_file(entry.path());
                }
            }
        }
        let mut dirs: Vec<PathBuf> = WalkDir::new(&root)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_dir())
            .map(|e| e.path().to_path_buf())
            .collect();
        dirs.sort_by_key(|p| std::cmp::Reverse(p.components().count()));
        for d in dirs {
            if d != root {
                let _ = fs::remove_dir(&d);
            }
        }
        Ok(())
    }
}

struct GetRow {
    url: String,
    host: String,
    method: String,
    path: String,
    status: u16,
    req_head: String,
    req_body: Vec<u8>,
    req_blob: Option<String>,
    resp_head: String,
    resp_body: Vec<u8>,
    resp_blob: Option<String>,
    ts: i64,
}

// --- free helpers ----------------------------------------------------------

fn now_unix() -> i64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_secs() as i64,
        Err(_) => 0,
    }
}

fn sha256_hex(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(data);
    format!("{:x}", h.finalize())
}

fn clip(b: &[u8], n: usize) -> &[u8] {
    if b.len() <= n {
        b
    } else {
        &b[..n]
    }
}

fn truncate_utf8(b: &[u8], n: usize) -> String {
    if b.len() <= n {
        return String::from_utf8_lossy(b).into_owned();
    }
    String::from_utf8_lossy(&b[..n]).into_owned()
}

fn hex_encode(b: &[u8]) -> String {
    let mut s = String::with_capacity(b.len() * 2);
    for byte in b {
        s.push_str(&format!("{byte:02x}"));
    }
    s
}

fn binary_tag(content_type: &str, body: &[u8]) -> String {
    let ct = if content_type.trim().is_empty() {
        "application/octet-stream"
    } else {
        content_type.trim()
    };
    let magic = hex_encode(clip(body, 4));
    format!("[binary {ct}, {} bytes, magic={magic}]", body.len())
}

fn is_binary_body(content_type: &str, body: &[u8]) -> bool {
    let ct = content_type.trim().to_lowercase();
    let ct = ct.split(';').next().unwrap_or("").trim();
    for p in BINARY_TYPES {
        if ct.starts_with(p) {
            return true;
        }
    }
    clip(body, 512).contains(&0)
}

fn is_binary_mime(content_type: &str) -> bool {
    let ct = content_type.trim().to_lowercase();
    let ct = ct.split(';').next().unwrap_or("").trim();
    matches!(ct, "application/octet-stream")
        || ct.starts_with("image/")
        || ct.starts_with("font/")
        || ct.starts_with("audio/")
        || ct.starts_with("video/")
}

fn truncate_body(s: &str) -> String {
    let bytes = s.as_bytes();
    if bytes.len() <= MAX_INGEST_BODY_SIZE {
        return s.to_string();
    }
    let mut out = Vec::with_capacity(MAX_INGEST_BODY_SIZE + 32);
    out.extend_from_slice(&bytes[..MAX_INGEST_BODY_SIZE]);
    out.extend_from_slice(b"\n... [truncated by ingest]");
    String::from_utf8_lossy(&out).into_owned()
}

fn header_get(headers: &BTreeMap<String, String>, name: &str) -> String {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.clone())
        .unwrap_or_default()
}

fn build_request_head(
    method: &str,
    url_str: &str,
    host: &str,
    headers: &BTreeMap<String, String>,
) -> String {
    let uri = request_uri(url_str);
    let mut b = String::new();
    b.push_str(method);
    b.push(' ');
    b.push_str(&uri);
    b.push_str(" HTTP/1.1\n");
    if !host.is_empty() {
        b.push_str("Host: ");
        b.push_str(host);
        b.push('\n');
    }
    for (k, v) in headers {
        if k.eq_ignore_ascii_case("host") {
            continue;
        }
        b.push_str(k);
        b.push_str(": ");
        b.push_str(v);
        b.push('\n');
    }
    b
}

fn build_response_head(status: u16, headers: &BTreeMap<String, String>) -> String {
    let mut b = format!("HTTP {status}\n");
    for (k, v) in headers {
        b.push_str(k);
        b.push_str(": ");
        b.push_str(v);
        b.push('\n');
    }
    b
}

fn parse_headers(head: &str) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    let mut first = true;
    for line in head.lines() {
        if first {
            first = false;
            continue;
        }
        if line.is_empty() {
            continue;
        }
        if let Some((k, v)) = line.split_once(": ") {
            map.insert(k.to_string(), v.to_string());
        } else if let Some(pos) = line.find(':') {
            map.insert(line[..pos].to_string(), line[pos + 1..].trim().to_string());
        }
    }
    map
}

fn request_uri(url_str: &str) -> String {
    if let Ok(u) = url::Url::parse(url_str) {
        let p = u.path();
        match u.query() {
            Some(q) => format!("{p}?{q}"),
            None => p.to_string(),
        }
    } else {
        "/".to_string()
    }
}

fn url_path(url_str: &str) -> String {
    url::Url::parse(url_str)
        .map(|u| u.path().to_string())
        .unwrap_or_default()
}

fn host_from_url(url_str: &str) -> String {
    url::Url::parse(url_str)
        .ok()
        .and_then(|u| u.host_str().map(|s| s.to_string()))
        .unwrap_or_default()
}

fn fts_quote(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

/// Collapse high-cardinality path segments into placeholders so the store is not
/// flooded by instances: `/user/123` → `/user/{id}` (port of `db.TemplatePath`).
fn template_path(path: &str) -> String {
    if path.is_empty() {
        return "/".to_string();
    }
    let segs: Vec<&str> = path.split('/').collect();
    let mut out: Vec<String> = Vec::with_capacity(segs.len());
    for s in segs {
        if s.is_empty() {
            out.push(String::new());
        } else if s.bytes().all(|c| c.is_ascii_digit()) {
            out.push("{id}".to_string());
        } else if is_uuid(s) {
            out.push("{uuid}".to_string());
        } else if s.len() >= 16 && s.bytes().all(|c| c.is_ascii_hexdigit()) {
            out.push("{hex}".to_string());
        } else if s.len() >= 24
            && s.bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
        {
            out.push("{token}".to_string());
        } else {
            out.push(s.to_string());
        }
    }
    out.join("/")
}

fn is_uuid(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 36 {
        return false;
    }
    for (i, &c) in b.iter().enumerate() {
        match i {
            8 | 13 | 18 | 23 => {
                if c != b'-' {
                    return false;
                }
            }
            _ => {
                if !c.is_ascii_hexdigit() {
                    return false;
                }
            }
        }
    }
    true
}

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
                "traffic-test-{tag}-{}-{nanos}-{n}",
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

    #[test]
    fn blob_put_get_content_addressed_and_dedup() {
        let tmp = TempDir::new("blob");
        let store = TrafficStore::open(tmp.0.clone()).expect("open");
        let data = b"hello traffic blob".repeat(1000);

        let h1 = store.put_blob(&data).expect("put 1");
        let h2 = store.put_blob(&data).expect("put 2");
        assert_eq!(h1, h2, "content-addressed hash is stable");
        assert_eq!(store.get_blob(&h1).expect("get"), data);

        let path = tmp.0.join("blobs").join(&h1[..2]).join(format!("{h1}.bin"));
        assert!(path.exists(), "blob written at content-addressed path");
        assert!(store.get_blob("nothex").is_err(), "invalid hash rejected");
    }

    #[test]
    fn external_exchange_into_exchange() {
        let ext = ExternalExchange {
            method: "post".to_string(),
            url: "https://api.example.com/v1/login".to_string(),
            status: 0,
            request_headers: BTreeMap::from([(
                "content-type".to_string(),
                "application/json".to_string(),
            )]),
            request_body: "{\"a\":1}".to_string(),
            response_headers: BTreeMap::new(),
            response_body: "ok".to_string(),
        };
        let ex = ext.into_exchange().expect("convert");
        assert_eq!(ex.method, "POST", "method upper-cased");
        assert_eq!(ex.status, 200, "zero status defaults to 200");
        assert_eq!(ex.host, "api.example.com");
        assert_eq!(ex.path, "/v1/login");
        assert_eq!(ex.request_body, b"{\"a\":1}");

        assert!(ExternalExchange {
            method: "GET".to_string(),
            url: "not a url".to_string(),
            ..Default::default()
        }
        .into_exchange()
        .is_err());
    }

    #[test]
    fn ingest_truncates_bodies_over_256k() {
        let tmp = TempDir::new("trunc");
        let store = TrafficStore::open(tmp.0.clone()).expect("open");
        let big = "A".repeat(MAX_INGEST_BODY_SIZE + 4096);
        let ext = ExternalExchange {
            method: "POST".to_string(),
            url: "https://api.example.com/upload".to_string(),
            status: 200,
            request_headers: BTreeMap::new(),
            request_body: big,
            response_headers: BTreeMap::from([(
                "content-type".to_string(),
                "text/plain".to_string(),
            )]),
            response_body: "ok".to_string(),
        };
        let (ingested, failed) = store.ingest_external(vec![ext]).expect("ingest");
        assert_eq!((ingested, failed), (1, 0));

        let meta = store
            .search("upload", 10, 0)
            .expect("search")
            .into_iter()
            .next()
            .expect("found");
        let ex = store.get(&meta.id).expect("get");
        assert!(
            ex.request_body.ends_with(b"\n... [truncated by ingest]"),
            "body truncated with marker"
        );
        assert!(
            ex.request_body.len() <= MAX_INGEST_BODY_SIZE + 32,
            "truncated body length bounded (got {})",
            ex.request_body.len()
        );
    }

    #[test]
    fn ingest_skips_binary_mime() {
        let tmp = TempDir::new("binary");
        let store = TrafficStore::open(tmp.0.clone()).expect("open");
        let cases = [
            "image/png",
            "font/woff2",
            "audio/mpeg",
            "video/mp4",
            "application/octet-stream",
        ];
        let batch: Vec<ExternalExchange> = cases
            .iter()
            .map(|ct| ExternalExchange {
                method: "GET".to_string(),
                url: format!("https://cdn.example.com/asset?ct={ct}"),
                status: 200,
                request_headers: BTreeMap::new(),
                request_body: String::new(),
                response_headers: BTreeMap::from([("Content-Type".to_string(), ct.to_string())]),
                response_body: "binarydata".to_string(),
            })
            .collect();
        let (ingested, failed) = store.ingest_external(batch).expect("ingest");
        assert_eq!(ingested, 0);
        assert_eq!(failed, cases.len());
        assert_eq!(store.count().expect("count"), 0);
    }

    #[test]
    fn ingest_batch_cap_50() {
        let tmp = TempDir::new("cap");
        let store = TrafficStore::open(tmp.0.clone()).expect("open");
        let make = |i: usize| ExternalExchange {
            method: "GET".to_string(),
            url: format!("https://example.com/{i}"),
            status: 200,
            request_headers: BTreeMap::new(),
            request_body: String::new(),
            response_headers: BTreeMap::from([(
                "content-type".to_string(),
                "text/plain".to_string(),
            )]),
            response_body: "ok".to_string(),
        };
        let fifty: Vec<ExternalExchange> = (0..50).map(make).collect();
        assert!(store.ingest_external(fifty).is_ok());

        let fifty_one: Vec<ExternalExchange> = (0..51).map(make).collect();
        let err = store.ingest_external(fifty_one).unwrap_err();
        assert!(matches!(err, TrafficError::Invalid(_)));
        assert!(store.ingest_external(Vec::new()).is_err());
    }
}
