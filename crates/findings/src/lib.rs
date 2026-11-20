//! Findings data layer — port of ARTEX `db/findings.go` + `db/finding_retests.go`.
//!
//! Models the standalone `findings` table (which survives task deletion) and the
//! manual `finding_retests` table, with the list/get/create/update/delete/stats/
//! grouping operations and the retest lifecycle the API and agent tools need.
//!
//! Behaviour-preserving notes (deviations from the Go source, by design):
//! - The `findings` table has no `updated_at` column and Go's `DBFinding` has no
//!   `UpdatedAt`; `Finding::updated_at` is therefore always `None` and omitted
//!   from JSON, matching Go.
//! - Severity whitelists `critical`/`high`/`medium`/`low` only. There is no
//!   `info` level and no CHECK constraint (validated server-side in Go too).
//! - Asset-scope subtree filtering (`db/finding_assets.go` tree building) and the
//!   archived cold-storage aggregate merge in `FindingStats` are not ported; they
//!   belong to other subsystems and are outside this crate's surface.
//! - `CreateFindingRetest` snapshots the finding and dedups the active run, but
//!   the conversation + first-message rows are created by the conversation layer
//!   (not this crate); `conversation_id` is left `NULL`.
//! - Status-change push notification (`SetFindingStatusTx` notify path) is a
//!   server concern; the low-level DB effect (`UPDATE findings SET status`) is
//!   what is ported here.

use database::Db;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::postgres::PgRow;
use sqlx::{Postgres, QueryBuilder, Row};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors from the findings data layer.
#[derive(Debug, thiserror::Error)]
pub enum FindingError {
    #[error("sql: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("finding not found")]
    NotFound,
    #[error("invalid: {0}")]
    Invalid(String),
}

// ---------------------------------------------------------------------------
// Severity / status whitelists (validated server-side, no DB CHECK)
// ---------------------------------------------------------------------------

pub const SEVERITY_CRITICAL: &str = "critical";
pub const SEVERITY_HIGH: &str = "high";
pub const SEVERITY_MEDIUM: &str = "medium";
pub const SEVERITY_LOW: &str = "low";

/// Reports whether `s` is a known severity level (Go `ValidSeverity`).
pub fn valid_severity(s: &str) -> bool {
    matches!(
        s,
        SEVERITY_CRITICAL | SEVERITY_HIGH | SEVERITY_MEDIUM | SEVERITY_LOW
    )
}

pub const STATUS_PENDING: &str = "pending";
pub const STATUS_IN_PROGRESS: &str = "in_progress";
pub const STATUS_CONFIRMED: &str = "confirmed";
pub const STATUS_RESOLVED: &str = "resolved";
pub const STATUS_FIXED: &str = "fixed";
pub const STATUS_FALSE_POSITIVE: &str = "false_positive";
pub const STATUS_IGNORED: &str = "ignored";
pub const STATUS_DUPLICATE: &str = "duplicate";
pub const STATUS_RISK_ACCEPTED: &str = "risk_accepted";

/// Reports whether `s` is a known triage state (Go `ValidFindingStatus`).
pub fn valid_status(s: &str) -> bool {
    matches!(
        s,
        STATUS_PENDING
            | STATUS_IN_PROGRESS
            | STATUS_CONFIRMED
            | STATUS_RESOLVED
            | STATUS_FIXED
            | STATUS_FALSE_POSITIVE
            | STATUS_IGNORED
            | STATUS_DUPLICATE
            | STATUS_RISK_ACCEPTED
    )
}

/// Task filter sentinel for findings whose task is absent (Go
/// `FindingUnassignedTask`): rows created without a task and rows retained after
/// their originating task was deleted (`ON DELETE SET NULL`).
pub const FINDING_UNASSIGNED_TASK: &str = "__unassigned__";

/// Advisory-lock key shared with the evidence filesystem GC (Go
/// `findingEvidenceLockKey`); `soft_delete` takes it to serialize with evidence
/// readers/exporters.
const FINDING_EVIDENCE_LOCK_KEY: i64 = 7337741004;

// ---------------------------------------------------------------------------
// Finding
// ---------------------------------------------------------------------------

/// A row in the standalone `findings` table. It persists across task deletion
/// unless the caller explicitly requests related finding cleanup.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct Finding {
    pub id: i64,
    pub task_id: Option<i64>,
    pub node_id: Option<i64>,
    /// Vulnerability class (human-readable category).
    pub vulnclass: String,
    /// Vulnerability name (readable title); empty falls back to `vulnclass`.
    pub name: String,
    pub severity: String,
    pub summary: String,
    pub evidence: String,
    pub worker: String,
    pub asset_ids: Vec<i64>,
    pub status: String,
    /// Detailed Markdown report; only populated by [`FindingStore::get`], empty
    /// in list queries (kept out of list APIs to avoid payload bloat).
    pub report: String,
    pub evidence_version: i64,
    pub report_evidence_version: i64,
    /// Proof-of-exploitation level 0-4 (S1); 0 = none declared.
    pub poe_level: i16,
    pub verification_status: String,
    pub verification_note: String,
    /// What was tested and did not work; empty = not given.
    pub counterevidence: String,
    /// Why the conclusion holds; empty = not given.
    pub confidence_rationale: String,
    pub created_at: String,
    /// Present for API-shape completeness; the `findings` table has no
    /// `updated_at` column, so this is always `None` (skipped in JSON, matching
    /// Go's `DBFinding`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

/// The common `SELECT` list for findings (no `report`; `created_at`/`asset_ids`
/// rendered as text to match Go's `::text` scanning). Every computed column is
/// aliased so `row.get("name")` resolves by name.
const FINDING_SELECT: &str = "SELECT f.id, f.task_id, f.node_id, f.vulnclass, \
COALESCE(f.name,'') AS name, f.severity, f.summary, \
f.evidence, f.worker, f.asset_ids::text AS asset_ids, COALESCE(f.status,'pending') AS status, \
f.created_at::text AS created_at, f.evidence_version, f.report_evidence_version, f.poe_level, \
COALESCE(f.verification_status,'unverified') AS verification_status, \
COALESCE(f.verification_note,'') AS verification_note, \
f.counterevidence, f.confidence_rationale";

fn scan_finding(r: &PgRow) -> Finding {
    let asset_raw: String = r.get("asset_ids");
    Finding {
        id: r.get("id"),
        task_id: r.get("task_id"),
        node_id: r.get("node_id"),
        vulnclass: r.get("vulnclass"),
        name: r.get("name"),
        severity: r.get("severity"),
        summary: r.get("summary"),
        evidence: r.get("evidence"),
        worker: r.get("worker"),
        asset_ids: serde_json::from_str(&asset_raw).unwrap_or_default(),
        status: r.get("status"),
        report: String::new(),
        evidence_version: r.get("evidence_version"),
        report_evidence_version: r.get("report_evidence_version"),
        poe_level: r.get("poe_level"),
        verification_status: r.get("verification_status"),
        verification_note: r.get("verification_note"),
        counterevidence: r.get("counterevidence"),
        confidence_rationale: r.get("confidence_rationale"),
        created_at: r.get("created_at"),
        updated_at: None,
    }
}

// ---------------------------------------------------------------------------
// Filter
// ---------------------------------------------------------------------------

/// Narrows a findings query. Empty-string fields mean "no filter on that column"
/// (Go `FindingFilter`). `sort` is `"severity"` (severity desc, then newest) or
/// anything else (newest first).
#[derive(Debug, Clone, Default)]
pub struct FindingFilter {
    pub severity: String,
    pub status: String,
    pub verification_status: String,
    pub vulnclass: String,
    /// Task id in string form; empty/invalid = no task filter,
    /// [`FINDING_UNASSIGNED_TASK`] = findings with no live task.
    pub task_id: String,
    /// Fuzzy search keyword over name/class/summary/evidence/report body.
    pub query: String,
    /// `"severity"` or `"time"` (anything else = newest first).
    pub sort: String,
}

/// Escape ILIKE wildcards so user input matches literally (Go `where`'s
/// `strings.NewReplacer`).
fn escape_like(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '%' => out.push_str("\\%"),
            '_' => out.push_str("\\_"),
            _ => out.push(c),
        }
    }
    out
}

/// Append `" WHERE "` (first condition) or `" AND "` (subsequent) to `qb`.
fn push_cond(qb: &mut QueryBuilder<'_, Postgres>, wrote: &mut bool) {
    if *wrote {
        qb.push(" AND ");
    } else {
        qb.push(" WHERE ");
        *wrote = true;
    }
}

/// Push the shared WHERE clause (all values parameterized) built from `f`
/// (Go `FindingFilter.where`).
fn apply_filter(qb: &mut QueryBuilder<'_, Postgres>, f: &FindingFilter) {
    let mut wrote = false;
    if !f.severity.is_empty() {
        push_cond(qb, &mut wrote);
        qb.push("f.severity = ").push_bind(f.severity.clone());
    }
    if !f.status.is_empty() {
        push_cond(qb, &mut wrote);
        qb.push("f.status = ").push_bind(f.status.clone());
    }
    if !f.verification_status.is_empty() {
        push_cond(qb, &mut wrote);
        qb.push("f.verification_status = ")
            .push_bind(f.verification_status.clone());
    }
    if !f.vulnclass.is_empty() {
        push_cond(qb, &mut wrote);
        qb.push("f.vulnclass = ").push_bind(f.vulnclass.clone());
    }
    if f.task_id == FINDING_UNASSIGNED_TASK {
        push_cond(qb, &mut wrote);
        qb.push("(f.task_id IS NULL OR t.id IS NULL)");
    } else if let Ok(tid) = f.task_id.parse::<i64>() {
        if tid > 0 {
            push_cond(qb, &mut wrote);
            qb.push("f.task_id = ").push_bind(tid);
        }
    }
    let query = f.query.trim();
    if !query.is_empty() {
        let pattern = format!("%{}%", escape_like(query));
        push_cond(qb, &mut wrote);
        qb.push("(COALESCE(f.name, '') ILIKE ")
            .push_bind(pattern.clone())
            .push(" ESCAPE '\\' OR ");
        qb.push("f.vulnclass ILIKE ")
            .push_bind(pattern.clone())
            .push(" ESCAPE '\\' OR ");
        qb.push("f.summary ILIKE ")
            .push_bind(pattern.clone())
            .push(" ESCAPE '\\' OR ");
        qb.push("f.evidence ILIKE ")
            .push_bind(pattern.clone())
            .push(" ESCAPE '\\' OR ");
        qb.push("COALESCE(f.report, '') ILIKE ")
            .push_bind(pattern)
            .push(" ESCAPE '\\')");
    }
}

fn order_clause(sort: &str) -> &'static str {
    if sort == "severity" {
        " ORDER BY CASE f.severity WHEN 'critical' THEN 4 WHEN 'high' THEN 3 \
         WHEN 'medium' THEN 2 WHEN 'low' THEN 1 ELSE 0 END DESC, \
         f.created_at DESC, f.id DESC"
    } else {
        " ORDER BY f.created_at DESC, f.id DESC"
    }
}

// ---------------------------------------------------------------------------
// Stats / groups
// ---------------------------------------------------------------------------

/// Whole-table aggregates powering the findings page's stat cards and vuln-class
/// filter (Go `FindingStats`).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct FindingStats {
    pub total: i64,
    pub pending: i64,
    pub critical: i64,
    pub high: i64,
    pub medium: i64,
    pub low: i64,
    pub vulnclasses: Vec<String>,
    pub tasks: Vec<FindingTaskOption>,
}

/// One entry in the findings page's "by task" filter (Go `FindingTaskOption`).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct FindingTaskOption {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub count: i64,
}

/// One task-level bucket in the global findings view (Go `FindingGroup`).
/// `task_id` is `None` for findings that never had a task and for findings
/// retained after task deletion; those share one "unassigned" bucket.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct FindingGroup {
    pub task_id: Option<i64>,
    pub task_name: String,
    pub task_description: String,
    pub task_status: String,
    pub count: i64,
    pub critical: i64,
    pub high: i64,
    pub medium: i64,
    pub low: i64,
    pub last_found_at: String,
}

fn groups_order_clause(sort: &str) -> &'static str {
    if sort == "severity" {
        " ORDER BY MAX(CASE f.severity WHEN 'critical' THEN 4 WHEN 'high' THEN 3 \
         WHEN 'medium' THEN 2 WHEN 'low' THEN 1 ELSE 0 END) DESC, \
         MAX(f.created_at) DESC, t.id DESC NULLS LAST"
    } else {
        " ORDER BY MAX(f.created_at) DESC, t.id DESC NULLS LAST"
    }
}

// ---------------------------------------------------------------------------
// Retests
// ---------------------------------------------------------------------------

/// An immutable historical test once its conversation turn ends (Go
/// `FindingRetest`). `snapshot` is only loaded for the agent, never sent with
/// the history list.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct FindingRetest {
    pub id: i64,
    pub finding_id: i64,
    pub conversation_id: Option<i64>,
    pub status: String,
    pub verdict: String,
    pub notes: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<Value>,
    pub summary: String,
    pub evidence: String,
    pub error: String,
    pub created_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}

impl FindingRetest {
    /// The first chat message for a freshly created retest (Go `InitialMessage`).
    pub fn initial_message(&self) -> String {
        let mut msg = format!(
            "Please retest vulnerability #{}. First call get_finding_retest_context to read \
             the original evidence and constraints associated with this conversation, then \
             perform targeted verification, and finally call record_finding_retest_result to \
             save the conclusion.",
            self.finding_id
        );
        if !self.notes.is_empty() {
            msg.push_str("\n\nAdditional notes for this retest:\n");
            msg.push_str(&self.notes);
        }
        msg
    }
}

/// Small status payload polled by the findings list (Go `ActiveFindingRetest`).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct ActiveFindingRetest {
    pub id: i64,
    pub finding_id: i64,
    pub conversation_id: i64,
    pub status: String,
}

const RETEST_COLS: &str = "id, finding_id, conversation_id, status, verdict, notes, \
summary, evidence, error, created_at::text AS created_at, \
started_at::text AS started_at, finished_at::text AS finished_at";

fn scan_retest(r: &PgRow) -> FindingRetest {
    FindingRetest {
        id: r.get("id"),
        finding_id: r.get("finding_id"),
        conversation_id: r.get("conversation_id"),
        status: r.get("status"),
        verdict: r.get("verdict"),
        notes: r.get("notes"),
        snapshot: None,
        summary: r.get("summary"),
        evidence: r.get("evidence"),
        error: r.get("error"),
        created_at: r.get("created_at"),
        started_at: r.get("started_at"),
        finished_at: r.get("finished_at"),
    }
}

/// Truncate `s` to at most `n` runes, adding an ellipsis when cut (Go
/// `truncateRunes`).
fn truncate_runes(s: &str, n: usize) -> String {
    let s = s.trim();
    if s.chars().count() <= n {
        return s.to_string();
    }
    let truncated: String = s.chars().take(n).collect();
    format!("{}\u{2026}", truncated)
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

/// Findings store bound to a DB handle.
pub struct FindingStore {
    db: Db,
}

impl FindingStore {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    /// List one page of findings matching `filter`, newest/severity first, plus
    /// the total count of matching rows (Go `ListFindingsPage`, expressed with
    /// `limit`/`offset` instead of `page`/`pageSize`).
    pub async fn list(
        &self,
        filter: &FindingFilter,
        limit: i64,
        offset: i64,
    ) -> Result<(Vec<Finding>, i64), FindingError> {
        let limit = if limit <= 0 { 20 } else { limit };
        let offset = if offset < 0 { 0 } else { offset };

        let mut cb = QueryBuilder::new(
            "SELECT COUNT(*) AS total FROM findings f LEFT JOIN tasks t ON f.task_id = t.id",
        );
        apply_filter(&mut cb, filter);
        let total: i64 = cb.build().fetch_one(self.db.pool()).await?.get("total");
        if total == 0 || offset >= total {
            return Ok((Vec::new(), total));
        }

        let mut qb = QueryBuilder::new(FINDING_SELECT);
        qb.push(" FROM findings f LEFT JOIN tasks t ON f.task_id = t.id");
        apply_filter(&mut qb, filter);
        qb.push(order_clause(filter.sort.as_str()));
        qb.push(" LIMIT ")
            .push_bind(limit)
            .push(" OFFSET ")
            .push_bind(offset);
        let rows = qb.build().fetch_all(self.db.pool()).await?;
        Ok((rows.iter().map(scan_finding).collect(), total))
    }

    /// Return a single finding (with the full Markdown report) or `None` when no
    /// row has that id (Go `GetFinding`).
    pub async fn get(&self, id: i64) -> Result<Option<Finding>, FindingError> {
        let sql = format!(
            "{}, COALESCE(f.report,'') AS report \
             FROM findings f LEFT JOIN tasks t ON f.task_id = t.id WHERE f.id = $1",
            FINDING_SELECT
        );
        let row = sqlx::query(&sql)
            .bind(id)
            .fetch_optional(self.db.pool())
            .await?;
        Ok(row.map(|r| {
            let mut f = scan_finding(&r);
            f.report = r.get("report");
            f
        }))
    }

    /// Insert a finding into the standalone table. `task_id`/`node_id` may be 0
    /// (stored as NULL); `asset_ids` may be empty. Returns the new finding id
    /// (Go `AddFinding`).
    #[allow(clippy::too_many_arguments)]
    pub async fn create(
        &self,
        task_id: i64,
        node_id: i64,
        vulnclass: &str,
        name: &str,
        severity: &str,
        summary: &str,
        evidence: &str,
        worker: &str,
        asset_ids: &[i64],
    ) -> Result<i64, FindingError> {
        let task_opt = if task_id > 0 { Some(task_id) } else { None };
        let node_opt = if node_id > 0 { Some(node_id) } else { None };
        let asset_json = Value::Array(asset_ids.iter().map(|&id| Value::from(id)).collect());
        let row = sqlx::query(
            "INSERT INTO findings \
             (task_id, node_id, vulnclass, name, severity, summary, evidence, worker, asset_ids) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) RETURNING id",
        )
        .bind(task_opt)
        .bind(node_opt)
        .bind(vulnclass)
        .bind(name)
        .bind(severity)
        .bind(summary)
        .bind(evidence)
        .bind(worker)
        .bind(&asset_json)
        .fetch_one(self.db.pool())
        .await?;
        Ok(row.get("id"))
    }

    /// Update one finding's triage state (Go low-level `SetFindingStatus`; the
    /// notifying variant is a server concern). Returns rows affected (0 = no
    /// finding with that id).
    pub async fn set_status(&self, id: i64, status: &str) -> Result<u64, FindingError> {
        let res = sqlx::query("UPDATE findings SET status=$1 WHERE id=$2")
            .bind(status)
            .bind(id)
            .execute(self.db.pool())
            .await?;
        Ok(res.rows_affected())
    }

    /// Update one text column and mirror the value into the originating
    /// exploration node's payload (Go `setFindingCol`). `col`/`json_key` MUST be
    /// trusted constants, never user input. Returns rows affected.
    async fn set_finding_col(
        &self,
        id: i64,
        col: &str,
        json_key: &str,
        val: &str,
    ) -> Result<u64, FindingError> {
        let sql = format!("UPDATE findings SET {col}=$1 WHERE id=$2 RETURNING node_id");
        let row = sqlx::query(&sql)
            .bind(val)
            .bind(id)
            .fetch_optional(self.db.pool())
            .await?;
        let Some(row) = row else {
            return Ok(0);
        };
        let node_id: Option<i64> = row.get("node_id");
        if let Some(nid) = node_id {
            // Best-effort node sync; failures are ignored, matching Go.
            let _ = sqlx::query(
                "UPDATE exploration_nodes \
                 SET payload = jsonb_set(payload, $2, to_jsonb($1::text)) WHERE id = $3",
            )
            .bind(val)
            .bind(vec![json_key.to_string()])
            .bind(nid)
            .execute(self.db.pool())
            .await;
        }
        Ok(1)
    }

    /// Update one finding's severity (+ node payload sync). Returns rows affected
    /// (Go `SetFindingSeverity`).
    pub async fn set_severity(&self, id: i64, severity: &str) -> Result<u64, FindingError> {
        self.set_finding_col(id, "severity", "severity", severity)
            .await
    }

    /// Update one finding's vulnerability name (+ node payload sync). Empty name
    /// is allowed (Go `SetFindingName`).
    pub async fn set_name(&self, id: i64, name: &str) -> Result<u64, FindingError> {
        self.set_finding_col(id, "name", "name", name).await
    }

    /// Update one finding's vulnerability class (+ node payload sync) (Go
    /// `SetFindingVulnClass`).
    pub async fn set_vulnclass(&self, id: i64, vulnclass: &str) -> Result<u64, FindingError> {
        self.set_finding_col(id, "vulnclass", "vulnclass", vulnclass)
            .await
    }

    /// Remove a finding entirely: the standalone row and its originating
    /// exploration node (`kind='finding'`), so it disappears from the list, the
    /// per-task tab, and the graph alike (Go `DeleteFinding`). Returns rows
    /// affected (0 = no finding with that id).
    pub async fn soft_delete(&self, id: i64) -> Result<u64, FindingError> {
        let mut tx = self.db.pool().begin().await?;
        sqlx::query("SELECT pg_advisory_xact_lock($1)")
            .bind(FINDING_EVIDENCE_LOCK_KEY)
            .execute(&mut *tx)
            .await?;
        let row = sqlx::query("DELETE FROM findings WHERE id=$1 RETURNING node_id")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?;
        let Some(row) = row else {
            tx.rollback().await?;
            return Ok(0);
        };
        let node_id: Option<i64> = row.get("node_id");
        if let Some(nid) = node_id {
            sqlx::query("DELETE FROM exploration_nodes WHERE id=$1 AND kind='finding'")
                .bind(nid)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(1)
    }

    /// Whole-table counts (by severity + pending), the sorted set of distinct
    /// vuln classes, and tasks that have findings (Go `FindingStats`). The
    /// archived cold-storage aggregate merge is not ported (see module docs).
    pub async fn stats(&self) -> Result<FindingStats, FindingError> {
        let row = sqlx::query(
            "SELECT COUNT(*) AS total, \
             COUNT(*) FILTER (WHERE status = 'pending') AS pending, \
             COUNT(*) FILTER (WHERE severity = 'critical') AS critical, \
             COUNT(*) FILTER (WHERE severity = 'high') AS high, \
             COUNT(*) FILTER (WHERE severity = 'medium') AS medium, \
             COUNT(*) FILTER (WHERE severity = 'low') AS low \
             FROM findings",
        )
        .fetch_one(self.db.pool())
        .await?;
        let mut st = FindingStats {
            total: row.get("total"),
            pending: row.get("pending"),
            critical: row.get("critical"),
            high: row.get("high"),
            medium: row.get("medium"),
            low: row.get("low"),
            vulnclasses: Vec::new(),
            tasks: Vec::new(),
        };

        let rows = sqlx::query(
            "SELECT DISTINCT vulnclass FROM findings WHERE vulnclass <> '' ORDER BY vulnclass",
        )
        .fetch_all(self.db.pool())
        .await?;
        for r in &rows {
            st.vulnclasses.push(r.get("vulnclass"));
        }

        let rows = sqlx::query(
            "SELECT f.task_id, COALESCE(t.name,'') AS name, \
             COALESCE(t.description,'') AS description, COUNT(*) AS count \
             FROM findings f LEFT JOIN tasks t ON f.task_id = t.id \
             WHERE f.task_id IS NOT NULL \
             GROUP BY f.task_id, t.name, t.description \
             ORDER BY MAX(f.created_at) DESC",
        )
        .fetch_all(self.db.pool())
        .await?;
        for r in &rows {
            st.tasks.push(FindingTaskOption {
                id: r.get("task_id"),
                name: r.get("name"),
                description: r.get("description"),
                count: r.get("count"),
            });
        }
        Ok(st)
    }

    /// Return a page of task groups matching `filter` plus the group count and
    /// the finding count (independent totals) (Go `ListFindingGroups`,
    /// expressed with `limit`/`offset`).
    pub async fn list_groups(
        &self,
        filter: &FindingFilter,
        limit: i64,
        offset: i64,
    ) -> Result<(Vec<FindingGroup>, i64, i64), FindingError> {
        let limit = if limit <= 0 { 10 } else { limit };
        let offset = if offset < 0 { 0 } else { offset };

        let mut cb = QueryBuilder::new(
            "SELECT COUNT(*) AS group_total, \
             COALESCE(SUM(finding_count),0)::bigint AS finding_total \
             FROM (SELECT COUNT(*) AS finding_count \
             FROM findings f LEFT JOIN tasks t ON f.task_id = t.id",
        );
        apply_filter(&mut cb, filter);
        cb.push(
            " GROUP BY t.id, t.name, t.description, t.status, t.paused, t.queued) \
                  grouped_findings",
        );
        let crow = cb.build().fetch_one(self.db.pool()).await?;
        let group_total: i64 = crow.get("group_total");
        let finding_total: i64 = crow.get("finding_total");
        if group_total == 0 || offset >= group_total {
            return Ok((Vec::new(), group_total, finding_total));
        }

        let mut qb = QueryBuilder::new(
            "SELECT t.id AS id, COALESCE(t.name,'') AS name, \
             COALESCE(t.description,'') AS description, \
             COALESCE(CASE WHEN t.status IN ('done','failed','timeout') THEN t.status \
                 WHEN t.queued THEN 'queued' WHEN t.paused THEN 'paused' \
                 ELSE t.status END,'') AS status, \
             COUNT(*) AS count, \
             COUNT(*) FILTER (WHERE f.severity='critical') AS critical, \
             COUNT(*) FILTER (WHERE f.severity='high') AS high, \
             COUNT(*) FILTER (WHERE f.severity='medium') AS medium, \
             COUNT(*) FILTER (WHERE f.severity='low') AS low, \
             MAX(f.created_at)::text AS last_found_at",
        );
        qb.push(" FROM findings f LEFT JOIN tasks t ON f.task_id = t.id");
        apply_filter(&mut qb, filter);
        qb.push(" GROUP BY t.id, t.name, t.description, t.status, t.paused, t.queued");
        qb.push(groups_order_clause(filter.sort.as_str()));
        qb.push(" LIMIT ")
            .push_bind(limit)
            .push(" OFFSET ")
            .push_bind(offset);
        let rows = qb.build().fetch_all(self.db.pool()).await?;
        let mut out = Vec::with_capacity(rows.len());
        for r in &rows {
            out.push(FindingGroup {
                task_id: r.get("id"),
                task_name: r.get("name"),
                task_description: r.get("description"),
                task_status: r.get("status"),
                count: r.get("count"),
                critical: r.get("critical"),
                high: r.get("high"),
                medium: r.get("medium"),
                low: r.get("low"),
                last_found_at: r.get("last_found_at"),
            });
        }
        Ok((out, group_total, finding_total))
    }

    // -----------------------------------------------------------------------
    // finding_retests
    // -----------------------------------------------------------------------

    /// List a finding's retests, newest first (Go `ListFindingRetests`).
    pub async fn list_retests(&self, finding_id: i64) -> Result<Vec<FindingRetest>, FindingError> {
        let sql = format!(
            "SELECT {} FROM finding_retests WHERE finding_id=$1 ORDER BY id DESC",
            RETEST_COLS
        );
        let rows = sqlx::query(&sql)
            .bind(finding_id)
            .fetch_all(self.db.pool())
            .await?;
        Ok(rows.iter().map(scan_retest).collect())
    }

    /// Snapshot the source finding and create its retest row, deduplicating an
    /// existing active run (Go `CreateFindingRetest`, minus the conversation +
    /// first-message rows owned by the conversation layer). Returns
    /// [`FindingError::NotFound`] when the finding does not exist.
    pub async fn create_retest(
        &self,
        finding_id: i64,
        notes: &str,
    ) -> Result<FindingRetest, FindingError> {
        let mut tx = self.db.pool().begin().await?;
        let snap_row = sqlx::query(
            "SELECT jsonb_build_object( \
                 'finding', to_jsonb(f), \
                 'assets', COALESCE((SELECT jsonb_agg(to_jsonb(a)) FROM assets a \
                     WHERE f.asset_ids @> to_jsonb(ARRAY[a.id])), '[]'::jsonb), \
                 'constraints', COALESCE((SELECT jsonb_agg(to_jsonb(c)) \
                     FROM task_constraints c JOIN tasks t ON t.exploration_id=c.exploration_id \
                     WHERE t.id=f.task_id), '[]'::jsonb))::text AS snapshot \
             FROM findings f WHERE f.id=$1 FOR UPDATE OF f",
        )
        .bind(finding_id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(snap_row) = snap_row else {
            tx.rollback().await?;
            return Err(FindingError::NotFound);
        };
        let snapshot: Value =
            serde_json::from_str(&snap_row.get::<String, _>("snapshot")).unwrap_or(Value::Null);

        let existing = sqlx::query(&format!(
            "SELECT {} FROM finding_retests \
             WHERE finding_id=$1 AND status IN ('pending','running')",
            RETEST_COLS
        ))
        .bind(finding_id)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(row) = existing {
            let retest = scan_retest(&row);
            tx.commit().await?;
            return Ok(retest);
        }

        let notes = notes.trim();
        let row = sqlx::query(&format!(
            "INSERT INTO finding_retests(finding_id, notes, snapshot) \
             VALUES ($1,$2,$3) RETURNING {}",
            RETEST_COLS
        ))
        .bind(finding_id)
        .bind(notes)
        .bind(&snapshot)
        .fetch_one(&mut *tx)
        .await?;
        let retest = scan_retest(&row);
        tx.commit().await?;
        Ok(retest)
    }

    /// List active (pending/running) retests with a conversation (Go
    /// `ListActiveFindingRetests`).
    pub async fn list_active_retests(&self) -> Result<Vec<ActiveFindingRetest>, FindingError> {
        let rows = sqlx::query(
            "SELECT id, finding_id, conversation_id, status FROM finding_retests \
             WHERE status IN ('pending','running') AND conversation_id IS NOT NULL ORDER BY id",
        )
        .fetch_all(self.db.pool())
        .await?;
        let mut out = Vec::with_capacity(rows.len());
        for r in &rows {
            out.push(ActiveFindingRetest {
                id: r.get("id"),
                finding_id: r.get("finding_id"),
                conversation_id: r.get("conversation_id"),
                status: r.get("status"),
            });
        }
        Ok(out)
    }

    /// Return the retest bound to a conversation, with its snapshot loaded (Go
    /// `FindingRetestForConversation`).
    pub async fn get_retest_for_conversation(
        &self,
        conversation_id: i64,
    ) -> Result<Option<FindingRetest>, FindingError> {
        let sql = format!(
            "SELECT {} FROM finding_retests WHERE conversation_id=$1",
            RETEST_COLS
        );
        let row = sqlx::query(&sql)
            .bind(conversation_id)
            .fetch_optional(self.db.pool())
            .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let mut retest = scan_retest(&row);
        let id = retest.id;
        let snap: Option<String> =
            sqlx::query_scalar("SELECT snapshot::text FROM finding_retests WHERE id=$1")
                .bind(id)
                .fetch_optional(self.db.pool())
                .await?;
        retest.snapshot = snap.and_then(|s| serde_json::from_str(&s).ok());
        Ok(Some(retest))
    }

    /// Mark a pending retest running (Go `StartFindingRetest`). Returns whether
    /// the transition happened.
    pub async fn start_retest(&self, id: i64) -> Result<bool, FindingError> {
        let res = sqlx::query(
            "UPDATE finding_retests SET status='running', started_at=now() \
             WHERE id=$1 AND status='pending'",
        )
        .bind(id)
        .execute(self.db.pool())
        .await?;
        Ok(res.rows_affected() == 1)
    }

    /// Persist the single retest conclusion (Go `RecordFindingRetestResult`).
    /// Identical retries are safe; a second verdict is refused.
    pub async fn record_retest_result(
        &self,
        conversation_id: i64,
        verdict: &str,
        summary: &str,
        evidence: &str,
    ) -> Result<(), FindingError> {
        if verdict != "reproduced" && verdict != "fixed" && verdict != "inconclusive" {
            return Err(FindingError::Invalid(
                "verdict must be reproduced / fixed / inconclusive".into(),
            ));
        }
        let summary = summary.trim();
        let evidence = evidence.trim();
        if summary.is_empty() || evidence.is_empty() {
            return Err(FindingError::Invalid(
                "summary and evidence must not be empty; when the result cannot be confirmed, \
                 describe the actual checks and the blocking reason"
                    .into(),
            ));
        }
        if summary.len() > 16000 || evidence.len() > 128000 {
            return Err(FindingError::Invalid(
                "retest conclusion is too long (summary <= 16KB, evidence <= 128KB)".into(),
            ));
        }
        let res = sqlx::query(
            "UPDATE finding_retests SET verdict=$2, summary=$3, evidence=$4 \
             WHERE conversation_id=$1 AND status='running' \
             AND (verdict='' OR (verdict=$2 AND summary=$3 AND evidence=$4))",
        )
        .bind(conversation_id)
        .bind(verdict)
        .bind(summary)
        .bind(evidence)
        .execute(self.db.pool())
        .await?;
        if res.rows_affected() == 0 {
            return Err(FindingError::Invalid(
                "this retest has ended or has not started; please start a new retest from the \
                 finding details"
                    .into(),
            ));
        }
        Ok(())
    }

    /// Seal a retest result. Cancellation/failure takes precedence over a staged
    /// verdict; only a newly completed fixed verdict updates triage (Go
    /// `FinishFindingRetest`, minus the status-change push).
    pub async fn finish_retest(
        &self,
        id: i64,
        status: &str,
        reason: &str,
    ) -> Result<(), FindingError> {
        if status != "completed" && status != "failed" && status != "stopped" {
            return Err(FindingError::Invalid(
                "invalid terminal retest status".into(),
            ));
        }
        let mut tx = self.db.pool().begin().await?;
        let frow = sqlx::query(
            "SELECT f.id FROM findings f \
             WHERE f.id=(SELECT finding_id FROM finding_retests WHERE id=$1) FOR UPDATE OF f",
        )
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(frow) = frow else {
            tx.rollback().await?;
            return Ok(());
        };
        let finding_id: i64 = frow.get("id");

        let upd = sqlx::query(
            "UPDATE finding_retests SET \
             status=CASE WHEN $2='completed' AND verdict='' THEN 'failed' ELSE $2 END, \
             error=CASE WHEN $2='completed' AND verdict='' THEN \
                 'Agent did not save the retest conclusion; please review the conversation and \
                  retest again' ELSE $3 END, \
             finished_at=now() \
             WHERE id=$1 AND status IN ('pending','running') \
             RETURNING status, verdict, summary",
        )
        .bind(id)
        .bind(status)
        .bind(reason)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(upd) = upd else {
            tx.rollback().await?;
            return Ok(());
        };
        let final_status: String = upd.get("status");
        let verdict: String = upd.get("verdict");
        let summary: String = upd.get("summary");

        if final_status == "completed" && (verdict == "reproduced" || verdict == "fixed") {
            let note = format!(
                "retest #{}: {} \u{2014} {}",
                id,
                verdict,
                truncate_runes(&summary, 200)
            );
            sqlx::query(
                "UPDATE findings SET verification_status='verified', verification_note=$2 \
                 WHERE id=$1 AND verification_status IN \
                 ('unverified','not_assessed','blocked')",
            )
            .bind(finding_id)
            .bind(note)
            .execute(&mut *tx)
            .await?;
        }
        if final_status == "completed" && verdict == "fixed" {
            sqlx::query("UPDATE findings SET status='fixed' WHERE id=$1")
                .bind(finding_id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Seal an unfinished retest whose conversation could not be loaded (Go
    /// `FailPendingRetestForConversation`).
    pub async fn fail_pending_retest_for_conversation(
        &self,
        conversation_id: i64,
        reason: &str,
    ) -> Result<(), FindingError> {
        sqlx::query(
            "UPDATE finding_retests SET status='failed', error=$2, finished_at=now() \
             WHERE conversation_id=$1 AND status IN ('pending','running')",
        )
        .bind(conversation_id)
        .bind(reason)
        .execute(self.db.pool())
        .await?;
        Ok(())
    }

    /// Mark every unfinished retest stopped after a restart (Go
    /// `RecoverFindingRetests`).
    pub async fn recover_retests(&self) -> Result<(), FindingError> {
        sqlx::query(
            "UPDATE finding_retests SET status='stopped', \
             error='Server restarted; the retest was interrupted, please start it again', \
             finished_at=now() WHERE status IN ('pending','running')",
        )
        .execute(self.db.pool())
        .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severity_whitelist() {
        for s in ["critical", "high", "medium", "low"] {
            assert!(valid_severity(s), "{s} should be valid");
        }
        // No "info" level; the schema has no CHECK and Go's whitelist excludes it.
        for s in ["info", "", "Critical", "urgent"] {
            assert!(!valid_severity(s), "{s} should be invalid");
        }
    }

    #[test]
    fn status_whitelist() {
        for s in [
            "pending",
            "in_progress",
            "confirmed",
            "resolved",
            "fixed",
            "false_positive",
            "ignored",
            "duplicate",
            "risk_accepted",
        ] {
            assert!(valid_status(s), "{s} should be valid");
        }
        for s in ["", "done", "open"] {
            assert!(!valid_status(s), "{s} should be invalid");
        }
    }

    #[test]
    fn escape_like_escapes_wildcards() {
        assert_eq!(escape_like("a%b_c\\d"), "a\\%b\\_c\\\\d");
        assert_eq!(escape_like("plain"), "plain");
    }

    #[test]
    fn truncate_runes_adds_ellipsis() {
        assert_eq!(truncate_runes("hello", 10), "hello");
        assert_eq!(truncate_runes("  hi  ", 10), "hi");
        assert_eq!(truncate_runes("hello", 3), "hel\u{2026}");
        // Multi-byte runes are counted, not bytes.
        assert_eq!(truncate_runes("héllo", 2), "hé\u{2026}");
    }

    #[test]
    fn retest_initial_message_includes_notes() {
        let retest = FindingRetest {
            finding_id: 7,
            notes: "focus on the admin path".into(),
            ..Default::default()
        };
        let msg = retest.initial_message();
        assert!(msg.contains("#7"));
        assert!(msg.contains("get_finding_retest_context"));
        assert!(msg.contains("Additional notes for this retest:\nfocus on the admin path"));

        let bare = FindingRetest {
            finding_id: 9,
            ..Default::default()
        };
        assert!(!bare.initial_message().contains("Additional notes"));
    }

    #[test]
    fn finding_json_shape() {
        let f = Finding {
            id: 3,
            severity: SEVERITY_HIGH.into(),
            asset_ids: vec![1, 2],
            ..Default::default()
        };
        let v = serde_json::to_value(&f).unwrap();
        assert_eq!(v["id"], 3);
        assert_eq!(v["severity"], "high");
        assert_eq!(v["asset_ids"], serde_json::json!([1, 2]));
        // updated_at is absent (no such column; skipped when None).
        assert!(v.get("updated_at").is_none());
        // status default reflects the DB default via scan; here Default is "".
        assert_eq!(v["status"], "");
    }
}
