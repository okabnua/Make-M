//! Exploration graph data layer — read-side port of ARTEX `db/exploration.go`.
//!
//! Covers the READ operations the API needs: the per-task graph (`nodes` +
//! `edges`), frontier/intent listings, the activity stream (forward cursor reads
//! and single-record detail), main-session segments and token aggregations.
//!
//! The write/agent path (node creation, intent claiming/heartbeats, soft deletes,
//! cancel/cleanup, inheritance/source stores) is intentionally out of scope for
//! this crate and remains in the original Go engine for now.
//!
//! JSONB columns are selected as text and parsed with `serde_json` so the crate
//! does not need sqlx's `json` feature.

use chrono::{DateTime, Utc};
use database::Db;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::postgres::PgRow;
use sqlx::Row;

/// Exploration node kinds (`exploration_nodes.kind`), port of `db/constants.go`.
pub mod node_kind {
    /// DEPRECATED: legacy task root; new tasks seed an origin fact instead.
    pub const BEGIN: &str = "begin";
    /// A task objective.
    pub const GOAL: &str = "goal";
    /// An exploration direction (planner-generated).
    pub const INTENT: &str = "intent";
    /// A worker's exploration result/conclusion (incl. negative results).
    pub const FACT: &str = "fact";
    /// A confirmed vulnerability (report_finding), distinct from a fact.
    pub const FINDING: &str = "finding";
    pub const HINT: &str = "hint";
    /// A compressed fold of cold intents/facts; lossless, restorable by id.
    pub const DIGEST: &str = "digest";
}

/// Exploration node states, port of the state constants in `db/constants.go`.
///
/// The `exploration_nodes` CHECK constraint is kind-specific; these are the
/// cross-kind markers the original declares as named constants. Digest states
/// (`active`/`superseded`) live here too for convenience.
pub mod node_state {
    /// Marks the task root fact (KindFact) seeded at task creation.
    pub const ORIGIN: &str = "origin";
    /// Marks an intent soft-deleted by the user.
    pub const DELETED: &str = "deleted";
    /// A digest that renders in graph_overview.
    pub const ACTIVE: &str = "active";
    /// A digest retired by major compaction (members repointed to a new digest).
    pub const SUPERSEDED: &str = "superseded";
}

/// Exploration edge relations (`exploration_edges.rel`).
pub mod edge_rel {
    pub const SPAWNS: &str = "spawns";
    pub const DERIVED_FROM: &str = "derived_from";
    pub const YIELDS: &str = "yields";
    pub const PROVES: &str = "proves";
    /// digest --covers--> member.
    pub const COVERS: &str = "covers";
}

/// Default node cap for `Nodes`/graph reads (Go `Nodes(2000)`).
pub const DEFAULT_NODES_LIMIT: i64 = 2000;
/// Default edge cap for graph reads (Go `Edges(5000)`).
pub const GRAPH_EDGE_LIMIT: i64 = 5000;
/// Default frontier page size (Go `Frontier(50)`).
pub const DEFAULT_FRONTIER_LIMIT: i64 = 50;
/// Default intent list cap (Go `ListByKind(KindIntent, 500)`).
pub const DEFAULT_INTENTS_LIMIT: i64 = 500;
/// Default forward activity page size (Go `ActivityList` default 300).
pub const DEFAULT_ACTIVITY_LIMIT: i64 = 300;

/// Errors from the exploration data layer.
#[derive(Debug, thiserror::Error)]
pub enum ExplorationError {
    #[error("sql: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

/// One typed reasoning node (`exploration_nodes`).
///
/// The subset of fields that appear on the wire matches the original Go `db.Node`
/// json tags; the remaining columns (`exploration_id`, `content_version`,
/// `cold_since_round`, `last_heartbeat_at`, `unclaim_token`, `updated_at`) are
/// carried so callers have the full row without a second query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExplorationNode {
    pub id: i64,
    #[serde(skip_serializing_if = "is_false")]
    pub exploration_id: i64,
    pub kind: String,
    pub payload: Value,
    pub priority: i32,
    pub state: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub origin: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub owner: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub blocked_reason: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub delete_reason: String,
    pub content_version: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cold_since_round: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_heartbeat_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unclaim_token: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// One edge row (`exploration_edges`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExplorationEdge {
    pub exploration_id: i64,
    pub src_id: i64,
    pub dst_id: i64,
    pub rel: String,
}

/// The full per-task exploration chain: nodes + edges for graph viz.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExplorationGraph {
    pub nodes: Vec<ExplorationNode>,
    pub edges: Vec<ExplorationEdge>,
}

/// One worker execution step (`activity`).
///
/// `detail` is the full blob; list reads leave it empty (lazy, fetched via
/// [`ExplorationStore::activity_detail`]), matching the original `db.Activity`.
/// `seq` is the activity row's `id` (the wire cursor).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Activity {
    pub exploration_id: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_id: Option<i64>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub worker: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub kind: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub tool: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub tool_use_id: String,
    pub is_error: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub summary: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub detail: String,
    pub metadata: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_read_tokens: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_write_tokens: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub main_seg: Option<i32>,
    pub seq: i64,
    pub created_at: DateTime<Utc>,
}

/// A forward activity page plus the new cursor (max id seen).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityPage {
    pub items: Vec<Activity>,
    pub cursor: i64,
}

/// One resettable main-agent conversation segment (`main_sessions`).
///
/// Segment 0 is the implicit original session (never stored); it is always
/// returned last with `created_at = None`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MainSession {
    pub seq: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<DateTime<Utc>>,
}

/// Per-worker token aggregate (`TokenStatsByWorker`).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TokenUsage {
    pub worker: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
}

/// Per-session token aggregate (`TokenStatsBySession`).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionTokenUsage {
    pub session: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
}

/// The combined `/tokens` payload: per-worker, per-session and whole-task totals.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TokenUsageReport {
    pub workers: Vec<TokenUsage>,
    pub sessions: Vec<SessionTokenUsage>,
    pub total: TokenUsage,
}

fn is_false(v: &i64) -> bool {
    *v == 0
}

/// Parse a JSONB column selected as text; an empty/null body becomes `{}`.
fn parse_json(raw: &str) -> Result<Value, ExplorationError> {
    if raw.trim().is_empty() {
        return Ok(Value::Object(Default::default()));
    }
    Ok(serde_json::from_str(raw)?)
}

fn scan_node(row: &PgRow) -> Result<ExplorationNode, ExplorationError> {
    let payload = parse_json(&row.try_get::<String, _>("payload")?)?;
    Ok(ExplorationNode {
        id: row.try_get("id")?,
        exploration_id: row.try_get("exploration_id")?,
        kind: row.try_get("kind")?,
        payload,
        priority: row.try_get("priority")?,
        state: row.try_get("state")?,
        origin: row.try_get("origin")?,
        owner: row.try_get("owner")?,
        blocked_reason: row.try_get("blocked_reason")?,
        delete_reason: row.try_get("delete_reason")?,
        content_version: row.try_get("content_version")?,
        cold_since_round: row.try_get("cold_since_round")?,
        last_heartbeat_at: row.try_get("last_heartbeat_at")?,
        unclaim_token: row.try_get("unclaim_token")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn scan_activity(row: &PgRow, with_detail: bool) -> Result<Activity, ExplorationError> {
    let metadata = parse_json(&row.try_get::<String, _>("metadata")?)?;
    let detail = if with_detail {
        row.try_get::<Option<String>, _>("detail")?
            .unwrap_or_default()
    } else {
        String::new()
    };
    Ok(Activity {
        exploration_id: row.try_get("exploration_id")?,
        node_id: row.try_get("node_id")?,
        worker: row.try_get("worker")?,
        kind: row.try_get("kind")?,
        tool: row.try_get("tool")?,
        tool_use_id: row.try_get("tool_use_id")?,
        is_error: row.try_get("is_error")?,
        summary: row.try_get("summary")?,
        detail,
        metadata,
        input_tokens: row.try_get("input_tokens")?,
        output_tokens: row.try_get("output_tokens")?,
        cache_read_tokens: row.try_get("cache_read_tokens")?,
        cache_write_tokens: row.try_get("cache_write_tokens")?,
        main_seg: row.try_get("main_seg")?,
        seq: row.try_get("seq")?,
        created_at: row.try_get("created_at")?,
    })
}

const NODE_SELECT: &str =
    "SELECT id, exploration_id, kind, payload::text AS payload, priority, state, \
COALESCE(origin,'') AS origin, COALESCE(owner,'') AS owner, \
COALESCE(blocked_reason,'') AS blocked_reason, COALESCE(delete_reason,'') AS delete_reason, \
content_version, cold_since_round, last_heartbeat_at, unclaim_token, created_at, updated_at \
FROM exploration_nodes";

const ACTIVITY_COLUMNS: &str =
    "id AS seq, exploration_id, node_id, COALESCE(worker,'') AS worker, \
COALESCE(kind,'') AS kind, COALESCE(tool,'') AS tool, COALESCE(tool_use_id,'') AS tool_use_id, \
is_error, COALESCE(summary,'') AS summary, metadata::text AS metadata, \
input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, main_seg, created_at";

/// Read-only handle onto the exploration graph + activity tables.
#[derive(Clone)]
pub struct ExplorationStore {
    db: Db,
}

impl ExplorationStore {
    /// Create a store bound to the shared database pool.
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    /// The whole exploration chain (nodes + edges) for graph viz, matching the
    /// Go `/api/exploration/graph` reads: up to 2000 nodes (id order) and 5000
    /// edges.
    pub async fn graph(&self, exploration_id: i64) -> Result<ExplorationGraph, ExplorationError> {
        let nodes = self
            .nodes(exploration_id, None, DEFAULT_NODES_LIMIT, 0)
            .await?;
        let edges = self.edges(exploration_id, GRAPH_EDGE_LIMIT).await?;
        Ok(ExplorationGraph { nodes, edges })
    }

    /// One page of nodes for an exploration, oldest first by id (`ORDER BY id`),
    /// optionally filtered by kind. `offset` is a row offset.
    pub async fn nodes(
        &self,
        exploration_id: i64,
        kind: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<ExplorationNode>, ExplorationError> {
        let rows = match kind {
            Some(k) => {
                sqlx::query(&format!(
                "{NODE_SELECT} WHERE exploration_id=$1 AND kind=$2 ORDER BY id LIMIT $3 OFFSET $4"
            ))
                .bind(exploration_id)
                .bind(k)
                .bind(limit)
                .bind(offset)
                .fetch_all(self.db.pool())
                .await?
            }
            None => {
                sqlx::query(&format!(
                    "{NODE_SELECT} WHERE exploration_id=$1 ORDER BY id LIMIT $2 OFFSET $3"
                ))
                .bind(exploration_id)
                .bind(limit)
                .bind(offset)
                .fetch_all(self.db.pool())
                .await?
            }
        };
        rows.iter().map(scan_node).collect()
    }

    /// Exploration edges for graph viz (no inherent order; ordered by src/dst
    /// for deterministic reads).
    pub async fn edges(
        &self,
        exploration_id: i64,
        limit: i64,
    ) -> Result<Vec<ExplorationEdge>, ExplorationError> {
        let rows = sqlx::query(
            "SELECT exploration_id, src_id, dst_id, rel FROM exploration_edges \
WHERE exploration_id=$1 ORDER BY src_id, dst_id, rel LIMIT $2",
        )
        .bind(exploration_id)
        .bind(limit)
        .fetch_all(self.db.pool())
        .await?;
        let mut out = Vec::with_capacity(rows.len());
        for row in &rows {
            out.push(ExplorationEdge {
                exploration_id: row.try_get("exploration_id")?,
                src_id: row.try_get("src_id")?,
                dst_id: row.try_get("dst_id")?,
                rel: row.try_get("rel")?,
            });
        }
        Ok(out)
    }

    /// Open intents ordered by priority desc then id (FIFO tiebreak), matching
    /// Go `Frontier` (default cap 50).
    pub async fn frontier(
        &self,
        exploration_id: i64,
    ) -> Result<Vec<ExplorationNode>, ExplorationError> {
        let rows = sqlx::query(&format!(
            "{NODE_SELECT} WHERE exploration_id=$1 AND kind='intent' AND state='open' \
ORDER BY priority DESC, id ASC LIMIT $2"
        ))
        .bind(exploration_id)
        .bind(DEFAULT_FRONTIER_LIMIT)
        .fetch_all(self.db.pool())
        .await?;
        rows.iter().map(scan_node).collect()
    }

    /// Intents of an exploration, newest first (`ORDER BY id DESC`), optionally
    /// filtered by state — matches Go `ListByKind(KindIntent, limit)`.
    pub async fn intents(
        &self,
        exploration_id: i64,
        state: Option<&str>,
    ) -> Result<Vec<ExplorationNode>, ExplorationError> {
        let rows = match state {
            Some(s) => {
                sqlx::query(&format!(
                    "{NODE_SELECT} WHERE exploration_id=$1 AND kind='intent' AND state=$2 \
ORDER BY id DESC LIMIT $3"
                ))
                .bind(exploration_id)
                .bind(s)
                .bind(DEFAULT_INTENTS_LIMIT)
                .fetch_all(self.db.pool())
                .await?
            }
            None => {
                sqlx::query(&format!(
                    "{NODE_SELECT} WHERE exploration_id=$1 AND kind='intent' \
ORDER BY id DESC LIMIT $2"
                ))
                .bind(exploration_id)
                .bind(DEFAULT_INTENTS_LIMIT)
                .fetch_all(self.db.pool())
                .await?
            }
        };
        rows.iter().map(scan_node).collect()
    }

    /// Forward activity read: rows with `seq > since`, optionally scoped to one
    /// intent (`node_id`). Returns items and the new cursor (max seq seen),
    /// matching Go `ActivityList`.
    pub async fn activity(
        &self,
        exploration_id: i64,
        since: i64,
        intent: Option<i64>,
        limit: i64,
    ) -> Result<ActivityPage, ExplorationError> {
        let limit = if limit <= 0 {
            DEFAULT_ACTIVITY_LIMIT
        } else {
            limit
        };
        let rows = match intent {
            Some(node_id) => {
                sqlx::query(&format!(
                    "SELECT {ACTIVITY_COLUMNS} FROM activity \
WHERE exploration_id=$1 AND node_id=$2 AND id>$3 ORDER BY id LIMIT $4"
                ))
                .bind(exploration_id)
                .bind(node_id)
                .bind(since)
                .bind(limit)
                .fetch_all(self.db.pool())
                .await?
            }
            None => {
                sqlx::query(&format!(
                    "SELECT {ACTIVITY_COLUMNS} FROM activity \
WHERE exploration_id=$1 AND id>$2 ORDER BY id LIMIT $3"
                ))
                .bind(exploration_id)
                .bind(since)
                .bind(limit)
                .fetch_all(self.db.pool())
                .await?
            }
        };
        let mut items = Vec::with_capacity(rows.len());
        let mut cursor = since;
        for row in &rows {
            let a = scan_activity(row, false)?;
            if a.seq > cursor {
                cursor = a.seq;
            }
            items.push(a);
        }
        Ok(ActivityPage { items, cursor })
    }

    /// One activity row by seq, with its full `detail` blob. `None` when absent.
    pub async fn activity_at(
        &self,
        exploration_id: i64,
        seq: i64,
    ) -> Result<Option<Activity>, ExplorationError> {
        let row = sqlx::query(&format!(
            "SELECT {ACTIVITY_COLUMNS}, detail FROM activity \
WHERE exploration_id=$1 AND id=$2"
        ))
        .bind(exploration_id)
        .bind(seq)
        .fetch_optional(self.db.pool())
        .await?;
        row.as_ref().map(|r| scan_activity(r, true)).transpose()
    }

    /// Lazily return the full `detail` blob for one step. `None` when absent.
    pub async fn activity_detail(
        &self,
        exploration_id: i64,
        seq: i64,
    ) -> Result<Option<String>, ExplorationError> {
        let row = sqlx::query("SELECT detail FROM activity WHERE exploration_id=$1 AND id=$2")
            .bind(exploration_id)
            .bind(seq)
            .fetch_optional(self.db.pool())
            .await?;
        match row {
            None => Ok(None),
            Some(r) => Ok(r.try_get::<Option<String>, _>("detail")?),
        }
    }

    /// Main-session segments newest-first, always ending with the implicit
    /// original segment 0, matching Go `ListMainSessions`.
    pub async fn main_sessions(
        &self,
        exploration_id: i64,
    ) -> Result<Vec<MainSession>, ExplorationError> {
        let rows = sqlx::query(
            "SELECT seq, created_at FROM main_sessions \
WHERE exploration_id=$1 ORDER BY seq DESC",
        )
        .bind(exploration_id)
        .fetch_all(self.db.pool())
        .await?;
        let mut out = Vec::with_capacity(rows.len() + 1);
        for row in &rows {
            out.push(MainSession {
                seq: row.try_get("seq")?,
                created_at: row.try_get("created_at")?,
            });
        }
        out.push(MainSession {
            seq: 0,
            created_at: None,
        });
        Ok(out)
    }

    /// Sum token usage per worker for this exploration (kind='result' rows).
    pub async fn token_stats_by_worker(
        &self,
        exploration_id: i64,
    ) -> Result<Vec<TokenUsage>, ExplorationError> {
        let rows = sqlx::query(
            "SELECT COALESCE(worker,'') AS worker, \
COALESCE(SUM(input_tokens),0) AS input_tokens, \
COALESCE(SUM(output_tokens),0) AS output_tokens, \
COALESCE(SUM(cache_read_tokens),0) AS cache_read_tokens, \
COALESCE(SUM(cache_write_tokens),0) AS cache_write_tokens \
FROM activity WHERE exploration_id=$1 AND kind='result' \
GROUP BY worker ORDER BY worker",
        )
        .bind(exploration_id)
        .fetch_all(self.db.pool())
        .await?;
        let mut out = Vec::with_capacity(rows.len());
        for row in &rows {
            out.push(TokenUsage {
                worker: row.try_get("worker")?,
                input_tokens: row.try_get("input_tokens")?,
                output_tokens: row.try_get("output_tokens")?,
                cache_read_tokens: row.try_get("cache_read_tokens")?,
                cache_write_tokens: row.try_get("cache_write_tokens")?,
            });
        }
        Ok(out)
    }

    /// Aggregate every persisted completed run by session key
    /// (`main:<seg>` | `plan` | `intent:<id>`), matching Go
    /// `TokenStatsBySession`.
    pub async fn token_stats_by_session(
        &self,
        exploration_id: i64,
    ) -> Result<Vec<SessionTokenUsage>, ExplorationError> {
        let rows = sqlx::query(
            "SELECT CASE \
WHEN COALESCE(a.worker,'')='mainagent' THEN 'main:' || COALESCE(a.main_seg,0)::text \
WHEN COALESCE(a.worker,'')='planner' THEN 'plan' \
WHEN n.id IS NOT NULL THEN 'intent:' || n.id::text \
ELSE '' END AS session_key, \
COALESCE(SUM(a.input_tokens),0) AS input_tokens, \
COALESCE(SUM(a.output_tokens),0) AS output_tokens, \
COALESCE(SUM(a.cache_read_tokens),0) AS cache_read_tokens, \
COALESCE(SUM(a.cache_write_tokens),0) AS cache_write_tokens \
FROM activity a \
LEFT JOIN exploration_nodes n \
  ON n.id=a.node_id AND n.exploration_id=a.exploration_id AND n.kind='intent' \
WHERE a.exploration_id=$1 AND a.kind='result' \
  AND (COALESCE(a.worker,'') IN ('mainagent','planner') OR n.id IS NOT NULL) \
GROUP BY session_key ORDER BY session_key",
        )
        .bind(exploration_id)
        .fetch_all(self.db.pool())
        .await?;
        let mut out = Vec::with_capacity(rows.len());
        for row in &rows {
            out.push(SessionTokenUsage {
                session: row.try_get("session_key")?,
                input_tokens: row.try_get("input_tokens")?,
                output_tokens: row.try_get("output_tokens")?,
                cache_read_tokens: row.try_get("cache_read_tokens")?,
                cache_write_tokens: row.try_get("cache_write_tokens")?,
            });
        }
        Ok(out)
    }

    /// Whole-task token total across all agents (kind='result' rows).
    pub async fn token_total(&self, exploration_id: i64) -> Result<TokenUsage, ExplorationError> {
        let row = sqlx::query(
            "SELECT COALESCE(SUM(input_tokens),0) AS input_tokens, \
COALESCE(SUM(output_tokens),0) AS output_tokens, \
COALESCE(SUM(cache_read_tokens),0) AS cache_read_tokens, \
COALESCE(SUM(cache_write_tokens),0) AS cache_write_tokens \
FROM activity WHERE exploration_id=$1 AND kind='result'",
        )
        .bind(exploration_id)
        .fetch_one(self.db.pool())
        .await?;
        Ok(TokenUsage {
            worker: String::new(),
            input_tokens: row.try_get("input_tokens")?,
            output_tokens: row.try_get("output_tokens")?,
            cache_read_tokens: row.try_get("cache_read_tokens")?,
            cache_write_tokens: row.try_get("cache_write_tokens")?,
        })
    }

    /// The combined `/tokens` report: per-worker, per-session and whole-task.
    pub async fn token_usage(
        &self,
        exploration_id: i64,
    ) -> Result<TokenUsageReport, ExplorationError> {
        let workers = self.token_stats_by_worker(exploration_id).await?;
        let sessions = self.token_stats_by_session(exploration_id).await?;
        let total = self.token_total(exploration_id).await?;
        Ok(TokenUsageReport {
            workers,
            sessions,
            total,
        })
    }

    /// Current maximum activity seq for the exploration (0 when empty).
    pub async fn activity_max_id(&self, exploration_id: i64) -> Result<i64, ExplorationError> {
        let row = sqlx::query(
            "SELECT COALESCE(MAX(id),0) AS max_id FROM activity WHERE exploration_id=$1",
        )
        .bind(exploration_id)
        .fetch_one(self.db.pool())
        .await?;
        Ok(row.try_get("max_id")?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_kind_constants_match_original() {
        assert_eq!(node_kind::BEGIN, "begin");
        assert_eq!(node_kind::GOAL, "goal");
        assert_eq!(node_kind::INTENT, "intent");
        assert_eq!(node_kind::FACT, "fact");
        assert_eq!(node_kind::FINDING, "finding");
        assert_eq!(node_kind::HINT, "hint");
        assert_eq!(node_kind::DIGEST, "digest");
    }

    #[test]
    fn node_state_constants_match_original() {
        assert_eq!(node_state::ORIGIN, "origin");
        assert_eq!(node_state::DELETED, "deleted");
        assert_eq!(node_state::ACTIVE, "active");
        assert_eq!(node_state::SUPERSEDED, "superseded");
    }

    #[test]
    fn edge_rel_constants_match_original() {
        assert_eq!(edge_rel::SPAWNS, "spawns");
        assert_eq!(edge_rel::DERIVED_FROM, "derived_from");
        assert_eq!(edge_rel::YIELDS, "yields");
        assert_eq!(edge_rel::PROVES, "proves");
        assert_eq!(edge_rel::COVERS, "covers");
    }

    #[test]
    fn parse_json_handles_empty() {
        assert_eq!(parse_json("").unwrap(), Value::Object(Default::default()));
        assert_eq!(parse_json("  ").unwrap(), Value::Object(Default::default()));
        assert_eq!(parse_json("{\"a\":1}").unwrap()["a"], 1);
    }
}
