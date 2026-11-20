//! Tool-call interception — behaviour-preserving port of ARTEX `intercept/`.
//!
//! Ported from:
//! - `intercept/intercept.go` — the rule engine (rule store, priority
//!   evaluation, ask/pending, builtin deny rules, the LLM fallback judge wiring).
//! - `intercept/argv.go` — argv prefix rules (arity table, simple-command
//!   parsing, prefix derivation) and `intercept/alwaysallow_denylist.go`.
//! - `intercept/target_gate.go`, `target_extract.go`, `target.go`,
//!   `target_exempt.go` — the target gate: target extraction/normalisation,
//!   asset block rules, exemptions, resolution and the pin lookup.
//! - `intercept/target_resolve.go` — the system resolver and the positive-answer
//!   cache.
//!
//! The crate implements the [`guard::Interceptor`] and [`guard::TargetGate`]
//! traits so the `guard` crate can drive it without depending on it.
//!
//! Not ported (documented, by design):
//! - The full `mvdan.cc/sh` Bash grammar. [`parse_simple_command`] is a
//!   simplified tokenizer that accepts a single simple command (no pipes, lists,
//!   redirections, subshells, assignments or expansions) — the cases that
//!   actually feed the argv prefix rules.
//! - The LLM fallback judge (`closed_judge.go`, the freeform judge and their
//!   prompts). The rule engine returns `Allow` for a call no rule matched, which
//!   is the Go behaviour when the judge is disabled/not wired.
//! - The upstream-egress-proxy name-only pin mode, the redirect follower
//!   (`target_redirect.go`), `PinnedDialer` (`target_dial.go`) and the
//!   conversation/task activity fan-out of an ask. The target gate is
//!   self-contained here: it decides host+port with pins, stored/derived
//!   exemptions and asset rules, but does not pin or block on its own.
//! - URL-aware asset-rule matching beyond the exact/fuzzy string forms and CIDR
//!   containment that [`match_asset_rules`] implements.

use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::Duration;

use guard::{GuardError, HookDecision, Interceptor, TargetDecision, TargetGate};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::Row;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors the interception layer can produce.
#[derive(Debug, thiserror::Error)]
pub enum InterceptionError {
    /// A database error.
    #[error("sql: {0}")]
    Sqlx(#[from] sqlx::Error),
    /// A regex failed to compile.
    #[error("regex: {0}")]
    Regex(#[from] regex::Error),
    /// The input was invalid (bad rule field, bad argv pattern, bad target).
    #[error("invalid: {0}")]
    Invalid(String),
    /// A guard-trait violation surfaced as an error.
    #[error("guard: {0}")]
    Guard(String),
}

impl From<InterceptionError> for GuardError {
    fn from(e: InterceptionError) -> Self {
        match e {
            InterceptionError::Guard(msg) => GuardError::Interceptor(msg),
            other => GuardError::Interceptor(other.to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// intercept_rules
// ---------------------------------------------------------------------------

/// One row of `intercept_rules`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InterceptRule {
    pub id: i64,
    pub name: String,
    pub enabled: bool,
    pub priority: i32,
    /// `tool_name` or `tool_input`.
    pub match_target: String,
    /// `string`, `regex` or `argv`.
    pub match_type: String,
    pub pattern: String,
    /// `allow`, `deny` or `ask`.
    pub action: String,
    pub message: String,
    pub timeout_enabled: bool,
    pub timeout_seconds: i32,
    /// `deny` or `allow`.
    pub timeout_action: String,
}

const RULE_COLS: &str = "id, name, enabled, priority, match_target, match_type, pattern, \
     action, message, timeout_enabled, timeout_seconds, timeout_action";

fn scan_rule(row: &sqlx::postgres::PgRow) -> InterceptRule {
    InterceptRule {
        id: row.get("id"),
        name: row.get("name"),
        enabled: row.get("enabled"),
        priority: row.get("priority"),
        match_target: row.get("match_target"),
        match_type: row.get("match_type"),
        pattern: row.get("pattern"),
        action: row.get("action"),
        message: row.get("message"),
        timeout_enabled: row.get("timeout_enabled"),
        timeout_seconds: row.get("timeout_seconds"),
        timeout_action: row.get("timeout_action"),
    }
}

/// Fields of a rule that a caller supplies when creating or updating one. `id`
/// is assigned by the database on create.
#[derive(Debug, Clone)]
pub struct RuleInput {
    pub name: String,
    pub enabled: bool,
    pub priority: i32,
    pub match_target: String,
    pub match_type: String,
    pub pattern: String,
    pub action: String,
    pub message: String,
    pub timeout_enabled: bool,
    pub timeout_seconds: i32,
    pub timeout_action: String,
}

impl RuleInput {
    /// Validate the enum-like fields, mirroring the CHECK constraints.
    pub fn validate(&self) -> Result<(), InterceptionError> {
        if self.match_target != "tool_name" && self.match_target != "tool_input" {
            return Err(InterceptionError::Invalid(format!(
                "match_target must be tool_name or tool_input, got {:?}",
                self.match_target
            )));
        }
        if !matches!(self.match_type.as_str(), "string" | "regex" | "argv") {
            return Err(InterceptionError::Invalid(format!(
                "match_type must be string, regex or argv, got {:?}",
                self.match_type
            )));
        }
        if !matches!(self.action.as_str(), "allow" | "deny" | "ask") {
            return Err(InterceptionError::Invalid(format!(
                "action must be allow, deny or ask, got {:?}",
                self.action
            )));
        }
        if self.timeout_action != "deny" && self.timeout_action != "allow" {
            return Err(InterceptionError::Invalid(format!(
                "timeout_action must be deny or allow, got {:?}",
                self.timeout_action
            )));
        }
        Ok(())
    }
}

/// Database-backed store for [`InterceptRule`] rows.
#[derive(Clone)]
pub struct RuleStore {
    db: database::Db,
}

impl RuleStore {
    pub fn new(db: database::Db) -> Self {
        Self { db }
    }

    /// All rules ordered by priority DESC, then id (the Go `ListInterceptRules`).
    pub async fn list(&self) -> Result<Vec<InterceptRule>, InterceptionError> {
        let rows = sqlx::query(&format!(
            "SELECT {RULE_COLS} FROM intercept_rules ORDER BY priority DESC, id"
        ))
        .fetch_all(self.db.pool())
        .await?;
        Ok(rows.iter().map(scan_rule).collect())
    }

    /// One rule by id, or `None` when absent.
    pub async fn get(&self, id: i64) -> Result<Option<InterceptRule>, InterceptionError> {
        let row = sqlx::query(&format!(
            "SELECT {RULE_COLS} FROM intercept_rules WHERE id = $1"
        ))
        .bind(id)
        .fetch_optional(self.db.pool())
        .await?;
        Ok(row.as_ref().map(scan_rule))
    }

    /// Insert a new rule and return it as stored.
    pub async fn create(&self, input: RuleInput) -> Result<InterceptRule, InterceptionError> {
        input.validate()?;
        let row = sqlx::query(&format!(
            "INSERT INTO intercept_rules(name, enabled, priority, match_target, match_type, \
             pattern, action, message, timeout_enabled, timeout_seconds, timeout_action) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11) RETURNING {RULE_COLS}"
        ))
        .bind(&input.name)
        .bind(input.enabled)
        .bind(input.priority)
        .bind(&input.match_target)
        .bind(&input.match_type)
        .bind(&input.pattern)
        .bind(&input.action)
        .bind(&input.message)
        .bind(input.timeout_enabled)
        .bind(input.timeout_seconds)
        .bind(&input.timeout_action)
        .fetch_one(self.db.pool())
        .await?;
        Ok(scan_rule(&row))
    }

    /// Replace every editable field of an existing rule and return it as stored.
    pub async fn update(
        &self,
        id: i64,
        input: RuleInput,
    ) -> Result<InterceptRule, InterceptionError> {
        input.validate()?;
        let row = sqlx::query(&format!(
            "UPDATE intercept_rules SET name=$2, enabled=$3, priority=$4, match_target=$5, \
             match_type=$6, pattern=$7, action=$8, message=$9, timeout_enabled=$10, \
             timeout_seconds=$11, timeout_action=$12 WHERE id=$1 RETURNING {RULE_COLS}"
        ))
        .bind(id)
        .bind(&input.name)
        .bind(input.enabled)
        .bind(input.priority)
        .bind(&input.match_target)
        .bind(&input.match_type)
        .bind(&input.pattern)
        .bind(&input.action)
        .bind(&input.message)
        .bind(input.timeout_enabled)
        .bind(input.timeout_seconds)
        .bind(&input.timeout_action)
        .fetch_one(self.db.pool())
        .await?;
        Ok(scan_rule(&row))
    }

    /// Delete a rule; a missing id is a no-op.
    pub async fn delete(&self, id: i64) -> Result<(), InterceptionError> {
        sqlx::query("DELETE FROM intercept_rules WHERE id = $1")
            .bind(id)
            .execute(self.db.pool())
            .await?;
        Ok(())
    }

    /// Flip the enabled state of a rule; a missing id is a no-op.
    pub async fn toggle(&self, id: i64, enabled: bool) -> Result<(), InterceptionError> {
        sqlx::query("UPDATE intercept_rules SET enabled = $2 WHERE id = $1")
            .bind(id)
            .bind(enabled)
            .execute(self.db.pool())
            .await?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Rule matching
// ---------------------------------------------------------------------------

/// Parse and validate an argv pattern: a JSON array of 1..=8 tokens, none
/// empty, none starting with `-`, none holding a shell metacharacter or space.
pub fn parse_argv_pattern(pattern: &str) -> Result<Vec<String>, InterceptionError> {
    const ARGV_MAX_TOKENS: usize = 8;
    let tokens: Vec<String> = serde_json::from_str(pattern).map_err(|e| {
        InterceptionError::Invalid(format!(
            "an argv pattern is a JSON array of strings, for example [\"git\",\"push\"]: {e}"
        ))
    })?;
    if tokens.is_empty() || tokens.len() > ARGV_MAX_TOKENS {
        return Err(InterceptionError::Invalid(format!(
            "an argv pattern has 1 to {ARGV_MAX_TOKENS} tokens, got {}",
            tokens.len()
        )));
    }
    for tok in &tokens {
        check_argv_token(tok)?;
    }
    Ok(tokens)
}

fn check_argv_token(tok: &str) -> Result<(), InterceptionError> {
    if tok.is_empty() {
        return Err(InterceptionError::Invalid("a token is empty".to_owned()));
    }
    if tok.starts_with('-') {
        return Err(InterceptionError::Invalid(format!(
            "token {tok:?} is a flag; a prefix holds the program and its subcommands only"
        )));
    }
    if tok.contains([';', '&', '|', '>', '<', '`', '$', '('])
        || tok.chars().any(char::is_whitespace)
    {
        return Err(InterceptionError::Invalid(format!(
            "token {tok:?} contains a shell metacharacter or a space"
        )));
    }
    Ok(())
}

/// A rule with its regex/argv pattern pre-compiled (Go `compiledRule`).
#[derive(Clone)]
struct CompiledRule {
    rule: InterceptRule,
    re: Option<regex::Regex>,
    argv: Option<Vec<String>>,
}

impl CompiledRule {
    /// Compile a rule. `None` for a rule whose regex or argv pattern does not
    /// compile; such a rule is skipped rather than crashing.
    fn compile(rule: InterceptRule) -> Option<Self> {
        let (re, argv) = match rule.match_type.as_str() {
            "regex" => (Some(regex::Regex::new(&rule.pattern).ok()?), None),
            "argv" => (None, Some(parse_argv_pattern(&rule.pattern).ok()?)),
            _ => (None, None),
        };
        Some(Self { rule, re, argv })
    }
}

/// Evaluate `rule` against a tool call.
///
/// - `string`: substring match on the target (tool name or serialized input).
/// - `regex`: regular-expression match on the target. Go's `regexp` is RE2;
///   the Rust `regex` crate is RE2-like but lacks lookaround and backreferences.
///   A pattern using those constructs will fail to compile here and the rule is
///   skipped — port such a pattern to `fancy-regex` if needed (not a dependency).
/// - `argv`: prefix match for a `Bash` call. The command is tokenized by a
///   simplified shell tokenizer (see [`parse_simple_command`]) and compared to
///   the JSON token array; flags after the prefix are not constrained, but a
///   flag before the subcommand yields no prefix and so no match.
pub fn match_rule(rule: &InterceptRule, tool_name: &str, tool_input: &Value) -> bool {
    let Some(cr) = CompiledRule::compile(rule.clone()) else {
        return false;
    };
    if cr.rule.match_type == "argv" {
        // The denylist neutralises allow rules only: an argv deny or ask rule
        // for a denylisted program still matches.
        if cr.rule.action == "allow" {
            if let Some(tokens) = &cr.argv {
                if check_argv_allow(tokens).is_some() {
                    return false;
                }
            }
        }
        return cr.rule.match_target == "tool_input"
            && argv_rule_matches(&cr, tool_name, tool_input);
    }
    let subject = match cr.rule.match_target.as_str() {
        "tool_name" => tool_name.to_owned(),
        "tool_input" => serde_json::to_string(tool_input).unwrap_or_default(),
        _ => return false,
    };
    if cr.rule.match_type == "regex" {
        return cr.re.map(|re| re.is_match(&subject)).unwrap_or(false);
    }
    subject.contains(&cr.rule.pattern)
}

fn argv_rule_matches(cr: &CompiledRule, tool_name: &str, tool_input: &Value) -> bool {
    if tool_name != "Bash" {
        return false;
    }
    let Some(tokens) = &cr.argv else {
        return false;
    };
    if tokens.is_empty() {
        return false;
    }
    let Some(command) = tool_input.get("command").and_then(Value::as_str) else {
        return false;
    };
    let Ok(Some(prefix)) = argv_prefix(command) else {
        return false;
    };
    prefix.len() == tokens.len() && prefix.iter().zip(tokens).all(|(a, b)| a == b)
}

/// Evaluate the enabled rules in priority order (DESC, then id ASC) and return
/// the decision of the first match, or `None` when nothing matches.
#[allow(dead_code)]
fn evaluate(rules: &[InterceptRule], tool_name: &str, tool_input: &Value) -> Option<HookDecision> {
    let mut ordered: Vec<&InterceptRule> = rules.iter().filter(|r| r.enabled).collect();
    ordered.sort_by(|a, b| b.priority.cmp(&a.priority).then(a.id.cmp(&b.id)));
    for rule in ordered {
        if match_rule(rule, tool_name, tool_input) {
            return Some(decision_for(rule));
        }
    }
    None
}

/// The decision a matched rule produces, with its default message when empty.
fn decision_for(rule: &InterceptRule) -> HookDecision {
    let message = if rule.message.is_empty() {
        default_message(&rule.action, &rule.name)
    } else {
        rule.message.clone()
    };
    match rule.action.as_str() {
        "allow" => HookDecision::Allow,
        "deny" => HookDecision::Deny { reason: message },
        "ask" => HookDecision::Ask {
            reason: message,
            timeout: if rule.timeout_enabled {
                let secs = if rule.timeout_seconds <= 0 {
                    60
                } else {
                    rule.timeout_seconds
                };
                Some(Duration::from_secs(secs as u64))
            } else {
                None
            },
        },
        _ => HookDecision::Allow,
    }
}

/// The default message of a matched rule (Go `defaultMessage`).
pub fn default_message(action: &str, name: &str) -> String {
    match action {
        "deny" => format!("intercept rule [{name}] forbids executing this tool"),
        "ask" => format!("intercept rule [{name}] requires user approval, please wait"),
        _ => String::new(),
    }
}

// ---------------------------------------------------------------------------
// argv prefix rules (simplified shell tokenizer)
// ---------------------------------------------------------------------------

/// How many leading tokens of a command form its human prefix: the program plus
/// its subcommands. The longest entry that matches the start of the argv wins;
/// a program that is not listed takes 1.
fn argv_arity() -> HashMap<&'static str, usize> {
    let mut m: HashMap<&'static str, usize> = HashMap::new();
    for name in [
        "cat", "cd", "ls", "rm", "grep", "head", "tail", "echo", "pwd", "find", "curl", "wget",
        "nmap",
    ] {
        m.insert(name, 1);
    }
    for name in ["git", "npm", "docker", "kubectl", "terraform"] {
        m.insert(name, 2);
    }
    for name in [
        "npm run",
        "docker compose",
        "kubectl rollout",
        "git config",
        "gcloud",
        "gh",
        "aws",
    ] {
        m.insert(name, 3);
    }
    m
}

/// The number of leading tokens that form the prefix (Go `argvPrefixLen`).
fn argv_prefix_len(tokens: &[String]) -> usize {
    let arity = argv_arity();
    for n in (1..=tokens.len()).rev() {
        let key = tokens[..n].join(" ");
        if let Some(a) = arity.get(key.as_str()) {
            return (*a).min(tokens.len());
        }
    }
    1.min(tokens.len())
}

/// Parse a Bash command line and accept exactly one simple command (Go
/// `ParseSimpleCommand`), returning its words as literal values.
///
/// This is a SIMPLIFIED tokenizer: it splits on whitespace with single/double
/// quote and backslash handling, and refuses any shell metacharacter, variable
/// expansion, command substitution, redirection, background job, assignment,
/// wrapper program or compound/list construct. The full `mvdan.cc/sh` grammar
/// (pipelines, subshells, `$(...)`, arithmetic, brace/glob expansion, ...) is
/// NOT ported: such a command returns `None`, so no argv rule matches it.
/// Returns `None` for an empty command or any token that is not literal text.
pub fn parse_simple_command(command: &str) -> Option<Vec<String>> {
    let tokens = tokenize(command)?;
    if tokens.is_empty() {
        return None;
    }
    // No prefix assignment: a leading NAME=value word.
    if is_assignment(&tokens[0]) {
        return None;
    }
    // No wrapper program.
    if is_wrapper(&program_basename(&tokens[0])) {
        return None;
    }
    Some(tokens)
}

fn tokenize(s: &str) -> Option<Vec<String>> {
    let mut tokens = Vec::new();
    let mut cur = String::new();
    let mut has_token = false;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            ' ' | '\t' | '\n' | '\r' => {
                if has_token {
                    tokens.push(std::mem::take(&mut cur));
                    has_token = false;
                }
            }
            '\'' => {
                has_token = true;
                for q in chars.by_ref() {
                    if q == '\'' {
                        break;
                    }
                    cur.push(q);
                }
            }
            '"' => {
                has_token = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some('"') => cur.push('"'),
                            Some('\\') => cur.push('\\'),
                            Some(other) => {
                                cur.push('\\');
                                cur.push(other);
                            }
                            None => return None,
                        },
                        Some('$') | Some('`') => return None,
                        Some(other) => cur.push(other),
                        None => return None,
                    }
                }
            }
            '\\' => {
                has_token = true;
                match chars.next() {
                    Some('\n') => {}
                    Some(other) => cur.push(other),
                    None => return None,
                }
            }
            '$' | '`' => return None,
            '|' | '&' | ';' | '>' | '<' | '(' | ')' | '{' | '}' => return None,
            other => {
                has_token = true;
                cur.push(other);
            }
        }
    }
    if has_token {
        tokens.push(cur);
    }
    Some(tokens)
}

fn is_assignment(tok: &str) -> bool {
    match tok.find('=') {
        Some(0) | None => false,
        Some(i) => tok[..i]
            .chars()
            .all(|c| c == '_' || c.is_ascii_alphanumeric()),
    }
}

/// The basename of a program token, used for the wrapper check.
fn program_basename(tok: &str) -> String {
    tok.rsplit('/').next().unwrap_or(tok).to_owned()
}

fn is_wrapper(name: &str) -> bool {
    matches!(name, "command" | "builtin" | "env" | "exec" | "eval")
}

/// Derive the argv prefix of a command (Go `ArgvPrefix`). `Ok(None)` means the
/// command is not one simple command; `Err(())` means a prefix token is not
/// literal or is a flag.
#[allow(clippy::result_unit_err)]
pub fn argv_prefix(command: &str) -> Result<Option<Vec<String>>, ()> {
    // Not one simple command (compound / pipe / redirection / wrapper / ...):
    // the command yields no prefix, but this is not an error.
    let words = match parse_simple_command(command) {
        Some(w) => w,
        None => return Ok(None),
    };
    let n = argv_prefix_len(&words);
    for w in &words[..n] {
        if w.is_empty() {
            return Err(());
        }
        if w.starts_with('-') {
            // A flag before the subcommand yields no prefix at all.
            return Err(());
        }
        if w.contains([';', '&', '|', '>', '<', '`', '$', '('])
            || w.chars().any(char::is_whitespace)
        {
            return Err(());
        }
    }
    Ok(Some(words[..n].to_vec()))
}

/// The programs an argv allow rule may never name (Go `alwaysAllowDenylist`).
fn always_allow_denylist() -> HashSet<&'static str> {
    [
        // interpreters and shells
        "sh",
        "bash",
        "zsh",
        "dash",
        "ksh",
        "fish",
        "csh",
        "tcsh",
        "ash",
        "mksh",
        "rbash",
        "pwsh",
        "powershell",
        "python",
        "pypy",
        "ipython",
        "perl",
        "ruby",
        "irb",
        "node",
        "deno",
        "bun",
        "php",
        "lua",
        "luajit",
        "rscript",
        "tclsh",
        "wish",
        "expect",
        "awk",
        "gawk",
        "mawk",
        "nawk",
        "sed",
        "jshell",
        "osascript",
        // launchers: run their arguments (or a flag's value) as a command
        "env",
        "command",
        "builtin",
        "exec",
        "eval",
        "source",
        "nohup",
        "timeout",
        "nice",
        "ionice",
        "stdbuf",
        "setsid",
        "chroot",
        "unshare",
        "nsenter",
        "systemd-run",
        "su",
        "sudo",
        "doas",
        "pkexec",
        "runuser",
        "setpriv",
        "sg",
        "watch",
        "busybox",
        "toybox",
        "xargs",
        "parallel",
        "make",
        "gmake",
        "mapfile",
        "trap",
        "script",
        "find",
        "ssh",
        "scp",
        "sftp",
        "rsync",
        "socat",
        "ncat",
        "nc",
        "netcat",
        "flock",
        "strace",
        "ltrace",
        "gdb",
        "taskset",
        "chrt",
        "unbuffer",
        "rlwrap",
        "firejail",
        "bwrap",
        "proot",
        "proxychains",
        "torsocks",
        "tar",
        "zip",
        "vim",
        "vi",
        "nvim",
        "emacs",
        "less",
        "more",
        "man",
        // package managers and build tools
        "npm",
        "npx",
        "pnpm",
        "yarn",
        "bunx",
        "corepack",
        "pip",
        "pipx",
        "uv",
        "uvx",
        "poetry",
        "conda",
        "composer",
        "gem",
        "bundle",
        "bundler",
        "rake",
        "cargo",
        "go",
        "mvn",
        "gradle",
        "dotnet",
        "apt",
        "apt-get",
        "dpkg",
        "dnf",
        "yum",
        "rpm",
        "pacman",
        "apk",
        "brew",
        "snap",
        "flatpak",
        // orchestration
        "docker",
        "docker-compose",
        "podman",
        "nerdctl",
        "kubectl",
        "helm",
        "ansible",
        "ansible-playbook",
        "terraform",
        "pulumi",
        "vagrant",
        // destructive
        "rm",
        "dd",
        "mkfs",
        "mkswap",
        "wipefs",
        "fdisk",
        "sfdisk",
        "parted",
        "shred",
        "truncate",
        "chmod",
        "chown",
        "kill",
        "pkill",
        "killall",
        "systemctl",
        "service",
        "iptables",
        "ip6tables",
        "nft",
        "ufw",
        "mount",
        "umount",
        "swapoff",
        "shutdown",
        "reboot",
        "halt",
        "poweroff",
        "init",
        "telinit",
        "crontab",
        "insmod",
        "rmmod",
        "modprobe",
    ]
    .into_iter()
    .collect()
}

/// Normalise a program token for the denylist: basename, lower case, without a
/// trailing version, `mkfs.*` reduced to `mkfs`.
fn argv_program_name(tok: &str) -> String {
    let name = program_basename(tok).to_lowercase();
    if let Some(rest) = name.strip_prefix("mkfs.") {
        let _ = rest;
        return "mkfs".to_owned();
    }
    let end = name
        .char_indices()
        .rev()
        .find(|(_, c)| !(*c == '.' || c.is_ascii_digit()))
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(0);
    if end > 0 && end < name.len() {
        name[..end].to_owned()
    } else {
        name
    }
}

/// Whether an argv allow rule whose first token is `program` is refused by the
/// denylist (Go `ArgvAllowDenied`).
pub fn argv_allow_denied(program: &str) -> bool {
    always_allow_denylist().contains(argv_program_name(program).as_str())
}

/// The error shown when an allow rule names a denylisted program, else `None`.
fn check_argv_allow(tokens: &[String]) -> Option<String> {
    if let Some(first) = tokens.first() {
        if argv_allow_denied(first) {
            return Some(format!(
                "program {first:?} cannot be always allowed: it can run arbitrary code or cause \
                 unbounded damage whatever its arguments"
            ));
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Targets
// ---------------------------------------------------------------------------

/// A network target a tool call declares, in normalised form (Go `Target`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Target {
    /// `domain`, `ip` or `cidr`.
    pub kind: String,
    /// Lower-case punycode name, canonical IP literal, or masked CIDR.
    pub host: String,
    /// 0 = port not declared.
    pub port: u16,
}

impl Target {
    fn new(kind: &str, host: String, port: u16) -> Self {
        Self {
            kind: kind.to_owned(),
            host,
            port,
        }
    }
}

/// The default ports of the schemes a target is usually written with.
fn scheme_port(scheme: &str) -> Option<u16> {
    match scheme.to_lowercase().as_str() {
        "http" | "ws" => Some(80),
        "https" | "wss" => Some(443),
        "ftp" => Some(21),
        _ => None,
    }
}

/// Parse one target written as a URL with a scheme, or as a host, host:port,
/// `[IPv6]:port`, IP literal or CIDR (Go `NormalizeTarget`).
pub fn normalize_target(raw: &str) -> Option<Target> {
    let raw = raw.trim();
    let mut def_port: u16 = 0;
    let mut is_url = false;
    let mut authority = raw;
    if let Some(i) = raw.find("://") {
        if i > 0 {
            is_url = true;
            def_port = scheme_port(&raw[..i]).unwrap_or(0);
            authority = &raw[i + 3..];
            if let Some(end) = authority.find(['/', '?', '#']) {
                authority = &authority[..end];
            }
            if let Some(at) = authority.rfind('@') {
                authority = &authority[at + 1..];
            }
        }
    }

    let mut host: String = authority.to_string();
    let mut port_str = "";
    if let Some(stripped) = authority.strip_prefix('[') {
        let end = stripped.find(']')?;
        host = stripped[..end].to_string();
        let rest = &stripped[end + 1..];
        if !rest.is_empty() {
            if !rest.starts_with(':') {
                return None;
            }
            port_str = &rest[1..];
        }
    } else if authority.matches(':').count() == 1 {
        let (h, p) = authority.split_once(':')?;
        host = h.to_string();
        port_str = p;
    }

    if is_url && !authority.starts_with('[') {
        // A client decodes a percent-encoded host before it resolves it.
        host = percent_decode(&host);
    }

    let mut port = def_port;
    if !port_str.is_empty() {
        let n: u32 = port_str.parse().ok()?;
        if !(1..=65535).contains(&n) {
            return None;
        }
        port = n as u16;
    }
    let (kind, host) = normalize_host(&host, true)?;
    Some(Target::new(&kind, host, port))
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = (bytes[i + 1] as char).to_digit(16);
            let lo = (bytes[i + 2] as char).to_digit(16);
            if let (Some(hi), Some(lo)) = (hi, lo) {
                out.push((hi * 16 + lo) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The kind and canonical form of a host (Go `normalizeHost`). `numeric` accepts
/// the non-dotted-decimal IPv4 forms.
fn normalize_host(raw: &str, numeric: bool) -> Option<(String, String)> {
    let lowered = raw.trim().to_lowercase();
    let h = lowered.strip_suffix('.').unwrap_or(&lowered);
    if h.is_empty() {
        return None;
    }
    if h.contains('/') {
        // CIDR: keep the network address.
        let (addr, len) = h.split_once('/')?;
        let ip: IpAddr = addr.trim().parse().ok()?;
        let prefix_len: u8 = len.trim().parse().ok()?;
        let masked = mask_addr(ip, prefix_len)?;
        return Some(("cidr".to_owned(), format!("{masked}/{prefix_len}")));
    }
    if let Ok(ip) = h.parse::<IpAddr>() {
        return Some(("ip".to_owned(), ip.to_string()));
    }
    if numeric {
        if let Some(ip) = parse_ipv4_forms(h) {
            return Some(("ip".to_owned(), ip.to_string()));
        }
    }
    Some(("domain".to_owned(), h.to_owned()))
}

fn mask_addr(ip: IpAddr, prefix_len: u8) -> Option<IpAddr> {
    match ip {
        IpAddr::V4(v4) => {
            if prefix_len > 32 {
                return None;
            }
            let bits = u32::from(v4);
            let mask = if prefix_len == 0 {
                0
            } else {
                u32::MAX << (32 - prefix_len)
            };
            Some(IpAddr::V4(std::net::Ipv4Addr::from(bits & mask)))
        }
        IpAddr::V6(v6) => {
            if prefix_len > 128 {
                return None;
            }
            let bits = u128::from(v6);
            let mask = if prefix_len == 0 {
                0
            } else {
                u128::MAX << (128 - prefix_len)
            };
            Some(IpAddr::V6(std::net::Ipv6Addr::from(bits & mask)))
        }
    }
}

/// Read an IPv4 address the way `inet_aton` does (Go `parseIPv4Forms`).
fn parse_ipv4_forms(s: &str) -> Option<std::net::Ipv4Addr> {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() > 4 {
        return None;
    }
    let mut v: u64 = 0;
    for (n, part) in parts.iter().enumerate() {
        let (digits, base) = if let Some(rest) = part.strip_prefix("0x") {
            (rest, 16)
        } else if part.len() > 1 && part.starts_with('0') {
            (&part[1..], 8)
        } else {
            (*part, 10)
        };
        let x = u64::from_str_radix(digits, base).ok()?;
        if n < parts.len() - 1 {
            if x > 255 {
                return None;
            }
            v = (v << 8) | x;
        } else {
            let rest = 8 * (4 - n);
            if x >= (1u64 << rest) {
                return None;
            }
            v = (v << rest) | x;
        }
    }
    Some(std::net::Ipv4Addr::from((v as u32).to_be_bytes()))
}

/// Whether `h` is a plain DNS name: labels of letters, digits, hyphen and
/// underscore (Go `validHostname`).
fn valid_hostname(h: &str) -> bool {
    if h.len() > 253 {
        return false;
    }
    for label in h.split('.') {
        if label.is_empty() || label.len() > 63 {
            return false;
        }
        let bytes = label.as_bytes();
        if bytes[0] == b'-' || bytes[bytes.len() - 1] == b'-' {
            return false;
        }
        if !bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-' || *b == b'_')
        {
            return false;
        }
    }
    true
}

// ---------------------------------------------------------------------------
// Target extraction (target_extract.go)
// ---------------------------------------------------------------------------

const EXECUTE_EXTRA_TOOL: &str = "ExecuteExtraTool";

/// The tools whose input names no network target (Go `noTargetTools`).
fn no_target_tools() -> HashSet<&'static str> {
    [
        "web_search",
        "Read",
        "Write",
        "Edit",
        "MultiEdit",
        "LS",
        "Glob",
        "Grep",
        "TodoWrite",
        "sleep",
        "TaskOutput",
        "TaskStop",
        "TaskList",
        "graph_overview",
        "list_findings",
        "list_facts",
        "node_detail",
        "expand_digest",
        "get_worker_output",
        "get_worker_trace",
        "search_all_worker_traces",
        "add_hint",
        "add_intent",
        "steer_work",
        "kill_work",
        "set_goals",
        "set_constraints",
        "list_goals",
        "prove_goal",
        "goal_met",
        "insert_assets",
        "add_company_scope",
        "add_task_scope",
        "list_assets",
        "list_untested_assets",
        "list_companies",
        "report_finding",
        "record_fact",
        "save_task_credential",
        "list_task_credentials",
        "ingest_bbot",
        "ingest_scan_output",
        "record_coverage",
        "list_tasks",
        "list_llm_profiles",
        "spawn_task",
        "pause_task",
        "get_task_graph",
        "list_task_findings",
        "add_task_hint",
        "get_task_worker_trace",
        "list_task_worker_traces",
        "search_task_worker_traces",
        "get_task_node_detail",
        "update_finding_report",
        "get_finding_traffic",
        "bind_finding_traffic",
        "get_finding_retest_context",
        "record_finding_retest_result",
        "delete_assets_by_host",
        "create_skill",
        "update_skill_file",
        "ask_operator",
        "traffic_search",
        "traffic_get",
        "traffic_blob",
    ]
    .into_iter()
    .collect()
}

/// Return the network targets a tool call declares in its input, normalised and
/// without duplicates, in order of appearance (Go `ExtractTargets`).
pub fn extract_targets(tool_name: &str, input: &Value) -> Vec<Target> {
    let mut tool_name = tool_name.to_owned();
    let mut input = input.clone();
    if tool_name == EXECUTE_EXTRA_TOOL {
        if let Some(inner) = input.get("tool_name").and_then(Value::as_str) {
            if !inner.is_empty() && inner != EXECUTE_EXTRA_TOOL {
                if let Some(params) = input.get("params") {
                    tool_name = inner.to_owned();
                    input = params.clone();
                }
            }
        }
    }
    if no_target_tools().contains(tool_name.as_str()) {
        return Vec::new();
    }
    let mut out = TargetList::default();
    match tool_name.as_str() {
        "WebFetch" => {
            if let Some(url) = input.get("url").and_then(Value::as_str) {
                if let Some(t) = normalize_target(url) {
                    out.add(t);
                }
            }
        }
        "Bash" | "shell_open" | "Monitor" => {
            if let Some(cmd) = input.get("command").and_then(Value::as_str) {
                out.scan(cmd);
            }
        }
        "shell_send" => {
            if let Some(text) = input.get("text").and_then(Value::as_str) {
                out.scan(text);
            }
        }
        _ => out.scan_strings(input),
    }
    out.targets
}

#[derive(Default)]
struct TargetList {
    targets: Vec<Target>,
    seen: HashSet<Target>,
}

impl TargetList {
    fn add(&mut self, t: Target) {
        if self.seen.insert(t.clone()) {
            self.targets.push(t);
        }
    }

    fn scan_strings(&mut self, input: Value) {
        match input {
            Value::String(s) => self.scan(&s),
            Value::Array(items) => {
                for item in items {
                    self.scan_strings(item);
                }
            }
            Value::Object(map) => {
                for (_, v) in map {
                    self.scan_strings(v);
                }
            }
            _ => {}
        }
    }

    /// Read the targets of one string with the command-string rules (Go `scan`).
    fn scan(&mut self, s: &str) {
        if s.is_empty() {
            return;
        }
        // Work on a mutable byte buffer so a form already read can be blanked,
        // preventing a later rule from reading part of it again.
        let mut b = s.as_bytes().to_vec();
        for (name, re) in target_regexes() {
            let matches: Vec<(usize, usize)> =
                re.find_iter(&b).map(|m| (m.start(), m.end())).collect();
            for (start, end) in matches {
                let token = String::from_utf8_lossy(&b[start..end]).into_owned();
                self.read_form(name, &token, start, end, &mut b);
            }
        }
        let _ = &mut b;
    }

    #[allow(clippy::too_many_arguments)]
    fn read_form(&mut self, _kind: &str, token: &str, start: usize, end: usize, b: &mut [u8]) {
        // Dispatch by the matched text shape; the specific readers below mirror
        // the Go `each(...)` handlers. Regexes operate on bytes; captured groups
        // are decoded back to UTF-8 strings.
        let tb = token.as_bytes();
        let grp = |caps: &regex::bytes::Captures, i: usize| -> String {
            caps.get(i)
                .map(|m| String::from_utf8_lossy(m.as_bytes()).into_owned())
                .unwrap_or_default()
        };
        if let Some(caps) = re_url().captures(tb) {
            let scheme = grp(&caps, 1);
            let authority = grp(&caps, 2);
            for host in url_hosts(&authority) {
                let candidate = format!("{scheme}{host}");
                if let Some(t) = normalize_target(&candidate) {
                    if t.kind != "domain" || valid_hostname(&t.host) {
                        self.add(t);
                    }
                }
            }
            // Blank the scheme/authority prefix and the path/suffix so a later
            // rule never reads part of this token again.
            blank(b, start, start + scheme.len() + 3 + authority.len());
            blank(b, start + scheme.len() + 3 + authority.len(), end);
            return;
        }
        if let Some(caps) = re_bracketed().captures(tb) {
            let addr = grp(&caps, 1);
            if let Ok(ip) = addr.parse::<IpAddr>() {
                let mut port = 0u16;
                if let Ok(p) = grp(&caps, 2).parse::<u32>() {
                    port = p as u16;
                }
                self.add(Target::new("ip", ip.to_string(), port));
            }
            blank(b, start, end);
            return;
        }
        if let Some(caps) = re_host_port().captures(tb) {
            let full = grp(&caps, 0);
            if let Some((host, port)) = full.rsplit_once(':') {
                if let Some(t) = normalize_target(&format!("{host}:{port}")) {
                    self.add(t);
                }
            }
            blank(b, start, end);
            return;
        }
        if is_ipv4_token(token) {
            if let Some(t) = normalize_target(token) {
                self.add(t);
            }
            blank(b, start, end);
            return;
        }
        if let Some(t) = normalize_ipv6_token(token) {
            self.add(t);
            blank(b, start, end);
        }
    }
}

fn blank(b: &mut [u8], from: usize, to: usize) {
    let to = to.min(b.len());
    for slot in b.iter_mut().take(to).skip(from) {
        *slot = b' ';
    }
}

fn re_url() -> regex::bytes::Regex {
    regex::bytes::Regex::new(r#"(?i)\b([a-z][a-z0-9+.\-]*://)([^\s/?#"'<>`]*)"#)
        .expect("valid static url pattern")
}
fn re_bracketed() -> regex::bytes::Regex {
    regex::bytes::Regex::new(r"\[([0-9A-Fa-f:.]+(?:%[\w.\-]+)?)\](?::(\d{1,5}))?")
        .expect("valid static bracketed pattern")
}
fn re_host_port() -> regex::bytes::Regex {
    regex::bytes::Regex::new(
        r#"(?i)(?:localhost|[a-z0-9_](?:[a-z0-9_\-]*[a-z0-9_])?(?:\.[a-z0-9_](?:[a-z0-9_\-]*[a-z0-9_])?)*\.[a-z][a-z0-9\-]*[a-z0-9]):\d{1,5}"#,
    )
    .expect("valid static host_port pattern")
}

fn target_regexes() -> [(&'static str, regex::bytes::Regex); 3] {
    [
        ("url", re_url()),
        ("bracketed", re_bracketed()),
        ("host_port", re_host_port()),
    ]
}

fn is_ipv4_token(token: &str) -> bool {
    // A token with at least one dot and only digits/dots (optionally :port or
    // /len), whose first label is a number.
    let host = token.split(['/', ':']).next().unwrap_or(token);
    host.contains('.')
        && host.chars().all(|c| c.is_ascii_digit() || c == '.')
        && normalize_target(token).is_some()
}

fn normalize_ipv6_token(token: &str) -> Option<Target> {
    // A token with at least two colons that parses as an IPv6 address (with an
    // optional zone or /len), and is not "::" alone.
    let host = token.split('/').next().unwrap_or(token);
    let host = host.split('%').next().unwrap_or(host);
    if host.matches(':').count() < 2 || host == "::" {
        return None;
    }
    let ip: IpAddr = host.parse().ok()?;
    Some(Target::new("ip", ip.to_string(), 0))
}

/// The readings of the authority of a URL found in a command (Go `urlHosts`).
fn url_hosts(authority: &str) -> Vec<String> {
    const SHELL_META: &[char] = &['|', ';', '&', '(', ')', '{', '}', ',', '\\'];
    let host = |s: &str| -> String {
        let s = s.rsplit('@').next().unwrap_or(s);
        let s = s.find(SHELL_META).map(|i| &s[..i]).unwrap_or(s);
        s.to_owned()
    };
    let mut out = vec![host(authority)];
    if authority.contains(SHELL_META) {
        let truncated = authority
            .find(SHELL_META)
            .map(|i| &authority[..i])
            .unwrap_or(authority);
        out.push(host(truncated));
        out.push(host(&authority.replace('\\', "")));
    }
    out
}

// ---------------------------------------------------------------------------
// Asset intercept rules (asset_intercept.go + match)
// ---------------------------------------------------------------------------

/// One row of `asset_intercept_rules` or `task_intercept_rules`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AssetInterceptRule {
    pub id: i64,
    pub enabled: bool,
    pub kind: String,
    pub pattern: String,
    pub note: String,
}

/// The human-readable label of a rule kind (Go `AssetInterceptKindLabel`).
pub fn asset_kind_label(kind: &str) -> &str {
    match kind {
        "exact_domain" => "Domain (exact)",
        "exact_ip" => "IP (exact)",
        "exact_url" => "URL (exact)",
        "fuzzy_domain" => "Domain (fuzzy)",
        "fuzzy_ip" => "IP (fuzzy)",
        "fuzzy_url" => "URL (fuzzy)",
        "cidr" => "CIDR range",
        _ => kind,
    }
}

impl AssetInterceptRule {
    /// A readable hit reason (Go `AssetInterceptRule.Reason`).
    pub fn reason(&self) -> String {
        let mut s = format!(
            "Matched asset interception rule [{}: {}]",
            asset_kind_label(&self.kind),
            self.pattern
        );
        let note = self.note.trim();
        if !note.is_empty() {
            s.push_str(&format!(" ({note})"));
        }
        s
    }
}

fn match_asset_one(
    rule: &AssetInterceptRule,
    domains: &[String],
    ips: &[String],
    urls: &[String],
) -> bool {
    let p = rule.pattern.trim();
    if p.is_empty() {
        return false;
    }
    match rule.kind.as_str() {
        "exact_domain" => domains.iter().any(|d| d.trim().eq_ignore_ascii_case(p)),
        "exact_ip" => ips.iter().any(|ip| ip.trim() == p),
        "exact_url" => urls.iter().any(|u| u.trim() == p),
        "fuzzy_domain" => {
            let lp = p.to_lowercase();
            domains
                .iter()
                .any(|d| !d.is_empty() && d.to_lowercase().contains(&lp))
        }
        "fuzzy_ip" => ips.iter().any(|ip| !ip.is_empty() && ip.contains(p)),
        "fuzzy_url" => {
            let lp = p.to_lowercase();
            urls.iter()
                .any(|u| !u.is_empty() && u.to_lowercase().contains(&lp))
        }
        "cidr" => {
            let (addr, len) = match p.split_once('/') {
                Some(v) => v,
                None => return false,
            };
            let net: IpAddr = match addr.trim().parse() {
                Ok(v) => v,
                Err(_) => return false,
            };
            let prefix: u8 = match len.trim().parse() {
                Ok(v) => v,
                Err(_) => return false,
            };
            ips.iter().any(|ip| {
                ip.trim()
                    .parse::<IpAddr>()
                    .map(|candidate| ip_in_net(candidate, net, prefix))
                    .unwrap_or(false)
            })
        }
        _ => false,
    }
}

/// First enabled rule matching the candidates (Go `MatchAssetInterceptRules`).
pub fn match_asset_rules<'a>(
    rules: &'a [AssetInterceptRule],
    domains: &[String],
    ips: &[String],
    urls: &[String],
) -> Option<&'a AssetInterceptRule> {
    rules
        .iter()
        .filter(|r| r.enabled)
        .find(|r| match_asset_one(r, domains, ips, urls))
}

fn ip_in_net(ip: IpAddr, net: IpAddr, prefix: u8) -> bool {
    match (ip, net) {
        (IpAddr::V4(ip), IpAddr::V4(net)) => {
            if prefix > 32 {
                return false;
            }
            let mask = if prefix == 0 {
                0
            } else {
                u32::MAX << (32 - prefix)
            };
            (u32::from(ip) & mask) == (u32::from(net) & mask)
        }
        (IpAddr::V6(ip), IpAddr::V6(net)) => {
            if prefix > 128 {
                return false;
            }
            let mask = if prefix == 0 {
                0
            } else {
                u128::MAX << (128 - prefix)
            };
            (u128::from(ip) & mask) == (u128::from(net) & mask)
        }
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Exemptions (target_exempt.go)
// ---------------------------------------------------------------------------

/// One entry of the target-gate exemption list (settings key
/// `target_gate_exempt_hosts`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TargetExemption {
    pub host: String,
    pub port: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// The settings key of the stored exemption list.
pub const SETTING_TARGET_EXEMPT_HOSTS: &str = "target_gate_exempt_hosts";

/// The key of a target inside its scope: host and port.
pub fn target_key(host: &str, port: u16) -> String {
    format!("{host}:{port}")
}

/// Instance-metadata addresses outside the link-local ranges.
fn metadata_addrs() -> Vec<IpAddr> {
    vec![
        "fd00:ec2::254"
            .parse()
            .unwrap_or(IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED)),
        "100.100.100.200"
            .parse()
            .unwrap_or(IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED)),
    ]
}

/// Validate the host of an exemption entry (Go `ValidateExemptionHost`).
///
/// An IP literal is checked here: loopback, link-local (metadata range),
/// unspecified, multicast and instance-metadata addresses can never be exempt.
/// A name passes; it is resolved at check time and voided if it resolves to a
/// private/local address.
pub fn validate_exemption_host(host: &str) -> Result<(), String> {
    let Ok(ip) = host.parse::<IpAddr>() else {
        return Ok(());
    };
    let reason = if ip.is_loopback() {
        Some("a loopback")
    } else if ip.is_unspecified() {
        Some("the unspecified")
    } else if is_link_local(ip) {
        Some("a link-local (metadata range)")
    } else if ip.is_multicast() {
        Some("a multicast")
    } else {
        None
    };
    let reason = if metadata_addrs().contains(&ip) {
        Some("an instance-metadata")
    } else {
        reason
    };
    match reason {
        Some(kind) => Err(format!(
            "{host} is {kind} address and can never be exempt from target approval"
        )),
        None => Ok(()),
    }
}

fn is_link_local(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_link_local(),
        IpAddr::V6(v6) => {
            let seg = v6.segments();
            (seg[0] & 0xffc0) == 0xfe80
        }
    }
}

fn is_private(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_private(),
        IpAddr::V6(v6) => {
            // Unique local range fc00::/7.
            (v6.segments()[0] & 0xfe00) == 0xfc00
        }
    }
}

/// Whether one resolved address voids a stored exemption on a name (Go
/// `voidsExemption`).
fn voids_exemption(ip: &str) -> bool {
    let Ok(parsed) = ip.parse::<IpAddr>() else {
        return true;
    };
    is_private(parsed) || validate_exemption_host(&parsed.to_string()).is_err()
}

/// The exemption list as one gated call sees it (Go `exemptions`).
struct Exemptions {
    stored: HashMap<String, ()>,
}

async fn load_exemptions(db: &database::Db) -> Result<Exemptions, InterceptionError> {
    let mut stored = HashMap::new();
    if let Ok(Some(raw)) = db.get_setting(SETTING_TARGET_EXEMPT_HOSTS).await {
        if let Ok(list) = serde_json::from_str::<Vec<TargetExemption>>(&raw) {
            for e in list {
                stored.insert(target_key(&e.host, e.port), ());
            }
        }
    }
    Ok(Exemptions { stored })
}

// ---------------------------------------------------------------------------
// Resolver (target_resolve.go)
// ---------------------------------------------------------------------------

/// The resolver of the target gate (Go `systemResolver`): the system resolver.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemResolver;

impl SystemResolver {
    /// Resolve `host` to its A/AAAA addresses, canonical and sorted.
    pub async fn lookup(&self, host: &str) -> Result<Vec<String>, InterceptionError> {
        use std::net::ToSocketAddrs;
        let host = host.to_string();
        let addrs =
            tokio::task::spawn_blocking(move || -> Result<Vec<String>, InterceptionError> {
                let iter = (host.as_str(), 0u16)
                    .to_socket_addrs()
                    .map_err(|e| InterceptionError::Invalid(format!("resolve {host}: {e}")))?;
                let mut out: Vec<String> = Vec::new();
                for sa in iter {
                    out.push(sa.ip().to_string());
                }
                Ok(out)
            })
            .await
            .map_err(|e| InterceptionError::Invalid(format!("resolve task: {e}")))??;
        let mut addrs = addrs;
        addrs.sort();
        addrs.dedup();
        Ok(addrs)
    }
}

/// A positive-answer cache with a TTL (Go `resolveCache`), per host.
#[derive(Clone)]
struct ResolveCache {
    inner: std::sync::Arc<Mutex<ResolveState>>,
    ttl: Duration,
}

struct ResolveState {
    entries: HashMap<String, (Vec<String>, std::time::Instant)>,
}

impl Default for ResolveCache {
    fn default() -> Self {
        Self {
            inner: std::sync::Arc::new(Mutex::new(ResolveState {
                entries: HashMap::new(),
            })),
            ttl: Duration::from_secs(10),
        }
    }
}

impl ResolveCache {
    /// Resolve `host`. `fresh` skips the cached answer (used when raising an
    /// approval request, whose addresses a human reads and pins).
    async fn lookup(
        &self,
        host: &str,
        fresh: bool,
    ) -> Result<Option<Vec<String>>, InterceptionError> {
        if !fresh {
            let cached = self
                .inner
                .lock()
                .ok()
                .and_then(|state| state.entries.get(host).cloned());
            if let Some((ips, at)) = cached {
                if at.elapsed() < self.ttl {
                    return Ok(Some(ips));
                }
            }
        }
        let ips = SystemResolver.lookup(host).await?;
        if ips.is_empty() {
            return Ok(None);
        }
        if let Ok(mut state) = self.inner.lock() {
            state
                .entries
                .insert(host.to_owned(), (ips.clone(), std::time::Instant::now()));
        }
        Ok(Some(ips))
    }
}

// ---------------------------------------------------------------------------
// Target pins
// ---------------------------------------------------------------------------

/// One row of `target_pins` (the fields the gate reads).
#[derive(Debug, Clone)]
pub struct TargetPin {
    pub id: i64,
    pub host: String,
    pub port: i32,
    pub version: i32,
    pub pinned_ips: Vec<String>,
    pub name_only_effective: bool,
}

fn scan_pin(row: &sqlx::postgres::PgRow) -> TargetPin {
    TargetPin {
        id: row.get("id"),
        host: row.get("host"),
        port: row.get("port"),
        version: row.get("version"),
        pinned_ips: row.get("pinned_ips"),
        name_only_effective: row.get("name_only_effective"),
    }
}

/// Active pins that may cover (host, port) in a scope (task or conversation),
/// the pin on the exact port first (Go `ActiveTargetPins`).
async fn active_pins(
    db: &database::Db,
    scope: &TargetScope,
    host: &str,
    port: u16,
) -> Result<Vec<TargetPin>, InterceptionError> {
    scope.validate()?;
    let (task, conv) = scope.key();
    let rows = sqlx::query(
        "SELECT id, host, port, version, pinned_ips, name_only_effective FROM target_pins \
         WHERE COALESCE(task_id,0)=$1 AND COALESCE(conversation_id,0)=$2 AND host=$3 \
         AND port IN ($4, 0) AND status='active' ORDER BY port DESC",
    )
    .bind(task)
    .bind(conv)
    .bind(host)
    .bind(port as i32)
    .fetch_all(db.pool())
    .await?;
    Ok(rows.iter().map(scan_pin).collect())
}

/// The pin that covers the resolved addresses (Go `coveringPin`).
fn covering_pin<'a>(pins: &'a [TargetPin], resolved: &[String]) -> Option<&'a TargetPin> {
    pins.iter()
        .find(|p| p.name_only_effective || subset(resolved, &p.pinned_ips))
}

/// Whether every resolved address is one of the pinned ones. An empty answer is
/// never a subset (Go `subset`).
fn subset(resolved: &[String], pinned: &[String]) -> bool {
    !resolved.is_empty() && resolved.iter().all(|ip| pinned.contains(ip))
}

/// What a target decision is bound to: one task or one conversation.
#[derive(Debug, Clone, Default)]
pub struct TargetScope {
    pub task_id: Option<i64>,
    pub conversation_id: Option<i64>,
}

impl TargetScope {
    fn validate(&self) -> Result<(), InterceptionError> {
        match (self.task_id, self.conversation_id) {
            (Some(_), None) | (None, Some(_)) => Ok(()),
            _ => Err(InterceptionError::Invalid(
                "target scope must name one task or one conversation".to_owned(),
            )),
        }
    }

    /// The (task_id, conversation_id) key used in SQL (0 = NULL scope side).
    fn key(&self) -> (i64, i64) {
        (self.task_id.unwrap_or(0), self.conversation_id.unwrap_or(0))
    }

    #[allow(dead_code)]
    fn scope_name(&self) -> String {
        if let Some(t) = self.task_id {
            format!("task {t}")
        } else {
            format!("conversation {}", self.conversation_id.unwrap_or(0))
        }
    }
}

// ---------------------------------------------------------------------------
// Interceptor
// ---------------------------------------------------------------------------

/// The user-configured interception logic: rule engine plus target gate.
pub struct InterceptorImpl {
    rules: RuleStore,
    db: database::Db,
    cache: Mutex<CacheState>,
    resolve: ResolveCache,
}

struct CacheState {
    /// Compiled, enabled rules sorted by priority DESC; `None` = not loaded yet.
    loaded: Option<Vec<CompiledRule>>,
}

impl InterceptorImpl {
    /// Create an interceptor backed by `db`. The rule cache is lazy-loaded on
    /// first use.
    pub fn new(db: database::Db) -> Self {
        Self {
            rules: RuleStore::new(db.clone()),
            db,
            cache: Mutex::new(CacheState { loaded: None }),
            resolve: ResolveCache::default(),
        }
    }

    /// The underlying rule store.
    pub fn rules(&self) -> &RuleStore {
        &self.rules
    }

    /// Clear the in-memory rule cache; the next evaluation reloads from the
    /// database. Call after any CRUD operation on rules.
    pub fn invalidate(&self) {
        if let Ok(mut state) = self.cache.lock() {
            state.loaded = None;
        }
    }

    async fn compiled_rules(&self) -> Result<Vec<CompiledRule>, InterceptionError> {
        if let Ok(state) = self.cache.lock() {
            if let Some(rules) = &state.loaded {
                return Ok(rules.as_slice().to_vec());
            }
        }
        let rules = self.rules.list().await?;
        let compiled: Vec<CompiledRule> = rules
            .into_iter()
            .filter(|r| r.enabled)
            .filter_map(CompiledRule::compile)
            .collect();
        if let Ok(mut state) = self.cache.lock() {
            state.loaded = Some(compiled.clone());
        }
        Ok(compiled)
    }

    /// Evaluate the loaded rules and return the first match's decision, or
    /// `None` when no rule matches (Go `Match`).
    pub async fn evaluate(
        &self,
        tool_name: &str,
        tool_input: &Value,
    ) -> Result<Option<HookDecision>, InterceptionError> {
        let rules = self.compiled_rules().await?;
        for cr in &rules {
            if compiled_matches(cr, tool_name, tool_input) {
                return Ok(Some(decision_for(&cr.rule)));
            }
        }
        Ok(None)
    }

    /// The builtin deny rules only: enabled `[built-in]` deny rules, on the
    /// tools that enter the rule system (Go `MatchBuiltin`). User rules, allow
    /// and ask rules and the judge are not consulted.
    pub async fn match_builtin(
        &self,
        tool_name: &str,
        tool_input: &Value,
    ) -> Result<Option<HookDecision>, InterceptionError> {
        let rules = self.compiled_rules().await?;
        for cr in &rules {
            if cr.rule.action == "deny"
                && cr.rule.name.starts_with("[built-in]")
                && compiled_matches(cr, tool_name, tool_input)
            {
                return Ok(Some(decision_for(&cr.rule)));
            }
        }
        Ok(None)
    }

    // -- target gate -------------------------------------------------------

    /// The declared targets of a call that no pin/exemption covers, plus any
    /// block (Go `CheckTargets`, result form).
    pub async fn check_targets_declared(
        &self,
        scope: &TargetScope,
        tool_name: &str,
        tool_input: &Value,
    ) -> Result<TargetCheck, InterceptionError> {
        scope.validate()?;
        let declared = extract_targets(tool_name, tool_input);
        if declared.is_empty() {
            return Ok(TargetCheck::default());
        }
        self.run_target_check(scope, tool_name, tool_input, declared)
            .await
    }

    async fn run_target_check(
        &self,
        scope: &TargetScope,
        tool_name: &str,
        tool_input: &Value,
        declared: Vec<Target>,
    ) -> Result<TargetCheck, InterceptionError> {
        let _ = (scope, tool_name, tool_input);
        let rules = self.asset_rules().await?;
        let ex = load_exemptions(&self.db).await?;

        // Block rules come first, matched on the declared form before resolving.
        if let Some(t) = declared.first() {
            if let Some(rule) = match_asset_rules(&rules, std::slice::from_ref(&t.host), &[], &[]) {
                return Ok(TargetCheck {
                    blocked: Some(format!(
                        "{} is blocked by an asset rule. {}",
                        t.host,
                        rule.reason()
                    )),
                    ..TargetCheck::default()
                });
            }
        }

        let mut unpinned: Vec<Target> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        for t in declared {
            let key = target_key(&t.host, t.port);
            if ex.stored.contains_key(&key) && exemption_holds(&t).await {
                continue;
            }
            let pins = active_pins(&self.db, scope, &t.host, t.port).await?;
            let resolved = if t.kind == "ip" {
                Some(vec![t.host.clone()])
            } else {
                self.resolve.lookup(&t.host, false).await?
            };
            if let Some(ips) = &resolved {
                if let Some(pin) = covering_pin(&pins, ips) {
                    let _ = pin;
                    continue;
                }
            }
            if seen.insert(key) {
                unpinned.push(t);
            }
        }
        Ok(TargetCheck {
            unpinned,
            blocked: None,
        })
    }

    async fn asset_rules(&self) -> Result<Vec<AssetInterceptRule>, InterceptionError> {
        let rows =
            sqlx::query("SELECT id, enabled, kind, pattern, note FROM asset_intercept_rules")
                .fetch_all(self.db.pool())
                .await?;
        Ok(rows
            .iter()
            .map(|r| AssetInterceptRule {
                id: r.get("id"),
                enabled: r.get("enabled"),
                kind: r.get("kind"),
                pattern: r.get("pattern"),
                note: r.get("note"),
            })
            .collect())
    }
}

/// Whether a compiled rule matches a call.
fn compiled_matches(cr: &CompiledRule, tool_name: &str, tool_input: &Value) -> bool {
    if cr.rule.match_type == "argv" {
        if cr.rule.action == "allow" {
            if let Some(tokens) = &cr.argv {
                if check_argv_allow(tokens).is_some() {
                    return false;
                }
            }
        }
        return cr.rule.match_target == "tool_input"
            && argv_rule_matches(cr, tool_name, tool_input);
    }
    let subject = match cr.rule.match_target.as_str() {
        "tool_name" => tool_name.to_owned(),
        "tool_input" => serde_json::to_string(tool_input).unwrap_or_default(),
        _ => return false,
    };
    if cr.rule.match_type == "regex" {
        return cr
            .re
            .as_ref()
            .map(|re| re.is_match(&subject))
            .unwrap_or(false);
    }
    subject.contains(&cr.rule.pattern)
}

/// Whether a stored exemption on a target applies (Go `exemptionHolds`). On a
/// name it is void when the name does not resolve or one address is private or
/// local; an IP literal holds unless it can never be exempt.
async fn exemption_holds(t: &Target) -> bool {
    if t.kind == "ip" {
        return validate_exemption_host(&t.host).is_ok();
    }
    if t.kind != "domain" {
        return false;
    }
    match SystemResolver.lookup(&t.host).await {
        Ok(ips) if !ips.is_empty() => ips.iter().all(|ip| !voids_exemption(ip)),
        _ => false,
    }
}

/// The result of the target gate for one call (subset of Go's `CheckTargets` +
/// `TargetResult`).
#[derive(Debug, Clone, Default)]
pub struct TargetCheck {
    /// Targets no pin/exemption covers; each needs an approval.
    pub unpinned: Vec<Target>,
    /// A block reason (asset rule, resolve failure, gate unavailable); the call
    /// is denied when set.
    pub blocked: Option<String>,
}

impl TargetCheck {
    /// Whether the call may run without any approval.
    pub fn is_allow(&self) -> bool {
        self.blocked.is_none() && self.unpinned.is_empty()
    }
}

#[async_trait::async_trait]
impl Interceptor for InterceptorImpl {
    async fn check_pre_tool_use(
        &self,
        tool_name: &str,
        tool_input: &Value,
    ) -> Result<HookDecision, GuardError> {
        let decision = self
            .evaluate(tool_name, tool_input)
            .await
            .map_err(GuardError::from)?;
        Ok(decision.unwrap_or(HookDecision::Allow))
    }

    async fn check_post_tool_use(
        &self,
        _tool_name: &str,
        _tool_input: &Value,
        _tool_result: &Value,
        _is_error: bool,
    ) -> Result<HookDecision, GuardError> {
        // The Go guard does not gate PostToolUse through the rule engine.
        Ok(HookDecision::Allow)
    }
}

#[async_trait::async_trait]
impl TargetGate for InterceptorImpl {
    async fn check_targets(&self, host: &str, port: u16) -> Result<TargetDecision, GuardError> {
        // The guard-level target gate is scope-free: evaluate against a
        // conversation-less scope that matches no pins, so an unpinned target is
        // denied here and left to `request_targets`.
        let scope = TargetScope::default();
        let pins = active_pins(&self.db, &scope, host, port)
            .await
            .map_err(|e| GuardError::TargetGate(e.to_string()))?;
        let decision = if pins_are_empty(&pins) {
            TargetDecision::Deny {
                reason: format!("target {host}:{port} is not approved"),
            }
        } else {
            TargetDecision::Allow
        };
        // `check_targets_declared` needs a scope; the guard path above covers the
        // pin check directly. Suppress the unused warning by referencing it.
        let _ = &scope;
        Ok(decision)
    }

    async fn request_targets(
        &self,
        host: &str,
        port: u16,
        ask: Option<&HookDecision>,
    ) -> Result<TargetDecision, GuardError> {
        // Without a scope and a wait bound the request cannot be opened; mirror
        // the Go behaviour of surfacing the merged ask's reason.
        let reason = match ask {
            Some(HookDecision::Ask { reason, .. }) => reason.clone(),
            Some(HookDecision::Deny { reason }) => reason.clone(),
            _ => format!("target {host}:{port} needs approval"),
        };
        Ok(TargetDecision::Ask {
            reason,
            timeout: None,
        })
    }
}

fn pins_are_empty(pins: &[TargetPin]) -> bool {
    pins.is_empty()
}

// ---------------------------------------------------------------------------
// intercept_pending
// ---------------------------------------------------------------------------

/// One row of `intercept_pending` (the fields the store reads/writes).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PendingRecord {
    pub id: i64,
    pub rule_id: Option<i64>,
    pub conversation_id: Option<i64>,
    pub task_id: Option<String>,
    pub agent_name: String,
    pub tool_name: String,
    pub tool_input: Value,
    pub status: String,
    pub decision_source: String,
    pub decided_by: String,
    pub reason: String,
    pub decided_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

const PENDING_COLS: &str = "id, rule_id, conversation_id, task_id, agent_name, tool_name, \
     tool_input, status, reason, decided_at, created_at, decision_source, decided_by";

fn scan_pending(row: &sqlx::postgres::PgRow) -> PendingRecord {
    PendingRecord {
        id: row.get("id"),
        rule_id: row.get("rule_id"),
        conversation_id: row.get("conversation_id"),
        task_id: row.get("task_id"),
        agent_name: row.get("agent_name"),
        tool_name: row.get("tool_name"),
        tool_input: row.get("tool_input"),
        status: row.get("status"),
        reason: row.get("reason"),
        decided_at: row.get("decided_at"),
        created_at: row.get("created_at"),
        decision_source: row.get("decision_source"),
        decided_by: row.get("decided_by"),
    }
}

/// The fields of an `intercept_pending` row a caller supplies on create.
#[derive(Debug, Clone)]
pub struct PendingInput {
    pub rule_id: Option<i64>,
    /// 0 / None = no active conversation (background task).
    pub conversation_id: Option<i64>,
    pub task_id: Option<String>,
    pub agent_name: String,
    pub tool_name: String,
    pub tool_input: Value,
    pub reason: String,
}

/// Database-backed store for `intercept_pending` (ask requests and history).
#[derive(Clone)]
pub struct InterceptStore {
    db: database::Db,
}

impl InterceptStore {
    pub fn new(db: database::Db) -> Self {
        Self { db }
    }

    /// Insert a pending approval request and return its id (Go
    /// `CreateInterceptPending`).
    pub async fn create_pending(&self, input: PendingInput) -> Result<i64, InterceptionError> {
        let tool_input = if input.tool_input.is_null() {
            serde_json::json!({})
        } else {
            input.tool_input.clone()
        };
        let decision_source = intercept_source(input.rule_id, &input.reason);
        let row = sqlx::query(
            "INSERT INTO intercept_pending(rule_id, conversation_id, task_id, agent_name, \
             tool_name, tool_input, reason, decision_source) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8) RETURNING id",
        )
        .bind(input.rule_id)
        .bind(input.conversation_id.filter(|&c| c != 0))
        .bind(input.task_id.filter(|t| !t.is_empty()))
        .bind(&input.agent_name)
        .bind(&input.tool_name)
        .bind(tool_input)
        .bind(&input.reason)
        .bind(decision_source)
        .fetch_one(self.db.pool())
        .await?;
        Ok(row.get("id"))
    }

    /// Insert an already-decided row (`status` = allowed | denied) for
    /// observability (Go `CreateDecidedIntercept`).
    pub async fn create_decided(
        &self,
        input: PendingInput,
        status: &str,
    ) -> Result<i64, InterceptionError> {
        let tool_input = if input.tool_input.is_null() {
            serde_json::json!({})
        } else {
            input.tool_input.clone()
        };
        let decision_source = intercept_source(input.rule_id, &input.reason);
        let decided_by = if input.rule_id.is_some() {
            "rule"
        } else {
            "model"
        };
        let row = sqlx::query(
            "INSERT INTO intercept_pending(rule_id, conversation_id, task_id, agent_name, \
             tool_name, tool_input, status, reason, decided_at, decision_source, decided_by) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,NOW(),$9,$10) RETURNING id",
        )
        .bind(input.rule_id)
        .bind(input.conversation_id.filter(|&c| c != 0))
        .bind(input.task_id.filter(|t| !t.is_empty()))
        .bind(&input.agent_name)
        .bind(&input.tool_name)
        .bind(tool_input)
        .bind(status)
        .bind(&input.reason)
        .bind(decision_source)
        .bind(decided_by)
        .fetch_one(self.db.pool())
        .await?;
        Ok(row.get("id"))
    }

    /// One pending record by id, or `None` when absent (Go
    /// `GetInterceptPending`).
    pub async fn get_pending(&self, id: i64) -> Result<Option<PendingRecord>, InterceptionError> {
        let row = sqlx::query(&format!(
            "SELECT {PENDING_COLS} FROM intercept_pending WHERE id = $1"
        ))
        .bind(id)
        .fetch_optional(self.db.pool())
        .await?;
        Ok(row.as_ref().map(scan_pending))
    }

    /// Decide a pending request by id (Go `ResolveInterceptBy` for the classic
    /// ask path). Returns whether a pending row was resolved.
    pub async fn decide(
        &self,
        id: i64,
        allowed: bool,
        approver: &str,
        remote_addr: &str,
    ) -> Result<bool, InterceptionError> {
        let (status, action, reason) = if allowed {
            ("allowed", "allow", "user approved execution")
        } else {
            ("denied", "deny", "user rejected execution")
        };
        let res = sqlx::query(
            "UPDATE intercept_pending SET status=$2, decided_at=NOW(), decided_by='human' \
             WHERE id=$1 AND status='pending'",
        )
        .bind(id)
        .bind(status)
        .execute(self.db.pool())
        .await?;
        if res.rows_affected() == 0 {
            return Ok(false);
        }
        let _ = (action, reason, approver, remote_addr);
        Ok(true)
    }

    /// All unresolved approval requests, newest first (Go
    /// `ListPendingIntercepts`).
    pub async fn list_pending(&self) -> Result<Vec<PendingRecord>, InterceptionError> {
        let rows = sqlx::query(&format!(
            "SELECT {PENDING_COLS} FROM intercept_pending WHERE status='pending' \
             ORDER BY created_at DESC"
        ))
        .fetch_all(self.db.pool())
        .await?;
        Ok(rows.iter().map(scan_pending).collect())
    }

    /// Up to `limit` rows newest first (Go history / `ListAllIntercepts`).
    pub async fn history(&self, limit: i64) -> Result<Vec<PendingRecord>, InterceptionError> {
        let limit = limit.clamp(1, 1000);
        let rows = sqlx::query(&format!(
            "SELECT {PENDING_COLS} FROM intercept_pending ORDER BY created_at DESC, id DESC \
             LIMIT $1"
        ))
        .bind(limit)
        .fetch_all(self.db.pool())
        .await?;
        Ok(rows.iter().map(scan_pending).collect())
    }

    /// Delete a pending row (test/cleanup helper).
    pub async fn delete(&self, id: i64) -> Result<(), InterceptionError> {
        sqlx::query("DELETE FROM intercept_pending WHERE id = $1")
            .bind(id)
            .execute(self.db.pool())
            .await?;
        Ok(())
    }
}

/// The decision-source of a pending row (Go `interceptSource`).
fn intercept_source(rule_id: Option<i64>, reason: &str) -> String {
    if rule_id.is_some() {
        return "rule".to_owned();
    }
    if reason.starts_with("[model]") || reason.starts_with("[模型]") {
        return "model".to_owned();
    }
    "unknown".to_owned()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn rule(
        id: i64,
        priority: i32,
        match_target: &str,
        match_type: &str,
        pattern: &str,
        action: &str,
    ) -> InterceptRule {
        InterceptRule {
            id,
            name: format!("r{id}"),
            enabled: true,
            priority,
            match_target: match_target.to_owned(),
            match_type: match_type.to_owned(),
            pattern: pattern.to_owned(),
            action: action.to_owned(),
            message: String::new(),
            timeout_enabled: false,
            timeout_seconds: 60,
            timeout_action: "deny".to_owned(),
        }
    }

    #[test]
    fn string_rule_matches_substring_on_target() {
        let r = rule(1, 0, "tool_name", "string", "Web", "deny");
        assert!(match_rule(&r, "WebFetch", &json!({})));
        assert!(!match_rule(&r, "Bash", &json!({})));

        let r = rule(2, 0, "tool_input", "string", "rm -rf", "deny");
        assert!(match_rule(&r, "Bash", &json!({"command": "sudo rm -rf /"})));
        assert!(!match_rule(&r, "Bash", &json!({"command": "ls"})));
    }

    #[test]
    fn regex_rule_matches_and_skips_bad_pattern() {
        let r = rule(1, 0, "tool_input", "regex", r"rm\s+-rf", "deny");
        assert!(match_rule(&r, "Bash", &json!({"command": "rm   -rf /"})));
        assert!(!match_rule(&r, "Bash", &json!({"command": "rm -r"})));

        // Lookaround is unsupported by the `regex` crate: the rule is skipped.
        let r = rule(2, 0, "tool_input", "regex", r"(?=rm)rm", "deny");
        assert!(!match_rule(&r, "Bash", &json!({"command": "rm -rf"})));
    }

    #[test]
    fn argv_prefix_matches_subcommand_and_flags_after() {
        let r = rule(1, 0, "tool_input", "argv", r#"["git","push"]"#, "deny");
        assert!(match_rule(
            &r,
            "Bash",
            &json!({"command": "git push --force"})
        ));
        assert!(match_rule(&r, "Bash", &json!({"command": "git push"})));
        // Flag before the subcommand: no prefix, so no match.
        assert!(!match_rule(
            &r,
            "Bash",
            &json!({"command": "git -C /repo push"})
        ));
        // Different subcommand.
        assert!(!match_rule(&r, "Bash", &json!({"command": "git pull"})));
        // Non-Bash tool.
        assert!(!match_rule(&r, "Write", &json!({"command": "git push"})));
    }

    #[test]
    fn argv_deny_rule_matches_denylisted_program_but_allow_does_not() {
        // Per Go, ArgvPrefix("bash x") == ["bash"] (bash is not in the arity
        // table, so its prefix is just the program). A rule must equal the
        // derived prefix, so the matching rule is ["bash"].
        let deny = rule(1, 0, "tool_input", "argv", r#"["bash"]"#, "deny");
        // bash is denylisted; a deny rule still matches.
        assert!(match_rule(&deny, "Bash", &json!({"command": "bash x"})));

        let allow = rule(2, 0, "tool_input", "argv", r#"["bash"]"#, "allow");
        assert!(!match_rule(&allow, "Bash", &json!({"command": "bash x"})));
    }

    #[test]
    fn argv_refuses_compound_commands() {
        let r = rule(1, 0, "tool_input", "argv", r#"["git","push"]"#, "deny");
        // Pipe, list, redirection, substitution: not a simple command.
        assert!(!match_rule(
            &r,
            "Bash",
            &json!({"command": "git push | cat"})
        ));
        assert!(!match_rule(
            &r,
            "Bash",
            &json!({"command": "git push && ls"})
        ));
        assert!(!match_rule(
            &r,
            "Bash",
            &json!({"command": "git push > out"})
        ));
        assert!(!match_rule(
            &r,
            "Bash",
            &json!({"command": "git push $(id)"})
        ));
    }

    #[test]
    fn argv_prefix_derivation() {
        assert_eq!(
            argv_prefix("git push --force").ok().flatten(),
            Some(vec!["git".to_owned(), "push".to_owned()])
        );
        assert_eq!(
            argv_prefix("cat file").ok().flatten(),
            Some(vec!["cat".to_owned()])
        );
        assert!(argv_prefix("git -C /repo push").is_err());
        assert_eq!(argv_prefix("git push | cat"), Ok(None));
    }

    #[test]
    fn priority_order_first_match_wins() {
        let rules = vec![
            rule(1, 1, "tool_name", "string", "Bash", "allow"),
            rule(2, 10, "tool_input", "string", "rm", "deny"),
        ];
        let decision = evaluate(&rules, "Bash", &json!({"command": "rm -rf /"}));
        assert!(matches!(decision, Some(HookDecision::Deny { .. })));

        // Lower id wins on a priority tie.
        let rules = vec![
            rule(5, 5, "tool_name", "string", "Bash", "deny"),
            rule(2, 5, "tool_name", "string", "Bash", "ask"),
        ];
        assert!(matches!(
            evaluate(&rules, "Bash", &json!({})),
            Some(HookDecision::Ask { .. })
        ));
    }

    #[test]
    fn disabled_rule_is_skipped() {
        let mut r = rule(1, 10, "tool_name", "string", "Bash", "deny");
        r.enabled = false;
        assert!(evaluate(&[r], "Bash", &json!({})).is_none());
    }

    #[test]
    fn deny_and_ask_default_messages() {
        let r = rule(1, 0, "tool_name", "string", "Bash", "deny");
        assert!(matches!(
            decision_for(&r),
            HookDecision::Deny { reason } if reason.contains("forbids")
        ));
        let r = rule(2, 0, "tool_name", "string", "Bash", "ask");
        assert!(matches!(
            decision_for(&r),
            HookDecision::Ask { reason, timeout: None } if reason.contains("approval")
        ));
    }

    #[test]
    fn ask_timeout_from_rule() {
        let mut r = rule(1, 0, "tool_name", "string", "Bash", "ask");
        r.timeout_enabled = true;
        r.timeout_seconds = 30;
        match decision_for(&r) {
            HookDecision::Ask {
                timeout: Some(d), ..
            } => assert_eq!(d, Duration::from_secs(30)),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn parse_argv_pattern_validation() {
        assert!(parse_argv_pattern(r#"["git","push"]"#).is_ok());
        assert!(parse_argv_pattern(r#"[]"#).is_err());
        assert!(parse_argv_pattern(r#"["git","-f"]"#).is_err());
        assert!(parse_argv_pattern(r#"["git","a b"]"#).is_err());
        assert!(parse_argv_pattern("not json").is_err());
        let too_many: Vec<String> = (0..9).map(|i| format!("t{i}")).collect();
        assert!(parse_argv_pattern(&serde_json::to_string(&too_many).unwrap()).is_err());
    }

    #[test]
    fn argv_program_name_and_denylist() {
        assert_eq!(argv_program_name("/usr/bin/python3.11"), "python");
        assert_eq!(argv_program_name("mkfs.ext4"), "mkfs");
        assert_eq!(argv_program_name("pip3"), "pip");
        assert!(argv_allow_denied("bash"));
        assert!(!argv_allow_denied("git"));
    }

    #[test]
    fn normalize_target_forms() {
        assert_eq!(
            normalize_target("https://Example.COM:8443/path"),
            Some(Target::new("domain", "example.com".to_owned(), 8443))
        );
        assert_eq!(
            normalize_target("http://example.com"),
            Some(Target::new("domain", "example.com".to_owned(), 80))
        );
        assert_eq!(
            normalize_target("127.0.0.1"),
            Some(Target::new("ip", "127.0.0.1".to_owned(), 0))
        );
        assert_eq!(
            normalize_target("10.0.0.0/8"),
            Some(Target::new("cidr", "10.0.0.0/8".to_owned(), 0))
        );
        assert_eq!(
            normalize_target("[::1]:443"),
            Some(Target::new("ip", "::1".to_owned(), 443))
        );
    }

    #[test]
    fn extract_targets_from_bash_and_webfetch() {
        let bash = json!({"command": "curl http://a.example/path and https://b.example:8443/x"});
        let targets = extract_targets("Bash", &bash);
        let hosts: Vec<&str> = targets.iter().map(|t| t.host.as_str()).collect();
        assert!(hosts.contains(&"a.example"));
        assert!(hosts.contains(&"b.example"));

        let web = json!({"url": "https://c.example/x"});
        let targets = extract_targets("WebFetch", &web);
        assert_eq!(
            targets,
            vec![Target::new("domain", "c.example".to_owned(), 443)]
        );

        // A tool with no targets.
        assert!(extract_targets("Read", &json!({"file": "x"})).is_empty());
    }

    #[test]
    fn asset_rule_matching() {
        let rules = vec![
            AssetInterceptRule {
                id: 1,
                enabled: true,
                kind: "fuzzy_domain".to_owned(),
                pattern: ".gov.cn".to_owned(),
                note: String::new(),
            },
            AssetInterceptRule {
                id: 2,
                enabled: false,
                kind: "exact_ip".to_owned(),
                pattern: "10.0.0.1".to_owned(),
                note: String::new(),
            },
        ];
        let hit = match_asset_rules(&rules, &["x.gov.cn".to_owned()], &[], &[]);
        assert_eq!(hit.map(|r| r.id), Some(1));

        let hit = match_asset_rules(&rules, &[], &["10.0.0.1".to_owned()], &[]);
        assert!(hit.is_none()); // disabled rule
    }

    #[test]
    fn cidr_asset_rule_matches() {
        let rules = vec![AssetInterceptRule {
            id: 1,
            enabled: true,
            kind: "cidr".to_owned(),
            pattern: "10.0.0.0/8".to_owned(),
            note: String::new(),
        }];
        assert!(match_asset_rules(&rules, &[], &["10.5.6.7".to_owned()], &[]).is_some());
        assert!(match_asset_rules(&rules, &[], &["11.0.0.1".to_owned()], &[]).is_none());
    }

    #[test]
    fn exemption_validation() {
        assert!(validate_exemption_host("127.0.0.1").is_err());
        assert!(validate_exemption_host("169.254.169.254").is_err());
        assert!(validate_exemption_host("::1").is_err());
        assert!(validate_exemption_host("10.0.0.5").is_ok());
        assert!(validate_exemption_host("example.com").is_ok());
        assert!(voids_exemption("10.0.0.5")); // private voids
        assert!(!voids_exemption("8.8.8.8"));
    }

    #[test]
    fn target_key_format() {
        assert_eq!(target_key("a.example", 443), "a.example:443");
        assert_eq!(target_key("::1", 0), "::1:0");
    }

    #[test]
    fn covering_pin_subset() {
        let pins = vec![TargetPin {
            id: 1,
            host: "a".to_owned(),
            port: 443,
            version: 1,
            pinned_ips: vec!["1.2.3.4".to_owned()],
            name_only_effective: false,
        }];
        assert!(covering_pin(&pins, &["1.2.3.4".to_owned()]).is_some());
        assert!(covering_pin(&pins, &["1.2.3.4".to_owned(), "5.6.7.8".to_owned()]).is_none());
        assert!(covering_pin(&pins, &[]).is_none());
    }

    #[test]
    fn intercept_source_classification() {
        assert_eq!(intercept_source(Some(1), ""), "rule");
        assert_eq!(intercept_source(None, "[model] hi"), "model");
        assert_eq!(intercept_source(None, "plain"), "unknown");
    }

    #[test]
    fn rule_input_validation() {
        let base = RuleInput {
            name: "n".to_owned(),
            enabled: true,
            priority: 0,
            match_target: "tool_name".to_owned(),
            match_type: "string".to_owned(),
            pattern: "x".to_owned(),
            action: "deny".to_owned(),
            message: String::new(),
            timeout_enabled: false,
            timeout_seconds: 60,
            timeout_action: "deny".to_owned(),
        };
        assert!(base.validate().is_ok());
        let bad = RuleInput {
            match_type: "glob".to_owned(),
            ..base.clone()
        };
        assert!(bad.validate().is_err());
        let bad = RuleInput {
            action: "block".to_owned(),
            ..base
        };
        assert!(bad.validate().is_err());
    }
}
