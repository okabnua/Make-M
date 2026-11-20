//! Builtin seeding — port of ARTEX `db/db.go` seedBuiltins + helpers and
//! `db/settings.go` subset.
//!
//! All seeds are idempotent and gated by `settings` flags so a user's later
//! edits are never overwritten on restart. Secret-setting encryption is out of
//! scope here (no seed writes a secret setting); secret handling lives in a
//! dedicated module later.

use sqlx::{PgConnection, Row};
use std::collections::HashMap;

const SETTING_TARGET_EXEMPT_HOSTS: &str = "target_gate_exempt_hosts";

/// Get a setting value; `None` when the key is unset.
pub async fn get_setting(
    conn: &mut PgConnection,
    key: &str,
) -> Result<Option<String>, sqlx::Error> {
    let row = sqlx::query("SELECT value FROM settings WHERE key = $1")
        .bind(key)
        .fetch_optional(&mut *conn)
        .await?;
    Ok(row.map(|r| r.get("value")))
}

/// Get a boolean setting; returns `def` when unset or unparseable.
pub async fn get_bool(conn: &mut PgConnection, key: &str, def: bool) -> bool {
    match get_setting(conn, key).await {
        Ok(Some(v)) => v == "true" || v == "1",
        _ => def,
    }
}

/// Upsert a setting value.
pub async fn set_setting(
    conn: &mut PgConnection,
    key: &str,
    value: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO settings(key, value) VALUES ($1, $2) \
         ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value, updated_at = now()",
    )
    .bind(key)
    .bind(value)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Insert a setting only when the key is absent; returns whether it was inserted.
pub async fn insert_setting_if_absent(
    conn: &mut PgConnection,
    key: &str,
    value: &str,
) -> Result<bool, sqlx::Error> {
    let res = sqlx::query(
        "INSERT INTO settings(key, value) VALUES ($1, $2) ON CONFLICT (key) DO NOTHING",
    )
    .bind(key)
    .bind(value)
    .execute(&mut *conn)
    .await?;
    Ok(res.rows_affected() > 0)
}

struct BuiltinAgent {
    key: &'static str,
    name: &'static str,
    role: &'static str,
    desc: &'static str,
    vars: &'static [(&'static str, &'static str, &'static str, &'static str)],
    interactive_shell: bool,
    run_seconds: Option<i32>,
}

const BUILTIN_AGENTS: &[BuiltinAgent] = &[
    BuiltinAgent {
        key: "goals",
        name: "Goal Decomposer",
        role: "goals",
        desc: "Breaks a penetration testing task's goal into several independent, verifiable sub-goals.",
        vars: &[(
            "EngagementDescription",
            "Task description (test target/background)",
            "Test the example.com site",
            "exploration",
        )],
        interactive_shell: false,
        run_seconds: None,
    },
    BuiltinAgent {
        key: "planner",
        name: "Planner",
        role: "planner",
        desc: "Reads the posture, judges goals, and only adds exploration intents when there is a genuinely uncovered new direction (one planning loop per task).",
        vars: &[
            ("Goal", "Overall task goal", "Gain admin access to example.com", "exploration"),
            ("AssetSummary", "Asset count/type distribution summary (optional)", "domain:3 ip:5 site:2", "distilled"),
        ],
        interactive_shell: false,
        run_seconds: None,
    },
    BuiltinAgent {
        key: "mainagent",
        name: "Main",
        role: "main",
        desc: "Human-machine interface: observes progress and turns human intent into hints or high-priority intents.",
        vars: &[
            ("Goal", "Current task goal", "Gain admin access to example.com", "exploration"),
            ("AssetSummary", "Initial posture summary (optional)", "domain:3 ip:5", "distilled"),
            ("FindingsSummary", "Confirmed vulnerability summary (optional)", "high:1 medium:2", "distilled"),
        ],
        interactive_shell: false,
        run_seconds: None,
    },
    BuiltinAgent {
        key: "worker",
        name: "Worker",
        role: "worker",
        desc: "Picks up one intent and executes it, writes discovered facts/vulnerabilities back to the knowledge graph, then stops.",
        vars: &[
            ("ProxyAddr", "Recorded proxy address (drives the if/else dual wording)", "127.0.0.1:8080", "runtime"),
            ("WorkerName", "Worker self-identifier (optional)", "worker-1", "runtime"),
        ],
        interactive_shell: false,
        run_seconds: None,
    },
    BuiltinAgent {
        key: "auto",
        name: "Auto",
        role: "assistant",
        desc: "Platform operations assistant: manages tasks (create/view/pause/hint) and assets with tools, and can create/modify skills, custom tools, and MCP.",
        vars: &[],
        interactive_shell: false,
        run_seconds: None,
    },
    BuiltinAgent {
        key: "pentest",
        name: "Penetration Testing",
        role: "assistant",
        desc: "Solo pentest agent: single-handedly walks the full chain from recon → attack surface → deep exploitation → verification → wrap-up, with its own planning, execution, and adversarial verification.",
        vars: &[],
        interactive_shell: true,
        run_seconds: Some(0),
    },
];

const BUILTIN_SKILL_VISIBILITY: &[(&str, &[&str])] =
    &[("api-recon", &["auto", "pentest", "worker"])];

struct RuleV1 {
    name: &'static str,
    target: &'static str,
    typ: &'static str,
    pattern: &'static str,
    action: &'static str,
    message: &'static str,
    priority: i32,
}

const RULES_V1: &[RuleV1] = &[
    RuleV1 { name: "[built-in] Recursive force delete rm -rf", target: "tool_input", typ: "regex", pattern: r#"(?i)\brm\b.{0,80}(?:-[a-z]*r[a-z]*f[a-z]*|-[a-z]*f[a-z]*r[a-z]*|--recursive|--no-preserve-root)"#, action: "deny", message: "Recursive force delete is forbidden (rm -rf / rm --recursive); it can permanently damage the system or the target machine environment", priority: 100 },
    RuleV1 { name: "[built-in] Delete critical system directories", target: "tool_input", typ: "regex", pattern: r#"\brm\b[^"'\n]{0,60}["'\s](/|/etc|/bin|/usr|/boot|/var|/lib|/sys|/proc|/dev|/sbin|/root)"#, action: "deny", message: "Deleting critical system paths is forbidden", priority: 100 },
    RuleV1 { name: "[built-in] Disk formatting mkfs", target: "tool_input", typ: "regex", pattern: r#"\bmkfs\b"#, action: "deny", message: "Formatting disks is forbidden (mkfs)", priority: 100 },
    RuleV1 { name: "[built-in] Overwrite disk device dd", target: "tool_input", typ: "regex", pattern: r#"\bdd\b[^|\n]{0,100}\bof=\s*/dev/[a-zA-Z]"#, action: "deny", message: "Using dd to overwrite disk devices is forbidden", priority: 100 },
    RuleV1 { name: "[built-in] Fork bomb", target: "tool_input", typ: "regex", pattern: r#":\(\)\s*\{[^}]*:\|:"#, action: "deny", message: "Executing a fork bomb is forbidden", priority: 100 },
    RuleV1 { name: "[built-in] Shutdown / reboot", target: "tool_input", typ: "regex", pattern: r#"\b(?:shutdown|reboot|halt|poweroff|init\s+[06])\b"#, action: "deny", message: "Executing shutdown or reboot commands is forbidden", priority: 100 },
    RuleV1 { name: "[built-in] Kill all processes", target: "tool_input", typ: "regex", pattern: r#"\bkill\s+-9\s+-1\b|\bkillall\s+-9\b"#, action: "deny", message: "kill -9 -1 or killall -9 is forbidden (kills all processes)", priority: 100 },
    RuleV1 { name: "[built-in] Disk wipe shred / wipe", target: "tool_input", typ: "regex", pattern: r#"\b(?:shred|wipe)\b[^|\n]{0,80}/dev/[a-zA-Z]"#, action: "deny", message: "Running shred/wipe erasure against disk devices is forbidden", priority: 100 },
    RuleV1 { name: "[built-in] Flush firewall rules", target: "tool_input", typ: "regex", pattern: r#"\biptables\s+(?:-F|--flush)\b|\bnft\s+flush\s+ruleset\b"#, action: "deny", message: "Flushing firewall rules is forbidden (iptables -F / nft flush)", priority: 100 },
    RuleV1 { name: "[built-in] SQL DROP DATABASE / TABLE / SCHEMA", target: "tool_input", typ: "regex", pattern: r#"(?i)\bDROP\s+(?:DATABASE|TABLE|SCHEMA|INDEX|VIEW|TABLESPACE|USER|ROLE)\b"#, action: "deny", message: "DROP operations are forbidden; they can irreversibly destroy database objects", priority: 90 },
    RuleV1 { name: "[built-in] SQL TRUNCATE", target: "tool_input", typ: "regex", pattern: r#"(?i)\bTRUNCATE\s+(?:TABLE\s+)?\w"#, action: "deny", message: "TRUNCATE is forbidden; it can wipe all data in a table", priority: 90 },
    RuleV1 { name: "[built-in] MongoDB drop / dropDatabase", target: "tool_input", typ: "regex", pattern: r#"(?i)\.(?:dropDatabase|dropCollection|drop)\s*\("#,
        action: "deny", message: "MongoDB drop operations are forbidden", priority: 90 },
    RuleV1 { name: "[built-in] Redis FLUSHALL / FLUSHDB", target: "tool_input", typ: "regex", pattern: r#"(?i)\b(?:FLUSHALL|FLUSHDB)\b"#, action: "deny", message: "Redis FLUSHALL / FLUSHDB is forbidden; it can wipe all cached data", priority: 90 },
    RuleV1 { name: "[built-in] curl / wget sending DELETE requests", target: "tool_input", typ: "regex", pattern: r#"(?i)\bcurl\b[^|\n&;"]{0,300}(?:-X\s*DELETE|--request\s+DELETE|-XDELETE)|\bwget\b[^|\n&;"]{0,300}--method[=\s]+DELETE"#, action: "deny", message: "Sending HTTP DELETE requests via curl/wget is forbidden; it may delete data on the target system", priority: 80 },
    RuleV1 { name: "[built-in] Python HTTP client DELETE (requests/httpx/aiohttp)", target: "tool_input", typ: "regex", pattern: r#"(?i)\b(?:requests|httpx|aiohttp|urllib\.request)\.delete\s*\(|session\.delete\s*\(|client\.delete\s*\("#, action: "deny", message: "Sending DELETE requests with Python HTTP clients is forbidden", priority: 80 },
    RuleV1 { name: "[built-in] Declaring the HTTP DELETE method in scripts (JS/generic)", target: "tool_input", typ: "regex", pattern: r#"(?i)axios\.delete\s*\(|method\s*[:=]\s*['"]DELETE['"]"#, action: "deny", message: "Declaring and sending HTTP DELETE requests in scripts is forbidden", priority: 80 },
    RuleV1 { name: "[built-in] Bulk clear / wipe endpoint paths", target: "tool_input", typ: "regex", pattern: r#"(?i)/(?:clear|wipe|flush|purge|truncate|drop|destroy|factory[-_]reset|reset[-_]all)(?:[/?#"'\s]|$)"#, action: "deny", message: "Calling bulk-clear or destroy-style endpoints is forbidden (/clear /wipe /flush /purge etc.)", priority: 80 },
];

struct RuleV2 {
    name: &'static str,
    pattern: &'static str,
    action: &'static str,
    message: &'static str,
    enabled: bool,
    priority: i32,
}

const RULES_V2: &[RuleV2] = &[
    RuleV2 { name: "[built-in] Destructive system commands", pattern: r#"(?i)\b(rm\s+-rf\s+/|mkfs|dd\s+if=|:\(\)\s*\{|shutdown|reboot|>\s*/dev/sd)"#, action: "deny", message: "Destructive command rejected (rm -rf / / mkfs / dd / fork bomb / shutdown-reboot / overwriting disk devices)", enabled: true, priority: 100 },
    RuleV2 { name: "[built-in] Data exfiltration pipe", pattern: r#"(?i)(curl|wget|nc|ncat)\b[^|]*\b(\|\s*(curl|wget|nc))"#, action: "deny", message: "Suspected data exfiltration pipe rejected (command output piped out via curl/wget/nc)", enabled: false, priority: 80 },
];

const DELETE_ENDPOINT_PATH_PATTERN: &str =
    r#"(?i)/(?:(?:delete|remove|unlink|erase|destroy)[-\w]*|del)(?:[/?#"'\s]|$)"#;

const ASSET_RULES: &[(&str, &str, &str)] = &[
    (
        "fuzzy_domain",
        ".gov",
        "[built-in] Government websites (.gov)",
    ),
    (
        "fuzzy_domain",
        ".gov.cn",
        "[built-in] Government websites (.gov.cn)",
    ),
    (
        "fuzzy_domain",
        ".edu",
        "[built-in] Education websites (.edu)",
    ),
    (
        "fuzzy_domain",
        ".edu.cn",
        "[built-in] Education websites (.edu.cn)",
    ),
];

/// Seed everything `db.seedBuiltins` does, in order.
pub async fn seed_builtins(conn: &mut PgConnection) -> Result<(), sqlx::Error> {
    // Builtin agents + their variable catalog.
    for a in BUILTIN_AGENTS {
        let agent_id: i64 = sqlx::query(
            "INSERT INTO agents(key, name, description, role, builtin, enabled, interactive_shell, run_seconds) \
             VALUES ($1, $2, NULLIF($3,''), $4, true, true, $5, COALESCE($6, 1200)) \
             ON CONFLICT (key) DO UPDATE SET name = EXCLUDED.name, description = EXCLUDED.description \
             RETURNING id",
        )
        .bind(a.key)
        .bind(a.name)
        .bind(a.desc)
        .bind(a.role)
        .bind(a.interactive_shell)
        .bind(a.run_seconds)
        .fetch_one(&mut *conn)
        .await?
        .get("id");
        for v in a.vars {
            sqlx::query(
                "INSERT INTO agent_prompt_vars(agent_id, var_name, description, example, source) \
                 VALUES ($1, $2, $3, $4, $5) \
                 ON CONFLICT (agent_id, var_name) DO UPDATE \
                 SET description = EXCLUDED.description, example = EXCLUDED.example, source = EXCLUDED.source",
            )
            .bind(agent_id)
            .bind(v.0)
            .bind(v.1)
            .bind(v.2)
            .bind(v.3)
            .execute(&mut *conn)
            .await?;
        }
    }
    // Drop renamed vars.
    sqlx::query("DELETE FROM agent_prompt_vars WHERE var_name IN ('EngagementTitle', 'CoverageGaps', 'Now')")
        .execute(&mut *conn)
        .await?;

    // Default-on interactive_shell for runtime agents, once.
    if get_setting(conn, "interactive_shell_default_v1").await? != Some("true".into()) {
        sqlx::query("UPDATE agents SET interactive_shell=true WHERE key IN ('planner','worker','mainagent','auto')")
            .execute(&mut *conn)
            .await?;
        set_setting(conn, "interactive_shell_default_v1", "true").await?;
    }

    // Built-in browser MCP (disabled by default).
    let args: serde_json::Value =
        serde_json::from_str(r#"["@playwright/mcp","--headless"]"#).unwrap();
    let env: serde_json::Value = serde_json::from_str("{}").unwrap();
    sqlx::query(
        "INSERT INTO mcp_servers(name, transport, command, args, env, enabled) \
         VALUES ('browser', 'stdio', 'npx', $1, $2, false) ON CONFLICT (name) DO NOTHING",
    )
    .bind(&args)
    .bind(&env)
    .execute(&mut *conn)
    .await?;

    // Skill visibility.
    for (skill, agents) in BUILTIN_SKILL_VISIBILITY {
        for key in *agents {
            sqlx::query(
                "INSERT INTO agent_skill_visibility(agent_id, skill_name, enabled) \
                 SELECT id, $2, true FROM agents WHERE key=$1 \
                 ON CONFLICT (agent_id, skill_name) DO NOTHING",
            )
            .bind(key)
            .bind(skill)
            .execute(&mut *conn)
            .await?;
        }
    }

    seed_intercept_v1(conn).await?;
    seed_intercept_v2(conn).await?;
    seed_intercept_v3(conn).await?;
    seed_asset_intercept(conn).await?;
    seed_target_gate_exemptions(conn).await?;
    Ok(())
}

async fn seed_intercept_v1(conn: &mut PgConnection) -> Result<(), sqlx::Error> {
    if get_setting(conn, "intercept_default_rules_v1").await? == Some("done".into()) {
        return Ok(());
    }
    for r in RULES_V1 {
        sqlx::query(
            "INSERT INTO intercept_rules(name, enabled, priority, match_target, match_type, pattern, action, message, timeout_enabled, timeout_seconds, timeout_action) \
             VALUES ($1, true, $2, $3, $4, $5, $6, $7, false, 60, 'deny') ON CONFLICT DO NOTHING",
        )
        .bind(r.name)
        .bind(r.priority)
        .bind(r.target)
        .bind(r.typ)
        .bind(r.pattern)
        .bind(r.action)
        .bind(r.message)
        .execute(&mut *conn)
        .await?;
    }
    set_setting(conn, "intercept_default_rules_v1", "done").await
}

async fn seed_intercept_v2(conn: &mut PgConnection) -> Result<(), sqlx::Error> {
    if get_setting(conn, "intercept_default_rules_v2").await? == Some("done".into()) {
        return Ok(());
    }
    for r in RULES_V2 {
        sqlx::query(
            "INSERT INTO intercept_rules(name, enabled, priority, match_target, match_type, pattern, action, message, timeout_enabled, timeout_seconds, timeout_action) \
             VALUES ($1, $2, $3, 'tool_input', 'regex', $4, $5, $6, false, 60, 'deny') ON CONFLICT DO NOTHING",
        )
        .bind(r.name)
        .bind(r.enabled)
        .bind(r.priority)
        .bind(r.pattern)
        .bind(r.action)
        .bind(r.message)
        .execute(&mut *conn)
        .await?;
    }
    set_setting(conn, "intercept_default_rules_v2", "done").await
}

async fn seed_intercept_v3(conn: &mut PgConnection) -> Result<(), sqlx::Error> {
    if get_setting(conn, "intercept_default_rules_v3").await? == Some("done".into()) {
        return Ok(());
    }
    let name = "[built-in] Delete-style endpoint paths";
    let message = "Calling delete-style endpoints is forbidden (/delete /remove /unlink /erase etc.), regardless of the HTTP method — most apps' delete endpoints can be triggered with GET/POST and will still really delete target data";
    sqlx::query(
        "INSERT INTO intercept_rules(name, enabled, priority, match_target, match_type, pattern, action, message, timeout_enabled, timeout_seconds, timeout_action) \
         SELECT $1, true, 80, 'tool_input', 'regex', $2, 'deny', $3, false, 60, 'deny' \
         WHERE NOT EXISTS (SELECT 1 FROM intercept_rules WHERE name = $1)",
    )
    .bind(name)
    .bind(DELETE_ENDPOINT_PATH_PATTERN)
    .bind(message)
    .execute(&mut *conn)
    .await?;
    set_setting(conn, "intercept_default_rules_v3", "done").await
}

async fn seed_asset_intercept(conn: &mut PgConnection) -> Result<(), sqlx::Error> {
    if get_setting(conn, "asset_intercept_default_rules_v1").await? == Some("done".into()) {
        return Ok(());
    }
    for (kind, pattern, note) in ASSET_RULES {
        sqlx::query(
            "INSERT INTO asset_intercept_rules(enabled, kind, pattern, note, builtin) \
             VALUES (true, $1, $2, $3, true) ON CONFLICT DO NOTHING",
        )
        .bind(kind)
        .bind(pattern)
        .bind(note)
        .execute(&mut *conn)
        .await?;
    }
    set_setting(conn, "asset_intercept_default_rules_v1", "done").await
}

async fn seed_target_gate_exemptions(conn: &mut PgConnection) -> Result<(), sqlx::Error> {
    if get_setting(conn, "target_gate_exempt_seed_v1").await? == Some("done".into()) {
        return Ok(());
    }
    // Same JSON shape as db/target_pins.go TargetExemption marshaled.
    let raw = serde_json::json!([
        {"host": "pypi.org", "port": 443, "note": "[built-in] Python package index"},
        {"host": "files.pythonhosted.org", "port": 443, "note": "[built-in] Python package files"},
        {"host": "registry.npmjs.org", "port": 443, "note": "[built-in] npm registry"},
        {"host": "deb.debian.org", "port": 80, "note": "[built-in] Debian packages"},
        {"host": "deb.debian.org", "port": 443, "note": "[built-in] Debian packages"},
    ])
    .to_string();
    insert_setting_if_absent(conn, SETTING_TARGET_EXEMPT_HOSTS, &raw).await?;
    set_setting(conn, "target_gate_exempt_seed_v1", "done").await
}

/// Re-export for tests that need the map type.
#[allow(dead_code)]
fn _assert_map_type() -> HashMap<String, String> {
    HashMap::new()
}
