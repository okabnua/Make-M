//! LLM-backed planner & worker — concrete implementations of the [`engine`]
//! traits that call a live LLM and parse structured JSON output.
//!
//! These are real implementations (they call the LLM and parse its output), but
//! a deliberately focused subset of ARTEX's `agent/planner.go` / `agent/worker.go`:
//! - The planner reads the graph overview + open goals and asks the LLM for new
//!   intents as a JSON array.
//! - The worker reads one intent + relevant facts and asks the LLM for a result
//!   (state, summary, facts, goal_met) as JSON.
//!
//! Not ported (require the full norma SDK / tool-execution runtime): tool calls,
//! compaction, transcript, harness, RAG. Those are layered on top later. The
//! trait boundary keeps this swappable.

use async_trait::async_trait;
use engine::{EngineError, PlannedIntent, Planner, Worker, WorkerResult};
use exploration::{ExplorationNode, ExplorationStore};
use llm::{ChatMessage, ChatRequest, LlmClient};
use serde::Deserialize;
use std::sync::Arc;

#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error("llm: {0}")]
    Llm(#[from] llm::LlmError),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("exploration: {0}")]
    Exploration(#[from] exploration::ExplorationError),
    #[error("empty response")]
    EmptyResponse,
}

const PLANNER_SYSTEM: &str = "You are the PLANNER of an autonomous penetration-testing agent. \
Read the situation (goal + asset overview + open facts) and output ONLY a JSON array of new \
exploration intents to dispatch. Each intent is an object: \
{\"description\": string, \"priority\": number (higher=sooner), \"asset_ids\": number[]}. \
If nothing new should be explored, output []. Output JSON only, no prose.";

const WORKER_SYSTEM: &str = "You are a WORKER of an autonomous penetration-testing agent. \
Execute the given intent and output ONLY a JSON object: \
{\"state\": \"done\"|\"failed\", \"summary\": string, \"facts\": string[], \"goal_met\": boolean}. \
Output JSON only, no prose.";

/// Build a compact graph overview for the planner prompt.
async fn graph_overview(
    store: &ExplorationStore,
    exploration_id: i64,
) -> Result<String, AgentError> {
    let graph = store.graph(exploration_id).await?;
    let goals: Vec<&ExplorationNode> = graph.nodes.iter().filter(|n| n.kind == "goal").collect();
    let facts: Vec<&ExplorationNode> = graph.nodes.iter().filter(|n| n.kind == "fact").collect();
    let mut s = String::new();
    for g in &goals {
        s.push_str(&format!("- GOAL: {}\n", g.payload));
    }
    for f in facts.iter().take(50) {
        s.push_str(&format!("- FACT: {}\n", f.payload));
    }
    if s.is_empty() {
        s.push_str("(no goals or facts yet)\n");
    }
    Ok(s)
}

/// The LLM-backed planner.
pub struct LlmPlanner {
    store: Arc<ExplorationStore>,
    client: Arc<LlmClient>,
    model: String,
}

impl LlmPlanner {
    pub fn new(store: Arc<ExplorationStore>, client: Arc<LlmClient>, model: String) -> Self {
        Self {
            store,
            client,
            model,
        }
    }
}

#[async_trait]
impl Planner for LlmPlanner {
    async fn plan(&self, exploration_id: i64) -> Result<Vec<PlannedIntent>, EngineError> {
        let overview = graph_overview(&self.store, exploration_id)
            .await
            .map_err(|e| EngineError::Planner(e.to_string()))?;
        let user = format!("Situation:\n{overview}\nOutput the JSON array of intents now.");
        let resp = self
            .client
            .chat(&ChatRequest {
                model: self.model.clone(),
                messages: vec![
                    ChatMessage {
                        role: "system".into(),
                        content: PLANNER_SYSTEM.into(),
                    },
                    ChatMessage {
                        role: "user".into(),
                        content: user,
                    },
                ],
                system: None,
                max_tokens: Some(2048),
                temperature: Some(0.0),
                thinking_type: None,
                reasoning_effort: None,
                tools: None,
            })
            .await
            .map_err(|e| EngineError::Planner(e.to_string()))?;
        if resp.content.trim().is_empty() {
            return Err(EngineError::Planner("empty planner response".into()));
        }
        let parsed: Vec<PlannedIntent> = serde_json::from_str(&extract_json(&resp.content))
            .map_err(|e| EngineError::Planner(format!("parse intents: {e}")))?;
        Ok(parsed)
    }
}

/// The LLM-backed worker.
pub struct LlmWorker {
    /// Retained for symmetry with the planner and future fact write-back.
    #[allow(dead_code)]
    store: Arc<ExplorationStore>,
    client: Arc<LlmClient>,
    model: String,
}

impl LlmWorker {
    pub fn new(store: Arc<ExplorationStore>, client: Arc<LlmClient>, model: String) -> Self {
        Self {
            store,
            client,
            model,
        }
    }
}

#[derive(Debug, Deserialize)]
struct WorkerOutput {
    #[serde(default)]
    state: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    facts: Vec<String>,
    #[serde(default)]
    goal_met: bool,
}

#[async_trait]
impl Worker for LlmWorker {
    async fn execute(&self, intent: &ExplorationNode) -> Result<WorkerResult, EngineError> {
        let user = format!(
            "Intent to execute:\n{}\n\nOutput the JSON result now.",
            intent.payload
        );
        let resp = self
            .client
            .chat(&ChatRequest {
                model: self.model.clone(),
                messages: vec![
                    ChatMessage {
                        role: "system".into(),
                        content: WORKER_SYSTEM.into(),
                    },
                    ChatMessage {
                        role: "user".into(),
                        content: user,
                    },
                ],
                system: None,
                max_tokens: Some(2048),
                temperature: Some(0.0),
                thinking_type: None,
                reasoning_effort: None,
                tools: None,
            })
            .await
            .map_err(|e| EngineError::Worker(e.to_string()))?;
        if resp.content.trim().is_empty() {
            return Err(EngineError::Worker("empty worker response".into()));
        }
        let out: WorkerOutput = serde_json::from_str(&extract_json(&resp.content))
            .map_err(|e| EngineError::Worker(format!("parse result: {e}")))?;
        let state = if out.state.is_empty() {
            "done".to_string()
        } else {
            out.state
        };
        Ok(WorkerResult {
            state,
            summary: out.summary,
            facts: out.facts,
            goal_met: out.goal_met,
        })
    }
}

/// Extract the first JSON value from an LLM response (handles ```json fences and
/// leading/trailing prose).
fn extract_json(s: &str) -> String {
    let s = s.trim();
    // Strip code fences.
    let s = if let Some(rest) = s.strip_prefix("```json") {
        rest.strip_suffix("```").unwrap_or(rest).trim()
    } else if let Some(rest) = s.strip_prefix("```") {
        rest.strip_suffix("```").unwrap_or(rest).trim()
    } else {
        s
    };
    // Find the first '[' or '{' and the matching last ']' or '}'.
    let start = s.find(['[', '{']);
    let end = s.rfind([']', '}']);
    if let (Some(a), Some(b)) = (start, end) {
        if b > a {
            return s[a..=b].to_string();
        }
    }
    s.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_json_plain_array() {
        assert_eq!(
            extract_json(r#"[{"description":"x"}]"#),
            r#"[{"description":"x"}]"#
        );
    }

    #[test]
    fn extract_json_fenced() {
        let s = "```json\n[{\"description\":\"x\"}]\n```";
        assert_eq!(extract_json(s), r#"[{"description":"x"}]"#);
    }

    #[test]
    fn extract_json_with_prose() {
        let s = "Here are the intents:\n[{\"a\":1}]\nDone.";
        assert_eq!(extract_json(s), r#"[{"a":1}]"#);
    }

    #[test]
    fn parse_planned_intent() {
        let v: Vec<PlannedIntent> = serde_json::from_str(
            r#"[{"description":"scan","priority":5,"asset_ids":[1],"origin":"planner"}]"#,
        )
        .unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].asset_ids, vec![1]);
    }

    #[test]
    fn parse_worker_output() {
        let w: WorkerOutput = serde_json::from_str(
            r#"{"state":"done","summary":"ok","facts":["a"],"goal_met":true}"#,
        )
        .unwrap();
        assert!(w.goal_met);
        assert_eq!(w.facts, vec!["a".to_string()]);
    }

    #[test]
    fn value_roundtrip() {
        let v: serde_json::Value = serde_json::from_str("{}").unwrap();
        assert!(v.is_object());
    }
}
