//! Integration test: engine intent lifecycle against a real Postgres.
//! Requires ARTEX_PG_DSN; skips when Postgres is unreachable.

use async_trait::async_trait;
use database::open;
use engine::{Engine, EngineConfig, EngineError, PlannedIntent, Planner, Worker, WorkerResult};
use exploration::{ExplorationNode, ExplorationStore};
use std::sync::Arc;

fn dsn() -> Option<String> {
    std::env::var("ARTEX_PG_DSN")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| Some("postgres://artex:artex@127.0.0.1:55432/artex?sslmode=disable".into()))
}

/// A planner that dispatches one intent once, then nothing.
struct OneShotPlanner;
#[async_trait]
impl Planner for OneShotPlanner {
    async fn plan(&self, exploration_id: i64) -> Result<Vec<PlannedIntent>, EngineError> {
        // Dispatch one intent if none exists yet.
        let store = STORE.get().unwrap();
        if store
            .has_active_intent(exploration_id)
            .await
            .unwrap_or(false)
        {
            return Ok(Vec::new());
        }
        Ok(vec![PlannedIntent {
            description: "scan the target".into(),
            priority: 5,
            asset_ids: vec![],
            origin: "itest".into(),
        }])
    }
}

/// A worker that completes the intent and reports the goal met.
struct CompletingWorker;
#[async_trait]
impl Worker for CompletingWorker {
    async fn execute(&self, _intent: &ExplorationNode) -> Result<WorkerResult, EngineError> {
        Ok(WorkerResult {
            state: "done".into(),
            summary: "done".into(),
            facts: vec!["fact one".into()],
            goal_met: true,
        })
    }
}

static STORE: std::sync::OnceLock<Arc<ExplorationStore>> = std::sync::OnceLock::new();

#[tokio::test]
async fn engine_dispatch_and_complete() {
    let Some(dsn) = dsn() else { return };
    let db = match open(&dsn).await {
        Ok(d) => d,
        Err(e) => {
            eprintln!("skipping: postgres unavailable ({e})");
            return;
        }
    };

    // Clean slate.
    sqlx::query("DELETE FROM exploration_anchors WHERE node_id IN (SELECT id FROM exploration_nodes WHERE origin='itest')")
        .execute(db.pool()).await.ok();
    sqlx::query("DELETE FROM exploration_nodes WHERE origin='itest'")
        .execute(db.pool())
        .await
        .ok();

    // Create an exploration with an open goal.
    let exp_id: i64 = sqlx::query_scalar(
        "INSERT INTO explorations(description, goal, status) VALUES ('itest','reach goal','open') RETURNING id",
    )
    .fetch_one(db.pool())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO exploration_nodes(exploration_id, kind, payload, state) \
         VALUES ($1, 'goal', '{\"description\":\"reach goal\"}'::jsonb, 'open')",
    )
    .bind(exp_id)
    .execute(db.pool())
    .await
    .unwrap();

    let store = Arc::new(ExplorationStore::new(db));
    STORE.set(store.clone()).ok();

    let engine = Engine::new(
        store.clone(),
        Arc::new(OneShotPlanner),
        Arc::new(CompletingWorker),
        EngineConfig::default(),
    );

    // Step 1: no active intent + open goal → planner dispatches one, worker claims & completes.
    let goal_met = engine.step(exp_id).await.expect("step 1");
    assert!(goal_met, "worker reports goal met");

    // The intent node should exist and be done.
    let done: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM exploration_nodes WHERE exploration_id=$1 AND kind='intent' AND state='done'",
    )
    .bind(exp_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(done, 1, "one intent completed");

    // No active (open/running) intent remains.
    assert!(!store.has_active_intent(exp_id).await.unwrap());

    // Cleanup.
    sqlx::query("DELETE FROM exploration_anchors WHERE node_id IN (SELECT id FROM exploration_nodes WHERE exploration_id=$1)")
        .bind(exp_id).execute(store.pool()).await.ok();
    sqlx::query("DELETE FROM exploration_nodes WHERE exploration_id=$1")
        .bind(exp_id)
        .execute(store.pool())
        .await
        .ok();
    sqlx::query("DELETE FROM explorations WHERE id=$1")
        .bind(exp_id)
        .execute(store.pool())
        .await
        .ok();
}

#[tokio::test]
async fn intent_claim_is_exclusive() {
    let Some(dsn) = dsn() else { return };
    let db = match open(&dsn).await {
        Ok(d) => d,
        Err(e) => {
            eprintln!("skipping: {e}");
            return;
        }
    };
    sqlx::query("DELETE FROM exploration_nodes WHERE origin='itest2'")
        .execute(db.pool())
        .await
        .ok();
    let exp_id: i64 = sqlx::query_scalar(
        "INSERT INTO explorations(description, goal, status) VALUES ('itest2','g','open') RETURNING id",
    ).fetch_one(db.pool()).await.unwrap();
    let node_id: i64 = sqlx::query_scalar(
        "INSERT INTO exploration_nodes(exploration_id, kind, payload, priority, state, origin) \
         VALUES ($1,'intent','{}'::jsonb,5,'open','itest2') RETURNING id",
    )
    .bind(exp_id)
    .fetch_one(db.pool())
    .await
    .unwrap();

    let store = ExplorationStore::new(db);
    // First claim wins.
    let t1 = store
        .claim_intent_token(exp_id, node_id, "w1")
        .await
        .unwrap();
    assert!(t1.is_some());
    // Second claim fails (already running).
    let t2 = store
        .claim_intent_token(exp_id, node_id, "w2")
        .await
        .unwrap();
    assert!(t2.is_none());

    sqlx::query("DELETE FROM exploration_nodes WHERE id=$1")
        .bind(node_id)
        .execute(store.pool())
        .await
        .ok();
    sqlx::query("DELETE FROM explorations WHERE id=$1")
        .bind(exp_id)
        .execute(store.pool())
        .await
        .ok();
}
