use std::sync::Arc;
use std::time::Duration;

use guard::{
    command_surface, AuditAction, Guard, GuardError, HookDecision, Interceptor, TargetDecision,
    TargetGate, AUDIT_CAPACITY,
};
use serde_json::{json, Value};

#[derive(Clone)]
struct MockInterceptor {
    pre: HookDecision,
    post: HookDecision,
    target: TargetDecision,
}

impl MockInterceptor {
    fn new(pre: HookDecision, post: HookDecision) -> Self {
        Self {
            pre,
            post,
            target: TargetDecision::Allow,
        }
    }

    fn with_target(mut self, target: TargetDecision) -> Self {
        self.target = target;
        self
    }
}

#[async_trait::async_trait]
impl TargetGate for MockInterceptor {
    async fn check_targets(&self, _host: &str, _port: u16) -> Result<TargetDecision, GuardError> {
        Ok(self.target.clone())
    }

    async fn request_targets(
        &self,
        _host: &str,
        _port: u16,
        _ask: Option<&HookDecision>,
    ) -> Result<TargetDecision, GuardError> {
        Ok(self.target.clone())
    }
}

#[async_trait::async_trait]
impl Interceptor for MockInterceptor {
    async fn check_pre_tool_use(
        &self,
        _tool_name: &str,
        _tool_input: &Value,
    ) -> Result<HookDecision, GuardError> {
        Ok(self.pre.clone())
    }

    async fn check_post_tool_use(
        &self,
        _tool_name: &str,
        _tool_input: &Value,
        _tool_result: &Value,
        _is_error: bool,
    ) -> Result<HookDecision, GuardError> {
        Ok(self.post.clone())
    }
}

#[test]
fn command_surface_extracts_bash_and_shell_tools() {
    let cases = [
        ("Bash", r#"{"command":"ls -la"}"#, "ls -la"),
        (
            "shell_open",
            r#"{"command":"cat /etc/hosts"}"#,
            "cat /etc/hosts",
        ),
        (
            "shell_send",
            r#"{"text":"curl http://example/"}"#,
            "curl http://example/",
        ),
        ("WebFetch", r#"{"command":"ignored"}"#, ""),
        ("Bash", r#"{"other":"x"}"#, ""),
        ("Bash", r#"{"command":123}"#, ""),
        ("shell_send", r#"{"text":null}"#, ""),
    ];
    for (tool, raw, want) in cases {
        let input: Value = serde_json::from_str(raw).expect("valid json");
        assert_eq!(command_surface(tool, &input), want, "tool={tool}");
    }
}

#[tokio::test]
async fn audit_ring_is_bounded_and_evicts_oldest() {
    let interceptor = Arc::new(MockInterceptor::new(
        HookDecision::Allow,
        HookDecision::Allow,
    ));
    let guard = Guard::new_with_interceptor(interceptor);

    for i in 0..(AUDIT_CAPACITY + 5) {
        let input = json!({"command": format!("cmd-{i}")});
        guard
            .check_pre_tool_use("Bash", &input)
            .await
            .expect("pre hook");
    }

    assert_eq!(guard.audit_len(), AUDIT_CAPACITY);
    let audit = guard.audit();
    assert_eq!(audit.len(), AUDIT_CAPACITY);
    assert_eq!(audit[0].command, "cmd-5");
    assert_eq!(
        audit[AUDIT_CAPACITY - 1].command,
        format!("cmd-{}", AUDIT_CAPACITY + 4)
    );
    assert!(audit[0].ts > 0);
}

#[tokio::test]
async fn deny_passes_through_and_records_allow_then_block() {
    let interceptor = Arc::new(MockInterceptor::new(
        HookDecision::Deny {
            reason: "nope".to_owned(),
        },
        HookDecision::Allow,
    ));
    let guard = Guard::new_with_interceptor(interceptor);

    let decision = guard
        .check_pre_tool_use("Bash", &json!({"command": "rm -rf /"}))
        .await
        .expect("pre hook");

    assert_eq!(
        decision,
        HookDecision::Deny {
            reason: "nope".to_owned()
        }
    );
    let audit = guard.audit();
    assert_eq!(audit.len(), 2);
    assert_eq!(audit[0].action, AuditAction::Allow);
    assert_eq!(audit[0].command, "rm -rf /");
    assert_eq!(audit[1].action, AuditAction::Block);
    assert_eq!(audit[1].reason, "nope");
}

#[tokio::test]
async fn ask_passes_through_with_single_allow_entry() {
    let interceptor = Arc::new(MockInterceptor::new(
        HookDecision::Ask {
            reason: "confirm".to_owned(),
            timeout: Some(Duration::from_secs(30)),
        },
        HookDecision::Allow,
    ));
    let guard = Guard::new_with_interceptor(interceptor);

    let decision = guard
        .check_pre_tool_use("Bash", &json!({"command": "ls"}))
        .await
        .expect("pre hook");

    assert_eq!(
        decision,
        HookDecision::Ask {
            reason: "confirm".to_owned(),
            timeout: Some(Duration::from_secs(30)),
        }
    );
    let audit = guard.audit();
    assert_eq!(audit.len(), 1);
    assert_eq!(audit[0].action, AuditAction::Allow);
}

#[tokio::test]
async fn guard_without_interceptor_allows_and_audits() {
    let guard = Guard::new();

    let decision = guard
        .check_pre_tool_use("Bash", &json!({"command": "rm -rf /"}))
        .await
        .expect("pre hook");

    assert_eq!(decision, HookDecision::Allow);
    assert_eq!(guard.audit_len(), 1);
}

#[tokio::test]
async fn target_only_guard_skips_allow_but_records_deny() {
    let allowing = Arc::new(MockInterceptor::new(
        HookDecision::Allow,
        HookDecision::Allow,
    ));
    let guard = Guard::new_target_only(allowing);
    guard
        .check_pre_tool_use("Bash", &json!({"command": "ls"}))
        .await
        .expect("pre hook");
    assert_eq!(guard.audit_len(), 0);

    let denying = Arc::new(MockInterceptor::new(
        HookDecision::Deny {
            reason: "built-in".to_owned(),
        },
        HookDecision::Allow,
    ));
    let guard = Guard::new_target_only(denying);
    let decision = guard
        .check_pre_tool_use("Bash", &json!({"command": "rm -rf /"}))
        .await
        .expect("pre hook");
    assert!(matches!(decision, HookDecision::Deny { .. }));
    assert_eq!(guard.audit_len(), 1);
    assert_eq!(guard.audit()[0].action, AuditAction::Block);
}

#[tokio::test]
async fn post_tool_use_delegates_without_auditing() {
    let interceptor = Arc::new(MockInterceptor::new(
        HookDecision::Allow,
        HookDecision::Deny {
            reason: "post".to_owned(),
        },
    ));
    let guard = Guard::new_with_interceptor(interceptor);

    let decision = guard
        .check_post_tool_use("Bash", &json!({"command": "ls"}), &json!("ok"), false)
        .await
        .expect("post hook");

    assert_eq!(
        decision,
        HookDecision::Deny {
            reason: "post".to_owned()
        }
    );
    assert_eq!(guard.audit_len(), 0);
}

#[tokio::test]
async fn target_gate_delegates_to_interceptor() {
    let interceptor = Arc::new(
        MockInterceptor::new(HookDecision::Allow, HookDecision::Allow).with_target(
            TargetDecision::Deny {
                reason: "blocked".to_owned(),
            },
        ),
    );
    let guard = Guard::new_with_interceptor(interceptor);

    assert_eq!(
        guard
            .check_targets("app.example.com", 443)
            .await
            .expect("check"),
        TargetDecision::Deny {
            reason: "blocked".to_owned()
        }
    );
    assert_eq!(
        guard
            .request_targets("app.example.com", 443, None)
            .await
            .expect("request"),
        TargetDecision::Deny {
            reason: "blocked".to_owned()
        }
    );
}

#[tokio::test]
async fn target_gate_without_interceptor_allows() {
    let guard = Guard::new();
    assert_eq!(
        guard
            .check_targets("app.example.com", 443)
            .await
            .expect("check"),
        TargetDecision::Allow
    );
}

#[test]
fn audit_action_wire_form() {
    assert_eq!(AuditAction::Allow.as_str(), "allow");
    assert_eq!(AuditAction::Block.as_str(), "block");
}

#[test]
fn guard_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Guard>();
}
