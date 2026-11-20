//! Autonomous engine loop — port of ARTEX `server/engine.go` (the scheduling
//! orchestration, not the LLM reasoning).
//!
//! The engine implements the closed loop:
//! 1. Wake on a graph change (debounced).
//! 2. If there is an open goal and no active intent, call the [`Planner`] to
//!    dispatch new intents into the frontier.
//! 3. Spawn workers that claim one intent each and execute it via [`Worker`],
//!    writing results back to the graph.
//! 4. End when the goal is met or the frontier is drained.
//!
//! The LLM-driven reasoning (norma SDK: planner/worker/mainagent) is behind the
//! [`Planner`] and [`Worker`] traits — this crate only orchestrates. That keeps
//! the scheduling loop testable and DB-driven.

use async_trait::async_trait;
use exploration::{ExplorationNode, ExplorationStore};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("exploration: {0}")]
    Exploration(#[from] exploration::ExplorationError),
    #[error("sql: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("planner: {0}")]
    Planner(String),
    #[error("worker: {0}")]
    Worker(String),
}

/// A single intent the planner wants dispatched into the frontier.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannedIntent {
    /// Human-readable direction (the intent payload `description`/`goal`).
    pub description: String,
    /// Priority (higher = dispatched first).
    pub priority: i32,
    /// Asset ids this intent is anchored to.
    pub asset_ids: Vec<i64>,
    /// Origin tag (e.g. "planner").
    pub origin: String,
}

/// The result a worker writes back after executing an intent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerResult {
    /// Terminal state for the intent: "done" | "failed" | "cancelled".
    pub state: String,
    /// Summary of what the worker concluded.
    pub summary: String,
    /// Facts discovered (recorded as fact nodes).
    pub facts: Vec<String>,
    /// Whether the goal is now met (drives loop termination).
    pub goal_met: bool,
}

/// The planner: reads the situation and returns intents to dispatch.
#[async_trait]
pub trait Planner: Send + Sync {
    /// Produce the next batch of intents for `exploration_id`. An empty vec
    /// means "nothing to dispatch right now".
    async fn plan(&self, exploration_id: i64) -> Result<Vec<PlannedIntent>, EngineError>;
}

/// The worker: executes one claimed intent.
#[async_trait]
pub trait Worker: Send + Sync {
    /// Execute `intent` (a claimed intent node). Returns the outcome to write back.
    async fn execute(&self, intent: &ExplorationNode) -> Result<WorkerResult, EngineError>;
}

/// Engine configuration.
#[derive(Debug, Clone)]
pub struct EngineConfig {
    /// Max workers claiming intents concurrently per task.
    pub max_parallel: usize,
    /// Debounce window before a planner round fires after a wake.
    pub wake_debounce: Duration,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            max_parallel: 4,
            wake_debounce: Duration::from_millis(500),
        }
    }
}

/// The autonomous engine. Holds the planner/worker implementations and drives
/// the per-task loop.
pub struct Engine {
    store: Arc<ExplorationStore>,
    planner: Arc<dyn Planner>,
    worker: Arc<dyn Worker>,
    config: EngineConfig,
}

impl Engine {
    pub fn new(
        store: Arc<ExplorationStore>,
        planner: Arc<dyn Planner>,
        worker: Arc<dyn Worker>,
        config: EngineConfig,
    ) -> Self {
        Self {
            store,
            planner,
            worker,
            config,
        }
    }

    /// Run one scheduling iteration for a task:
    ///
    /// - If there is an open goal and no active intent, dispatch planner intents.
    /// - Claim up to `max_parallel` frontier intents and execute them.
    ///
    /// Returns whether the goal is now met (loop should stop).
    pub async fn step(&self, exploration_id: i64) -> Result<bool, EngineError> {
        // 1. Dispatch: if the goal is open and no intent is in play, plan.
        if self.store.has_open_goal(exploration_id).await?
            && !self.store.has_active_intent(exploration_id).await?
        {
            let intents = self.planner.plan(exploration_id).await?;
            for intent in intents {
                self.dispatch_intent(exploration_id, &intent).await?;
            }
        }

        // 2. Claim + execute up to max_parallel frontier intents.
        let frontier = self.store.frontier(exploration_id).await?;
        let mut goal_met = false;
        for intent in frontier.into_iter().take(self.config.max_parallel) {
            let Some(token) = self
                .store
                .claim_intent_token(exploration_id, intent.id, "engine")
                .await?
            else {
                continue; // claimed by another worker
            };
            let _ = token; // fencing token (heartbeat/settle would present it)
            match self.worker.execute(&intent).await {
                Ok(result) => {
                    self.store
                        .set_intent_state(exploration_id, intent.id, &result.state)
                        .await?;
                    if result.goal_met {
                        goal_met = true;
                    }
                }
                Err(e) => {
                    tracing::warn!(intent = intent.id, error = %e, "worker failed");
                    self.store
                        .set_intent_state(exploration_id, intent.id, "failed")
                        .await?;
                }
            }
        }
        Ok(goal_met)
    }

    /// Dispatch one planner intent into the frontier as a new `intent` node.
    async fn dispatch_intent(
        &self,
        exploration_id: i64,
        intent: &PlannedIntent,
    ) -> Result<(), EngineError> {
        let payload = serde_json::json!({
            "description": intent.description,
            "origin": intent.origin,
        });
        let payload_text = payload.to_string();
        // Insert an intent node in the open state. Anchors are recorded as
        // exploration_anchors rows (best-effort).
        let node_id: i64 = sqlx::query_scalar(
            "INSERT INTO exploration_nodes(exploration_id, kind, payload, priority, state, origin) \
             VALUES ($1, 'intent', $2::jsonb, $3, 'open', $4) RETURNING id",
        )
        .bind(exploration_id)
        .bind(&payload_text)
        .bind(intent.priority)
        .bind(&intent.origin)
        .fetch_one(self.store.pool())
        .await?;
        for asset_id in &intent.asset_ids {
            let _ = sqlx::query(
                "INSERT INTO exploration_anchors(node_id, asset_id) VALUES ($1, $2) \
                 ON CONFLICT DO NOTHING",
            )
            .bind(node_id)
            .bind(asset_id)
            .execute(self.store.pool())
            .await;
        }
        Ok(())
    }

    /// Recover a task on startup: reset any running intents back to open.
    pub async fn recover(&self, exploration_id: i64) -> Result<u64, EngineError> {
        Ok(self.store.reset_running_intents(exploration_id).await?)
    }
}

/// A no-op planner/worker pair for tests and wiring stubs.
pub struct NoopPlanner;
#[async_trait]
impl Planner for NoopPlanner {
    async fn plan(&self, _exploration_id: i64) -> Result<Vec<PlannedIntent>, EngineError> {
        Ok(Vec::new())
    }
}

pub struct NoopWorker;
#[async_trait]
impl Worker for NoopWorker {
    async fn execute(&self, _intent: &ExplorationNode) -> Result<WorkerResult, EngineError> {
        Ok(WorkerResult {
            state: "done".into(),
            summary: String::new(),
            facts: Vec::new(),
            goal_met: false,
        })
    }
}

/// Helper for building a task-scoped store map (used by the server wiring).
pub type StoreMap = HashMap<i64, Arc<ExplorationStore>>;

/// Convenience: the default frontier limit used when dispatching.
pub const DEFAULT_MAX_PARALLEL: usize = 4;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_defaults() {
        let c = EngineConfig::default();
        assert_eq!(c.max_parallel, 4);
        assert_eq!(c.wake_debounce, Duration::from_millis(500));
    }

    #[test]
    fn planned_intent_serde() {
        let p = PlannedIntent {
            description: "scan".into(),
            priority: 5,
            asset_ids: vec![1, 2],
            origin: "planner".into(),
        };
        let s = serde_json::to_string(&p).unwrap();
        let back: PlannedIntent = serde_json::from_str(&s).unwrap();
        assert_eq!(back.description, "scan");
        assert_eq!(back.asset_ids, vec![1, 2]);
    }
}
