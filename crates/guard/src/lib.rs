//! Tool-call safety boundary.
//!
//! Behavior-preserving port of ARTEX `guard/guard.go`. The guard wraps the
//! PreToolUse / PostToolUse hooks: it extracts the shell-command surface for the
//! audit ring, records the decision, and delegates the safety evaluation to an
//! [`Interceptor`]. The concrete interception logic (rules, judge, target gate,
//! database) lives in the `interception` crate, which implements the traits
//! defined here. This crate deliberately does not depend on it.

use async_trait::async_trait;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

/// Maximum number of audit entries retained, matching the Go guard's ring.
pub const AUDIT_CAPACITY: usize = 2000;

/// A permission decision, matching the shape of the Go hook result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HookDecision {
    /// The call may proceed.
    Allow,
    /// The call is blocked; `reason` is shown to the model.
    Deny { reason: String },
    /// The call needs human approval; `timeout` bounds the wait, if any.
    Ask {
        reason: String,
        timeout: Option<Duration>,
    },
}

/// A target-gate decision for one asset target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TargetDecision {
    /// The target is covered by a pin or otherwise reachable.
    Allow,
    /// The target is excluded; `reason` is shown to the model.
    Deny { reason: String },
    /// The target needs operator approval; `timeout` bounds the wait, if any.
    Ask {
        reason: String,
        timeout: Option<Duration>,
    },
}

/// The audit action recorded for one gated tool call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuditAction {
    /// The call was allowed to proceed.
    #[serde(rename = "allow")]
    Allow,
    /// The call was blocked.
    #[serde(rename = "block")]
    Block,
}

impl AuditAction {
    /// The wire form used by the Go audit entries.
    pub fn as_str(self) -> &'static str {
        match self {
            AuditAction::Allow => "allow",
            AuditAction::Block => "block",
        }
    }
}

/// One entry of the guard's audit ring.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditEntry {
    /// Unix timestamp in seconds.
    pub ts: i64,
    /// The tool that was gated.
    pub tool: String,
    /// The recorded action.
    pub action: AuditAction,
    /// Why the call was blocked (empty for an allow).
    pub reason: String,
    /// The extracted shell-command surface (empty for non-shell tools).
    pub command: String,
}

/// Errors the guard and its collaborators can produce.
#[derive(Debug, Error)]
pub enum GuardError {
    /// The interceptor failed while evaluating a call.
    #[error("interceptor error: {0}")]
    Interceptor(String),
    /// The target gate failed while checking or requesting a target.
    #[error("target gate error: {0}")]
    TargetGate(String),
    /// A tool input could not be interpreted.
    #[error("invalid tool input: {0}")]
    InvalidInput(String),
}

/// The approval gate for asset targets, as the guard uses it.
#[async_trait]
pub trait TargetGate: Send + Sync {
    /// Check whether `host:port` is covered by a pin or otherwise reachable.
    async fn check_targets(&self, host: &str, port: u16) -> Result<TargetDecision, GuardError>;

    /// Ask an operator to approve `host:port`, which no pin covers. `ask` is the
    /// pending rule/judge ask (if any) that this request may also answer.
    async fn request_targets(
        &self,
        host: &str,
        port: u16,
        ask: Option<&HookDecision>,
    ) -> Result<TargetDecision, GuardError>;
}

/// The user-configured interception logic the guard delegates to.
///
/// An interceptor is also the session's target gate, so the guard can use one
/// value for both roles, as the Go guard wires the interceptor in as the gate.
#[async_trait]
pub trait Interceptor: TargetGate {
    /// Evaluate a tool call before it executes.
    async fn check_pre_tool_use(
        &self,
        tool_name: &str,
        tool_input: &Value,
    ) -> Result<HookDecision, GuardError>;

    /// Evaluate a tool call after it executed.
    async fn check_post_tool_use(
        &self,
        tool_name: &str,
        tool_input: &Value,
        tool_result: &Value,
        is_error: bool,
    ) -> Result<HookDecision, GuardError>;
}

/// Enforces the side-effect policy by wrapping the agent-core hooks.
pub struct Guard {
    interceptor: Option<Arc<dyn Interceptor>>,
    audit: Mutex<VecDeque<AuditEntry>>,
    target_only: bool,
}

impl Guard {
    /// Creates a guard without user-configured intercept rules.
    pub fn new() -> Self {
        Self {
            interceptor: None,
            audit: Mutex::new(VecDeque::with_capacity(AUDIT_CAPACITY)),
            target_only: false,
        }
    }

    /// Creates a guard with user-configured intercept rules.
    pub fn new_with_interceptor(interceptor: Arc<dyn Interceptor>) -> Self {
        Self {
            interceptor: Some(interceptor),
            audit: Mutex::new(VecDeque::with_capacity(AUDIT_CAPACITY)),
            target_only: false,
        }
    }

    /// Creates the guard for the planner and the main agent: built-in deny rules
    /// and the target gate only. Passing calls are not written to the audit ring.
    pub fn new_target_only(interceptor: Arc<dyn Interceptor>) -> Self {
        Self {
            interceptor: Some(interceptor),
            audit: Mutex::new(VecDeque::with_capacity(AUDIT_CAPACITY)),
            target_only: true,
        }
    }

    /// Whether this is a target-only guard (planner / main agent).
    pub fn is_target_only(&self) -> bool {
        self.target_only
    }

    /// Whether an interceptor is wired in.
    pub fn has_interceptor(&self) -> bool {
        self.interceptor.is_some()
    }

    /// Handles a PreToolUse hook: records the shell-command surface for the
    /// audit log, then delegates the evaluation to the interceptor.
    pub async fn check_pre_tool_use(
        &self,
        tool_name: &str,
        tool_input: &Value,
    ) -> Result<HookDecision, GuardError> {
        let command = command_surface(tool_name, tool_input);
        if !self.target_only {
            self.record(tool_name, AuditAction::Allow, "", &command);
        }
        let decision = match &self.interceptor {
            Some(interceptor) => {
                interceptor
                    .check_pre_tool_use(tool_name, tool_input)
                    .await?
            }
            None => HookDecision::Allow,
        };
        if let HookDecision::Deny { reason } = &decision {
            self.record(tool_name, AuditAction::Block, reason, "");
        }
        Ok(decision)
    }

    /// Handles a PostToolUse hook by delegating to the interceptor.
    pub async fn check_post_tool_use(
        &self,
        tool_name: &str,
        tool_input: &Value,
        tool_result: &Value,
        is_error: bool,
    ) -> Result<HookDecision, GuardError> {
        match &self.interceptor {
            Some(interceptor) => {
                interceptor
                    .check_post_tool_use(tool_name, tool_input, tool_result, is_error)
                    .await
            }
            None => Ok(HookDecision::Allow),
        }
    }

    /// Returns a snapshot of recent gated calls, most recent last.
    pub fn audit(&self) -> Vec<AuditEntry> {
        let audit = self.lock_audit();
        audit.iter().cloned().collect()
    }

    /// Returns the number of entries currently in the audit ring.
    pub fn audit_len(&self) -> usize {
        self.lock_audit().len()
    }

    fn lock_audit(&self) -> std::sync::MutexGuard<'_, VecDeque<AuditEntry>> {
        self.audit.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn record(&self, tool: &str, action: AuditAction, reason: &str, command: &str) {
        let mut audit = self.lock_audit();
        audit.push_back(AuditEntry {
            ts: now_unix(),
            tool: tool.to_owned(),
            action,
            reason: reason.to_owned(),
            command: command.to_owned(),
        });
        while audit.len() > AUDIT_CAPACITY {
            audit.pop_front();
        }
    }
}

impl Default for Guard {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl TargetGate for Guard {
    async fn check_targets(&self, host: &str, port: u16) -> Result<TargetDecision, GuardError> {
        match &self.interceptor {
            Some(interceptor) => interceptor.check_targets(host, port).await,
            None => Ok(TargetDecision::Allow),
        }
    }

    async fn request_targets(
        &self,
        host: &str,
        port: u16,
        ask: Option<&HookDecision>,
    ) -> Result<TargetDecision, GuardError> {
        match &self.interceptor {
            Some(interceptor) => interceptor.request_targets(host, port, ask).await,
            None => Ok(TargetDecision::Allow),
        }
    }
}

/// Extracts the shell-command surface of a tool call for the audit log:
/// `Bash` and `shell_open` carry `command`, `shell_send` carries `text`, and
/// every other tool carries nothing.
pub fn command_surface(tool_name: &str, tool_input: &Value) -> String {
    let key = match tool_name {
        "Bash" | "shell_open" => "command",
        "shell_send" => "text",
        _ => return String::new(),
    };
    tool_input
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned()
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
