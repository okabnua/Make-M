//! App settings + stats — port of ARTEX `db/settings.go`, the settings data
//! layer of `db/config.go`, and the `stats()` / `getSettings()` data reads in
//! `server/server.go` / `server/manager.go`.
//!
//! The Go original stores runtime settings in a tiny `settings(key, value,
//! updated_at)` table. Secret settings (`db/secrets.go` `secretSettingKeys`)
//! are sealed at rest with AES-256-GCM; at-rest encryption is a later migration
//! step, so this crate stores secrets as plaintext and marks them via
//! [`SettingsStore::set_secret`]. Reads never expose secret values through
//! [`SettingsStore::list_all`].
//!
//! Behavior-preserving notes:
//! - Boolean parsing matches Go `GetBool`: `"true"` or `"1"` is true, anything
//!   else is false; unset/unparseable returns the caller's default.
//! - Typed getters reproduce the exact keys and defaults of the Go handlers
//!   (`server/manager.go`, `server/constraints_api.go`, `server/notifier.go`,
//!   `server/customtool.go`).
//! - `stats()` reproduces the global fields of `server/server.go` `stats()` that
//!   live in the database. Live engine state (engine_mode/active task) is owned
//!   by the (not-yet-ported) server engine layer; `engine_mode` is the `idle`
//!   placeholder and `traffic` is `None` (traffic store not ported yet).

use database::Db;
use serde::{Deserialize, Serialize};
use sqlx::Row;
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// Setting keys (exact strings from the Go sources)
// ---------------------------------------------------------------------------

// server/manager.go
/// `settingTrafficCapture` — recording-proxy capture toggle (default off).
pub const KEY_TRAFFIC_CAPTURE: &str = "traffic_capture";
/// `settingAgentTrafficBinding` — bind findings to captured traffic (default off).
pub const KEY_AGENT_TRAFFIC_BINDING: &str = "agent_traffic_binding";
/// `settingWebSearchOn` — web-search tool toggle (default off).
pub const KEY_WEB_SEARCH_ENABLED: &str = "web_search_enabled";
/// `settingWebSearchBackend` — web-search backend (default `ddgs`).
pub const KEY_WEB_SEARCH_BACKEND: &str = "web_search_backend";
/// `settingBraveKey` — Brave Search API key (secret).
pub const KEY_BRAVE_API_KEY: &str = "brave_search_api_key";
/// `settingTavilyKey` — Tavily Search API key (secret).
pub const KEY_TAVILY_API_KEY: &str = "tavily_search_api_key";
/// `settingWebSearchProxy` — dedicated web-search egress proxy (secret).
pub const KEY_WEB_SEARCH_PROXY: &str = "web_search_proxy";
/// `settingGlobalProxy` — global egress proxy (secret).
pub const KEY_GLOBAL_PROXY: &str = "global_proxy";
/// `settingWorkers` — concurrent worker-agent count (default 3).
pub const KEY_WORKERS: &str = "workers";
/// `settingLLMRecord` — LLM request/response recording toggle (default off).
pub const KEY_LLM_RECORD: &str = "llm_record";
/// `settingLLMPoolOn` — LLM failover pool toggle (default off).
pub const KEY_LLM_POOL_ENABLED: &str = "llm_pool_enabled";
/// `settingLLMPoolBindFallback` — bound-profile failover toggle (default off).
pub const KEY_LLM_POOL_BIND_FALLBACK: &str = "llm_pool_bind_fallback";
/// `settingConcurrencyOn` — simultaneous-running-task cap switch (default off).
pub const KEY_TASK_CONCURRENCY_ENABLED: &str = "task_concurrency_enabled";
/// `settingConcurrencyLimit` — simultaneous-running-task cap (default 5).
pub const KEY_TASK_CONCURRENCY_LIMIT: &str = "task_concurrency_limit";
/// `settingNoaCompaction` — experimental noa compaction toggle (default off).
pub const KEY_NOA_COMPACTION: &str = "noa_compaction";

// server/customtool.go
/// `settingPythonInterp` — python interpreter path for the custom script tool.
pub const KEY_PYTHON_INTERP: &str = "python_interpreter";

// server/constraints_api.go
/// `settingConstraintsInjectPlanner` — inject constraints into planner (default on).
pub const KEY_CONSTRAINTS_INJECT_PLANNER: &str = "constraints_inject_planner";
/// `settingConstraintsInjectWorker` — inject constraints into worker (default on).
pub const KEY_CONSTRAINTS_INJECT_WORKER: &str = "constraints_inject_worker";

// server/notifier.go
/// `settingNotifyEnabled` — IM push master switch (default on).
pub const KEY_NOTIFY_ENABLED: &str = "notify_enabled";
/// `settingNotifyPublicBaseURL` — external base URL for backlinks (default empty).
pub const KEY_NOTIFY_PUBLIC_BASE_URL: &str = "notify_public_base_url";
/// `settingNotifyDigestMinutes` — digest-mode period in minutes (default 30).
pub const KEY_NOTIFY_DIGEST_MINUTES: &str = "notify_digest_interval_min";
/// `settingNotifySummaryMaxRunes` — summary character cap, 0 = unlimited (default 120).
pub const KEY_NOTIFY_SUMMARY_MAX_RUNES: &str = "notify_summary_max_runes";
/// `settingNotifyAssetsMaxShown` — asset-name list cap, 0 = unlimited (default 3).
pub const KEY_NOTIFY_ASSETS_MAX_SHOWN: &str = "notify_assets_max_shown";

/// Frontend chat input send binding. In the Go original this is a pure
/// client-side localStorage preference (`artex_chat_send_mode`,
/// web/src/lib/chat-send-mode.ts) and is never stored in the database; this
/// key/default exist so the API layer has a typed accessor with the same shape.
pub const KEY_CHAT_SEND_MODE: &str = "artex_chat_send_mode";

// Defaults ported verbatim from server/manager.go / notifier.go / customtool.go.
const DEFAULT_WORKERS: i64 = 3;
const DEFAULT_WEB_SEARCH_BACKEND: &str = "ddgs";
const DEFAULT_CONCURRENCY_LIMIT: i64 = 5;
const DEFAULT_NOTIFY_DIGEST_MINUTES: i64 = 30;
const DEFAULT_NOTIFY_SUMMARY_MAX_RUNES: i64 = 120;
const DEFAULT_NOTIFY_ASSETS_MAX_SHOWN: i64 = 3;
const NOTIFY_SUMMARY_MAX_RUNES_CAP: i64 = 4000;
const NOTIFY_ASSETS_MAX_SHOWN_CAP: i64 = 50;
const DEFAULT_CHAT_SEND_MODE: &str = "enter";

/// Settings-keys that are stored encrypted in the Go original
/// (`db/secrets.go` `secretSettingKeys`): `brave_search_api_key`,
/// `tavily_search_api_key`, `web_search_proxy`, `global_proxy`. Plus
/// `tools.exec` (a secret *column* of the tools table, never a settings row, but
/// excluded defensively per the migration spec).
pub const SECRET_SETTING_KEYS: &[&str] = &[
    KEY_BRAVE_API_KEY,
    KEY_TAVILY_API_KEY,
    KEY_WEB_SEARCH_PROXY,
    KEY_GLOBAL_PROXY,
    "tools.exec",
    // Never expose the admin password hash (bcrypt) via GET /api/settings.
    "auth.password_hash",
];

/// True when `key` holds a secret that `list_all` must never return.
///
/// Mirrors `secretSettingKeys` plus `tools.exec`. The suffix checks also cover
/// any future `<name>_api_key` / `<name>_proxy` setting, matching the migration
/// spec's "never return secret keys like `*_api_key`, `*_proxy`".
pub fn is_secret_key(key: &str) -> bool {
    SECRET_SETTING_KEYS.contains(&key) || key.ends_with("_api_key") || key.ends_with("_proxy")
}

// ---------------------------------------------------------------------------
// Pure parsing helpers (ported from the Go getters)
// ---------------------------------------------------------------------------

/// Go `GetBool`: `"true"` or `"1"` is true; anything else is false.
fn bool_value(v: &str) -> bool {
    v == "true" || v == "1"
}

/// Positive integer with fallback: used by `workers` and `notify_digest_interval_min`
/// (Go rejects unparseable or `<= 0` and returns the default).
fn parse_positive(v: &str, default: i64) -> i64 {
    match v.trim().parse::<i64>() {
        Ok(n) if n > 0 => n,
        _ => default,
    }
}

/// Bounded integer with fallback: used by the notify render limits. `0` is a
/// valid value (no truncation); values outside `[0, max]` or unparseable fall
/// back to the default (Go `notifyLimitValue`).
fn parse_bounded(v: &str, default: i64, max: i64) -> i64 {
    match v.trim().parse::<i64>() {
        Ok(n) if (0..=max).contains(&n) => n,
        _ => default,
    }
}

/// Parse a concurrency limit: Go's `ConcurrencyLimit` reads `settingConcurrencyOn`
/// (`trim == "true"`); when on, a stored limit `>= 1` wins, else the default.
fn parse_concurrency(on_raw: Option<String>, limit_raw: Option<String>) -> (bool, i64) {
    let on = on_raw.map(|v| v.trim() == "true").unwrap_or(false);
    if !on {
        return (false, 0);
    }
    let limit = match limit_raw {
        Some(v) => match v.trim().parse::<i64>() {
            Ok(n) if n >= 1 => n,
            _ => DEFAULT_CONCURRENCY_LIMIT,
        },
        None => DEFAULT_CONCURRENCY_LIMIT,
    };
    (true, limit)
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error("sql: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("invalid setting value: {0}")]
    Invalid(String),
}

// ---------------------------------------------------------------------------
// Settings facade
// ---------------------------------------------------------------------------

/// Typed settings facade over a [`database::Db`].
#[derive(Clone)]
pub struct SettingsStore {
    db: Db,
}

impl SettingsStore {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    /// The underlying database handle.
    pub fn db(&self) -> &Db {
        &self.db
    }

    // -- generic key/value --------------------------------------------------

    /// Get a setting value; `None` when the key is unset.
    ///
    /// Secret settings are returned as stored. At-rest encryption is not yet
    /// ported (see [`SettingsStore::set_secret`]), so a stored secret is
    /// plaintext here; once `db/secrets.go` is ported this must decrypt.
    pub async fn get(&self, key: &str) -> Result<Option<String>, SettingsError> {
        let row = sqlx::query("SELECT value FROM settings WHERE key = $1")
            .bind(key)
            .fetch_optional(self.db.pool())
            .await?;
        Ok(row.map(|r| r.get::<String, _>("value")))
    }

    /// Upsert a setting value.
    pub async fn set(&self, key: &str, value: &str) -> Result<(), SettingsError> {
        sqlx::query(
            "INSERT INTO settings(key, value) VALUES ($1, $2) \
             ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value, updated_at = now()",
        )
        .bind(key)
        .bind(value)
        .execute(self.db.pool())
        .await?;
        Ok(())
    }

    /// Get a boolean setting; `default` when unset or unparseable.
    pub async fn get_bool(&self, key: &str, default: bool) -> Result<bool, SettingsError> {
        match self.get(key).await? {
            Some(v) => Ok(bool_value(&v)),
            None => Ok(default),
        }
    }

    /// Store a boolean setting as `"true"` / `"false"`.
    pub async fn set_bool(&self, key: &str, value: bool) -> Result<(), SettingsError> {
        self.set(key, if value { "true" } else { "false" }).await
    }

    /// Return every non-secret setting as a sorted `key -> value` map.
    ///
    /// This is the data behind `GET /api/settings`: secret keys are never
    /// returned (see [`is_secret_key`]), matching the Go secret-setting policy.
    pub async fn list_all(&self) -> Result<BTreeMap<String, String>, SettingsError> {
        let rows = sqlx::query("SELECT key, value FROM settings")
            .fetch_all(self.db.pool())
            .await?;
        let mut out = BTreeMap::new();
        for r in rows {
            let key: String = r.get("key");
            if is_secret_key(&key) {
                continue;
            }
            let value: String = r.get("value");
            out.insert(key, value);
        }
        Ok(out)
    }

    /// Set a secret setting (e.g. `brave_search_api_key`, `web_search_proxy`).
    ///
    /// Secret settings are encrypted at rest in the Go original: AES-256-GCM
    /// with a per-value nonce and AAD `"settings.<key>"`, stored as
    /// `enc:v1:<base64(nonce||ciphertext)>` (`ARTEX/db/secrets.go`,
    /// `sealSetting`/`openSecret`).
    ///
    /// TODO(secrets): encryption lives in a later secrets module. Until
    /// `db/secrets.go` is ported this stores the value as plaintext. Do not
    /// rely on `get`/`list_all` to redact it — `list_all` already excludes the
    /// key, but `get` returns the stored (plaintext) value.
    pub async fn set_secret(&self, key: &str, value: &str) -> Result<(), SettingsError> {
        self.set(key, value).await
    }

    // -- typed getters (ported from server/manager.go & friends) ------------

    /// `traffic_capture` (default off).
    pub async fn traffic_capture_enabled(&self) -> Result<bool, SettingsError> {
        self.get_bool(KEY_TRAFFIC_CAPTURE, false).await
    }

    /// `agent_traffic_binding` (default off).
    pub async fn agent_traffic_binding(&self) -> Result<bool, SettingsError> {
        self.get_bool(KEY_AGENT_TRAFFIC_BINDING, false).await
    }

    /// `llm_record` (default off).
    pub async fn llm_record_enabled(&self) -> Result<bool, SettingsError> {
        self.get_bool(KEY_LLM_RECORD, false).await
    }

    /// `web_search_enabled` (default off).
    pub async fn web_search_enabled(&self) -> Result<bool, SettingsError> {
        self.get_bool(KEY_WEB_SEARCH_ENABLED, false).await
    }

    /// `web_search_backend` (default `ddgs`).
    pub async fn web_search_backend(&self) -> Result<String, SettingsError> {
        match self.get(KEY_WEB_SEARCH_BACKEND).await? {
            Some(v) if !v.is_empty() => Ok(v),
            _ => Ok(DEFAULT_WEB_SEARCH_BACKEND.to_string()),
        }
    }

    /// `workers` — concurrent worker-agent count (default 3; unparseable/`<=0`
    /// falls back to 3, matching `Manager.Workers`).
    pub async fn worker_concurrency(&self) -> Result<i64, SettingsError> {
        match self.get(KEY_WORKERS).await? {
            Some(v) => Ok(parse_positive(&v, DEFAULT_WORKERS)),
            None => Ok(DEFAULT_WORKERS),
        }
    }

    /// `task_concurrency_enabled` / `task_concurrency_limit` as `(enabled, limit)`.
    ///
    /// Port of `Manager.ConcurrencyLimit`: when off, `(false, 0)`; when on, the
    /// stored limit `>= 1` or the default `5`.
    pub async fn concurrency_limit(&self) -> Result<(bool, i64), SettingsError> {
        let on_raw = self.get(KEY_TASK_CONCURRENCY_ENABLED).await?;
        let limit_raw = self.get(KEY_TASK_CONCURRENCY_LIMIT).await?;
        Ok(parse_concurrency(on_raw, limit_raw))
    }

    /// `llm_pool_enabled` (default off).
    pub async fn llm_pool_enabled(&self) -> Result<bool, SettingsError> {
        self.get_bool(KEY_LLM_POOL_ENABLED, false).await
    }

    /// `llm_pool_bind_fallback` (default off).
    pub async fn llm_pool_bind_fallback(&self) -> Result<bool, SettingsError> {
        self.get_bool(KEY_LLM_POOL_BIND_FALLBACK, false).await
    }

    /// `noa_compaction` (default off).
    pub async fn noa_compaction_enabled(&self) -> Result<bool, SettingsError> {
        self.get_bool(KEY_NOA_COMPACTION, false).await
    }

    /// `constraints_inject_planner` (default on).
    pub async fn constraints_inject_planner(&self) -> Result<bool, SettingsError> {
        self.get_bool(KEY_CONSTRAINTS_INJECT_PLANNER, true).await
    }

    /// `constraints_inject_worker` (default on).
    pub async fn constraints_inject_worker(&self) -> Result<bool, SettingsError> {
        self.get_bool(KEY_CONSTRAINTS_INJECT_WORKER, true).await
    }

    /// `notify_enabled` (default on).
    pub async fn notify_enabled(&self) -> Result<bool, SettingsError> {
        self.get_bool(KEY_NOTIFY_ENABLED, true).await
    }

    /// `notify_public_base_url` (default empty).
    pub async fn notify_public_base_url(&self) -> Result<String, SettingsError> {
        Ok(self
            .get(KEY_NOTIFY_PUBLIC_BASE_URL)
            .await?
            .unwrap_or_default())
    }

    /// `notify_digest_interval_min` (default 30; unparseable/`<=0` → 30).
    pub async fn notify_digest_interval_min(&self) -> Result<i64, SettingsError> {
        match self.get(KEY_NOTIFY_DIGEST_MINUTES).await? {
            Some(v) => Ok(parse_positive(&v, DEFAULT_NOTIFY_DIGEST_MINUTES)),
            None => Ok(DEFAULT_NOTIFY_DIGEST_MINUTES),
        }
    }

    /// `notify_summary_max_runes` (default 120; unparseable/outside `[0,4000]` → 120).
    pub async fn notify_summary_max_runes(&self) -> Result<i64, SettingsError> {
        match self.get(KEY_NOTIFY_SUMMARY_MAX_RUNES).await? {
            Some(v) => Ok(parse_bounded(
                &v,
                DEFAULT_NOTIFY_SUMMARY_MAX_RUNES,
                NOTIFY_SUMMARY_MAX_RUNES_CAP,
            )),
            None => Ok(DEFAULT_NOTIFY_SUMMARY_MAX_RUNES),
        }
    }

    /// `notify_assets_max_shown` (default 3; unparseable/outside `[0,50]` → 3).
    pub async fn notify_assets_max_shown(&self) -> Result<i64, SettingsError> {
        match self.get(KEY_NOTIFY_ASSETS_MAX_SHOWN).await? {
            Some(v) => Ok(parse_bounded(
                &v,
                DEFAULT_NOTIFY_ASSETS_MAX_SHOWN,
                NOTIFY_ASSETS_MAX_SHOWN_CAP,
            )),
            None => Ok(DEFAULT_NOTIFY_ASSETS_MAX_SHOWN),
        }
    }

    /// `python_interpreter` (trimmed; empty = use runtime detection).
    pub async fn python_interpreter(&self) -> Result<String, SettingsError> {
        Ok(self
            .get(KEY_PYTHON_INTERP)
            .await?
            .unwrap_or_default()
            .trim()
            .to_string())
    }

    /// `web_search_proxy` (secret; empty = direct). Stored plaintext for now.
    pub async fn web_search_proxy(&self) -> Result<String, SettingsError> {
        Ok(self.get(KEY_WEB_SEARCH_PROXY).await?.unwrap_or_default())
    }

    /// `global_proxy` (secret; empty = direct). Stored plaintext for now.
    pub async fn global_proxy(&self) -> Result<String, SettingsError> {
        Ok(self.get(KEY_GLOBAL_PROXY).await?.unwrap_or_default())
    }

    /// `chat_send_mode` — frontend send binding, `"enter"` (default) or
    /// `"ctrl-enter"`. In Go this is a localStorage-only preference; this reads
    /// an optional same-named setting and falls back to `"enter"`.
    pub async fn chat_send_mode(&self) -> Result<String, SettingsError> {
        Ok(match self.get(KEY_CHAT_SEND_MODE).await? {
            Some(v) if v == "enter" || v == "ctrl-enter" => v,
            _ => DEFAULT_CHAT_SEND_MODE.to_string(),
        })
    }

    // -- typed setters (ported from the Go Set* methods) --------------------

    /// Set `python_interpreter` (trimmed).
    pub async fn set_python_interpreter(&self, value: &str) -> Result<(), SettingsError> {
        self.set(KEY_PYTHON_INTERP, value.trim()).await
    }

    /// Set `web_search_backend` (trimmed; empty → `ddgs`).
    pub async fn set_web_search_backend(&self, value: &str) -> Result<(), SettingsError> {
        let backend = value.trim();
        let backend = if backend.is_empty() {
            DEFAULT_WEB_SEARCH_BACKEND
        } else {
            backend
        };
        self.set(KEY_WEB_SEARCH_BACKEND, backend).await
    }

    /// Set `workers`; values `<= 0` are rejected (Go `Manager.SetWorkers`).
    pub async fn set_worker_concurrency(&self, n: i64) -> Result<(), SettingsError> {
        if n <= 0 {
            return Err(SettingsError::Invalid("workers must be >0".to_string()));
        }
        self.set(KEY_WORKERS, &n.to_string()).await
    }

    /// Persist the running-task concurrency cap (Go `Manager.SetConcurrency`).
    /// `limit < 1` is clamped to the default `5`.
    pub async fn set_concurrency(&self, enabled: bool, limit: i64) -> Result<(), SettingsError> {
        let limit = if limit < 1 {
            DEFAULT_CONCURRENCY_LIMIT
        } else {
            limit
        };
        self.set(KEY_TASK_CONCURRENCY_LIMIT, &limit.to_string())
            .await?;
        self.set_bool(KEY_TASK_CONCURRENCY_ENABLED, enabled).await
    }

    // -- stats (/api/stats) -------------------------------------------------

    /// Aggregate the database-backed fields of `GET /api/stats` (the global
    /// fields of `server/server.go` `stats()`).
    pub async fn stats(&self) -> Result<Stats, SettingsError> {
        let asset_counts = self.asset_counts().await?;
        let assets = asset_counts.values().copied().sum();
        let (tasks_total, tasks_by_status) = self.task_counts().await?;
        let llm_configured = self.llm_configured().await?;
        Ok(Stats {
            // Live engine state is owned by the (not-yet-ported) server engine;
            // the Go handler's placeholder before a task is resolved is "idle".
            engine_mode: "idle".to_string(),
            llm_configured,
            // Traffic recording is a separate store (not ported yet); unknown.
            traffic: None,
            traffic_enabled: false,
            assets,
            asset_counts,
            tasks_total,
            tasks_by_status,
        })
    }

    /// Asset counts grouped by type (Go `AssetStore.CountsByType`).
    pub async fn asset_counts(&self) -> Result<BTreeMap<String, i64>, SettingsError> {
        let rows =
            sqlx::query("SELECT type, count(*) AS n FROM assets GROUP BY type ORDER BY type")
                .fetch_all(self.db.pool())
                .await?;
        let mut out = BTreeMap::new();
        for r in rows {
            out.insert(r.get::<String, _>("type"), r.get::<i64, _>("n"));
        }
        Ok(out)
    }

    /// Live (non-deleted) task total and per-status counts.
    pub async fn task_counts(&self) -> Result<(i64, BTreeMap<String, i64>), SettingsError> {
        let rows = sqlx::query(
            "SELECT status, count(*) AS n FROM tasks WHERE deleted_at IS NULL \
             GROUP BY status ORDER BY status",
        )
        .fetch_all(self.db.pool())
        .await?;
        let mut by_status = BTreeMap::new();
        let mut total = 0i64;
        for r in rows {
            let n: i64 = r.get("n");
            total += n;
            by_status.insert(r.get::<String, _>("status"), n);
        }
        Ok((total, by_status))
    }

    /// Whether any LLM profile exists (Go `stats()` `llm_configured`).
    pub async fn llm_configured(&self) -> Result<bool, SettingsError> {
        let n: i64 = sqlx::query_scalar("SELECT count(*) FROM llm_profiles")
            .fetch_one(self.db.pool())
            .await?;
        Ok(n > 0)
    }
}

/// The database-backed subset of `GET /api/stats` (Go `server/server.go`
/// `stats()`), shaped for the preserved frontend (`web/src/lib/types.ts`
/// `Stats` plus the aggregate counts the migration adds).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Stats {
    /// Spec enum; always `idle` from the data layer (live state is computed by
    /// the server engine, not ported here).
    pub engine_mode: String,
    /// Whether any `llm_profiles` row exists.
    pub llm_configured: bool,
    /// Recorded-traffic count. `None` while the traffic store is not ported.
    pub traffic: Option<i64>,
    /// Whether traffic capture is enabled.
    pub traffic_enabled: bool,
    /// Total assets across all types.
    pub assets: i64,
    /// Asset counts per type.
    pub asset_counts: BTreeMap<String, i64>,
    /// Live (non-deleted) task total.
    pub tasks_total: i64,
    /// Live task counts per status.
    pub tasks_by_status: BTreeMap<String, i64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bool_parsing_matches_go() {
        assert!(bool_value("true"));
        assert!(bool_value("1"));
        assert!(!bool_value("false"));
        assert!(!bool_value("0"));
        assert!(!bool_value(""));
        assert!(!bool_value("TRUE"));
        assert!(!bool_value("yes"));
    }

    #[test]
    fn secret_key_filtering() {
        // The four Go secretSettingKeys.
        for k in SECRET_SETTING_KEYS {
            assert!(is_secret_key(k), "{k} must be secret");
        }
        // Suffix-based coverage.
        assert!(is_secret_key("some_api_key"));
        assert!(is_secret_key("other_proxy"));
        // Non-secrets pass through.
        assert!(!is_secret_key("traffic_capture"));
        assert!(!is_secret_key("web_search_backend"));
        assert!(!is_secret_key("notify_public_base_url"));
        assert!(!is_secret_key("python_interpreter"));
        assert!(!is_secret_key("artex_chat_send_mode"));
    }

    #[test]
    fn positive_and_bounded_parsing() {
        assert_eq!(parse_positive("3", DEFAULT_WORKERS), 3);
        assert_eq!(parse_positive(" 10 ", DEFAULT_WORKERS), 10);
        assert_eq!(parse_positive("0", DEFAULT_WORKERS), DEFAULT_WORKERS);
        assert_eq!(parse_positive("-2", DEFAULT_WORKERS), DEFAULT_WORKERS);
        assert_eq!(parse_positive("nope", DEFAULT_WORKERS), DEFAULT_WORKERS);

        assert_eq!(parse_bounded("0", 120, 4000), 0); // 0 is valid (no truncation)
        assert_eq!(parse_bounded("200", 120, 4000), 200);
        assert_eq!(parse_bounded("4001", 120, 4000), 120);
        assert_eq!(parse_bounded("-1", 120, 4000), 120);
        assert_eq!(parse_bounded("x", 120, 4000), 120);
    }

    #[test]
    fn concurrency_parsing() {
        assert_eq!(parse_concurrency(None, None), (false, 0));
        assert_eq!(
            parse_concurrency(Some("false".into()), Some("7".into())),
            (false, 0)
        );
        assert_eq!(
            parse_concurrency(Some("true".into()), None),
            (true, DEFAULT_CONCURRENCY_LIMIT)
        );
        assert_eq!(
            parse_concurrency(Some("true".into()), Some("9".into())),
            (true, 9)
        );
        assert_eq!(
            parse_concurrency(Some("true".into()), Some("0".into())),
            (true, DEFAULT_CONCURRENCY_LIMIT)
        );
        assert_eq!(
            parse_concurrency(Some(" true ".into()), Some("bad".into())),
            (true, DEFAULT_CONCURRENCY_LIMIT)
        );
    }
}
