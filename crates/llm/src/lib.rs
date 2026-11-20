//! LLM provider/config/records data layer — port of ARTEX `agent/provider.go`
//! (config model), `llmpool/health.go`, `llmrec/llmrec.go` and the LLM profile,
//! record and usage parts of `db/`.
//!
//! This crate covers the DB + config model + circuit-breaker bookkeeping. The
//! live HTTP streaming path (provider construction, SSE, proxies, retries) is
//! intentionally out of scope and remains unported.
//!
//! `api_key` is write-only on the wire (matching Go's `json:"-"`): it is never
//! serialized, and `LlmProfile`'s `Debug` impl redacts it. Use
//! [`LlmProfile::redact`] before logging or returning a profile to a caller
//! that must not see credentials.

#![forbid(unsafe_code)]

use chrono::{DateTime, Duration as ChronoDuration, Utc};
use database::Db;
use serde::{Deserialize, Serialize};
use sqlx::postgres::PgRow;
use sqlx::{Postgres, QueryBuilder, Row};
use std::fmt;

/// Errors from the LLM data layer.
#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    #[error("sql: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("invalid: {0}")]
    Invalid(String),
}

// ---------------------------------------------------------------------------
// Formats
// ---------------------------------------------------------------------------

/// LLM wire formats (`llm_profiles.format`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LlmFormat;

impl LlmFormat {
    pub const OPENAI: &'static str = "openai";
    pub const ANTHROPIC: &'static str = "anthropic";
    pub const OPENAI_RESPONSES: &'static str = "openai-responses";
    pub const OLLAMA: &'static str = "ollama";

    pub const ALL: [&'static str; 4] = [
        Self::OPENAI,
        Self::ANTHROPIC,
        Self::OPENAI_RESPONSES,
        Self::OLLAMA,
    ];

    /// Parse a user/config string into a canonical format. Case- and
    /// surrounding-space-insensitive, mirroring `agent.FromEnv`.
    pub fn parse(raw: &str) -> Result<&'static str, LlmError> {
        let normalized = raw.trim().to_ascii_lowercase();
        match normalized.as_str() {
            "openai" => Ok(Self::OPENAI),
            "anthropic" => Ok(Self::ANTHROPIC),
            "openai-responses" => Ok(Self::OPENAI_RESPONSES),
            "ollama" => Ok(Self::OLLAMA),
            _ => Err(LlmError::Invalid(format!(
                "unknown llm format {:?} (want one of {:?})",
                raw,
                Self::ALL
            ))),
        }
    }

    /// Whether `raw` is a known format.
    pub fn is_valid(raw: &str) -> bool {
        Self::parse(raw).is_ok()
    }

    /// The short provider name, defaulting to `anthropic` (Go's `Provider()`).
    pub fn provider_name(format: &str) -> &'static str {
        match format {
            "openai" => "openai",
            "openai-responses" => "openai-responses",
            "ollama" => "ollama",
            _ => "anthropic",
        }
    }

    /// Whether this format requires an API key. Ollama is keyless.
    pub fn needs_api_key(format: &str) -> bool {
        format != Self::OLLAMA
    }
}

// ---------------------------------------------------------------------------
// Retry overrides
// ---------------------------------------------------------------------------

/// One retry layer's override (`db/llmretry.go` `RetryRule`).
///
/// `attempts`: 0 = inherit the global default, -1 = disable, >0 = use this
/// count. `interval_ms`: 0 = keep the layer's exponential backoff, >0 = fixed
/// interval.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RetryRule {
    pub attempts: i32,
    pub interval_ms: i32,
}

impl RetryRule {
    pub const MAX_ATTEMPTS: i32 = 20;
    pub const MAX_INTERVAL_MS: i32 = 3_600_000;

    /// Pull out-of-range values back into the band the DB CHECK accepts.
    pub fn clamped(mut self) -> Self {
        self.attempts = self.attempts.clamp(-1, Self::MAX_ATTEMPTS);
        self.interval_ms = self.interval_ms.clamp(0, Self::MAX_INTERVAL_MS);
        self
    }

    /// Field-wise inherit: each zero field is filled from `fallback`.
    pub fn or(mut self, fallback: Self) -> Self {
        if self.attempts == 0 {
            self.attempts = fallback.attempts;
        }
        if self.interval_ms == 0 {
            self.interval_ms = fallback.interval_ms;
        }
        self
    }
}

/// Per-profile retry overrides (`db/llmretry.go` `RetryOverride`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RetryOverride {
    pub connect: RetryRule,
    pub empty: RetryRule,
    pub stream: RetryRule,
}

impl RetryOverride {
    pub fn clamped(self) -> Self {
        Self {
            connect: self.connect.clamped(),
            empty: self.empty.clamped(),
            stream: self.stream.clamped(),
        }
    }
}

// ---------------------------------------------------------------------------
// LLM profile
// ---------------------------------------------------------------------------

fn is_zero_f64(v: &f64) -> bool {
    *v == 0.0
}

fn non_empty(s: Option<&str>) -> Option<&str> {
    s.filter(|v| !v.is_empty())
}

/// One row of `llm_profiles`, mirroring Go's `db.LLMProfile` JSON shape.
///
/// `api_key` is never serialized; use [`LlmProfile::redact`] before logging.
#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct LlmProfile {
    pub id: i64,
    pub name: String,
    pub format: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proxy: Option<String>,
    pub model: String,
    #[serde(skip_serializing, default)]
    pub api_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key_hint: Option<String>,
    pub rate_per_second: f64,
    pub rate_per_minute: f64,
    pub context_window_k: i32,
    pub thinking_type: String,
    pub reasoning_effort: String,
    pub is_default: bool,
    pub priority: i32,
    pub pool_exclude: bool,
    pub streaming: bool,
    pub max_tokens: i32,
    pub max_tokens_field: String,
    pub session_header_key: String,
    pub user_agent: String,
    #[serde(default)]
    pub retry: RetryOverride,
    #[serde(default, skip_serializing_if = "is_zero_f64")]
    pub price_input_per_m: f64,
    #[serde(default, skip_serializing_if = "is_zero_f64")]
    pub price_output_per_m: f64,
    #[serde(default, skip_serializing_if = "is_zero_f64")]
    pub price_cache_read_per_m: f64,
    #[serde(default, skip_serializing_if = "is_zero_f64")]
    pub price_cache_write_per_m: f64,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

impl LlmProfile {
    /// A copy safe to log or return: the API key is dropped.
    pub fn redact(&self) -> Self {
        let mut out = self.clone();
        out.api_key = None;
        out
    }
}

impl fmt::Debug for LlmProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LlmProfile")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("format", &self.format)
            .field("base_url", &self.base_url)
            .field("proxy", &self.proxy)
            .field("model", &self.model)
            .field("api_key", &self.api_key.as_ref().map(|_| "***"))
            .field("api_key_hint", &self.api_key_hint)
            .field("rate_per_second", &self.rate_per_second)
            .field("rate_per_minute", &self.rate_per_minute)
            .field("context_window_k", &self.context_window_k)
            .field("thinking_type", &self.thinking_type)
            .field("reasoning_effort", &self.reasoning_effort)
            .field("is_default", &self.is_default)
            .field("priority", &self.priority)
            .field("pool_exclude", &self.pool_exclude)
            .field("streaming", &self.streaming)
            .field("max_tokens", &self.max_tokens)
            .field("max_tokens_field", &self.max_tokens_field)
            .field("session_header_key", &self.session_header_key)
            .field("user_agent", &self.user_agent)
            .field("retry", &self.retry)
            .field("price_input_per_m", &self.price_input_per_m)
            .field("price_output_per_m", &self.price_output_per_m)
            .field("price_cache_read_per_m", &self.price_cache_read_per_m)
            .field("price_cache_write_per_m", &self.price_cache_write_per_m)
            .field("created_at", &self.created_at)
            .field("updated_at", &self.updated_at)
            .finish()
    }
}

fn api_key_hint(key: &str) -> Option<String> {
    // Slice the last four bytes like Go, but never panic on a multi-byte
    // character boundary.
    if key.len() < 4 {
        return None;
    }
    key.get(key.len() - 4..)
        .map(|tail| format!("\u{2026}{tail}"))
}

fn provided_hint(p: &LlmProfile) -> Option<String> {
    p.api_key_hint
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Select list. `with_key=false` avoids reading the secret column at all.
fn profile_columns(with_key: bool) -> String {
    let key = if with_key {
        "api_key"
    } else {
        "NULL::text AS api_key"
    };
    format!(
        "id, name, format, base_url, proxy, model, {key}, api_key_hint, \
         rate_per_second, rate_per_minute, context_window_k, reasoning_effort, \
         thinking_type, is_default, priority, pool_exclude, streaming, max_tokens, \
         max_tokens_field, session_header_key, user_agent, \
         COALESCE(retry_connect_attempts,0) AS retry_connect_attempts, \
         COALESCE(retry_connect_interval_ms,0) AS retry_connect_interval_ms, \
         COALESCE(retry_empty_attempts,0) AS retry_empty_attempts, \
         COALESCE(retry_empty_interval_ms,0) AS retry_empty_interval_ms, \
         COALESCE(retry_stream_attempts,0) AS retry_stream_attempts, \
         COALESCE(retry_stream_interval_ms,0) AS retry_stream_interval_ms, \
         COALESCE(price_input_per_m,0)::double precision AS price_input_per_m, \
         COALESCE(price_output_per_m,0)::double precision AS price_output_per_m, \
         COALESCE(price_cache_read_per_m,0)::double precision AS price_cache_read_per_m, \
         COALESCE(price_cache_write_per_m,0)::double precision AS price_cache_write_per_m, \
         created_at::text AS created_at, updated_at::text AS updated_at"
    )
}

fn scan_profile(row: &PgRow) -> LlmProfile {
    LlmProfile {
        id: row.get("id"),
        name: row.get("name"),
        format: row.get("format"),
        base_url: row.try_get("base_url").ok().flatten(),
        proxy: row.try_get("proxy").ok().flatten(),
        model: row.get("model"),
        api_key: row.try_get("api_key").ok().flatten(),
        api_key_hint: row.try_get("api_key_hint").ok().flatten(),
        rate_per_second: row.get("rate_per_second"),
        rate_per_minute: row.get("rate_per_minute"),
        context_window_k: row.get("context_window_k"),
        thinking_type: row.get("thinking_type"),
        reasoning_effort: row.get("reasoning_effort"),
        is_default: row.get("is_default"),
        priority: row.get("priority"),
        pool_exclude: row.get("pool_exclude"),
        streaming: row.get("streaming"),
        max_tokens: row.get("max_tokens"),
        max_tokens_field: row.get("max_tokens_field"),
        session_header_key: row.get("session_header_key"),
        user_agent: row.get("user_agent"),
        retry: RetryOverride {
            connect: RetryRule {
                attempts: row.get("retry_connect_attempts"),
                interval_ms: row.get("retry_connect_interval_ms"),
            },
            empty: RetryRule {
                attempts: row.get("retry_empty_attempts"),
                interval_ms: row.get("retry_empty_interval_ms"),
            },
            stream: RetryRule {
                attempts: row.get("retry_stream_attempts"),
                interval_ms: row.get("retry_stream_interval_ms"),
            },
        },
        price_input_per_m: row.get("price_input_per_m"),
        price_output_per_m: row.get("price_output_per_m"),
        price_cache_read_per_m: row.get("price_cache_read_per_m"),
        price_cache_write_per_m: row.get("price_cache_write_per_m"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

/// `llm_profiles` CRUD, port of `db/config.go` profile methods.
pub struct LlmProfileStore {
    db: Db,
}

impl LlmProfileStore {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    /// All profiles ordered by id, without API keys (only the hint).
    pub async fn list(&self) -> Result<Vec<LlmProfile>, LlmError> {
        let sql = format!(
            "SELECT {} FROM llm_profiles ORDER BY id",
            profile_columns(false)
        );
        let rows = sqlx::query(&sql).fetch_all(self.db.pool()).await?;
        Ok(rows.iter().map(scan_profile).collect())
    }

    /// One profile by id, with its API key. `None` when absent.
    pub async fn get(&self, id: i64) -> Result<Option<LlmProfile>, LlmError> {
        let sql = format!(
            "SELECT {} FROM llm_profiles WHERE id = $1",
            profile_columns(true)
        );
        let row = sqlx::query(&sql)
            .bind(id)
            .fetch_optional(self.db.pool())
            .await?;
        Ok(row.as_ref().map(scan_profile))
    }

    /// One profile by exact name, with its API key.
    pub async fn get_by_name(&self, name: &str) -> Result<Option<LlmProfile>, LlmError> {
        let sql = format!(
            "SELECT {} FROM llm_profiles WHERE name = $1",
            profile_columns(true)
        );
        let row = sqlx::query(&sql)
            .bind(name)
            .fetch_optional(self.db.pool())
            .await?;
        Ok(row.as_ref().map(scan_profile))
    }

    /// The active (default) profile, with its API key. `None` when unset.
    pub async fn get_active(&self) -> Result<Option<LlmProfile>, LlmError> {
        let sql = format!(
            "SELECT {} FROM llm_profiles WHERE is_default LIMIT 1",
            profile_columns(true)
        );
        let row = sqlx::query(&sql).fetch_optional(self.db.pool()).await?;
        Ok(row.as_ref().map(scan_profile))
    }

    /// Alias for [`LlmProfileStore::get_active`].
    pub async fn get_default(&self) -> Result<Option<LlmProfile>, LlmError> {
        self.get_active().await
    }

    /// Insert a profile. Returns the new id. Mirrors Go's `SaveProfile`: an
    /// empty API key stores NULL, and `is_default` is managed separately by
    /// [`LlmProfileStore::set_active`].
    pub async fn create(&self, p: &LlmProfile) -> Result<i64, LlmError> {
        let key = non_empty(p.api_key.as_deref().map(str::trim));
        let hint = match key {
            Some(k) => api_key_hint(k).or_else(|| provided_hint(p)),
            None => provided_hint(p),
        };
        let retry = p.retry.clamped();
        let row = sqlx::query(
            "INSERT INTO llm_profiles(\
             name, format, base_url, proxy, model, api_key, api_key_hint, \
             rate_per_second, rate_per_minute, context_window_k, reasoning_effort, \
             thinking_type, priority, pool_exclude, streaming, max_tokens, \
             max_tokens_field, session_header_key, user_agent, \
             retry_connect_attempts, retry_connect_interval_ms, \
             retry_empty_attempts, retry_empty_interval_ms, \
             retry_stream_attempts, retry_stream_interval_ms, \
             price_input_per_m, price_output_per_m, price_cache_read_per_m, \
             price_cache_write_per_m) \
             VALUES ($1,$2,NULLIF($3,''),NULLIF($4,''),$5,NULLIF($6,''),NULLIF($7,''),\
             $8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23,$24,$25,\
             $26::numeric,$27::numeric,$28::numeric,$29::numeric) RETURNING id",
        )
        .bind(&p.name)
        .bind(&p.format)
        .bind(non_empty(p.base_url.as_deref()))
        .bind(non_empty(p.proxy.as_deref()))
        .bind(&p.model)
        .bind(key)
        .bind(hint.as_deref())
        .bind(p.rate_per_second)
        .bind(p.rate_per_minute)
        .bind(p.context_window_k)
        .bind(&p.reasoning_effort)
        .bind(&p.thinking_type)
        .bind(p.priority)
        .bind(p.pool_exclude)
        .bind(p.streaming)
        .bind(p.max_tokens)
        .bind(&p.max_tokens_field)
        .bind(&p.session_header_key)
        .bind(&p.user_agent)
        .bind(retry.connect.attempts)
        .bind(retry.connect.interval_ms)
        .bind(retry.empty.attempts)
        .bind(retry.empty.interval_ms)
        .bind(retry.stream.attempts)
        .bind(retry.stream.interval_ms)
        .bind(p.price_input_per_m)
        .bind(p.price_output_per_m)
        .bind(p.price_cache_read_per_m)
        .bind(p.price_cache_write_per_m)
        .fetch_one(self.db.pool())
        .await?;
        Ok(row.get("id"))
    }

    /// Update a profile by `p.id`. A `None`/empty `api_key` keeps the stored
    /// key (Go's behaviour). Returns the id.
    pub async fn update(&self, p: &LlmProfile) -> Result<i64, LlmError> {
        let key = non_empty(p.api_key.as_deref().map(str::trim));
        let hint = match key {
            Some(k) => api_key_hint(k).or_else(|| provided_hint(p)),
            None => None,
        };
        let retry = p.retry.clamped();
        let res = sqlx::query(
            "UPDATE llm_profiles SET \
             name=$1, format=$2, base_url=NULLIF($3,''), proxy=NULLIF($4,''), model=$5, \
             api_key=COALESCE($6, api_key), api_key_hint=COALESCE($7, api_key_hint), \
             rate_per_second=$8, rate_per_minute=$9, context_window_k=$10, \
             reasoning_effort=$11, thinking_type=$12, priority=$13, pool_exclude=$14, \
             streaming=$15, max_tokens=$16, max_tokens_field=$17, \
             session_header_key=$18, user_agent=$19, \
             retry_connect_attempts=$20, retry_connect_interval_ms=$21, \
             retry_empty_attempts=$22, retry_empty_interval_ms=$23, \
             retry_stream_attempts=$24, retry_stream_interval_ms=$25, \
             price_input_per_m=$26::numeric, price_output_per_m=$27::numeric, \
             price_cache_read_per_m=$28::numeric, price_cache_write_per_m=$29::numeric \
             WHERE id=$30",
        )
        .bind(&p.name)
        .bind(&p.format)
        .bind(non_empty(p.base_url.as_deref()))
        .bind(non_empty(p.proxy.as_deref()))
        .bind(&p.model)
        .bind(key)
        .bind(hint.as_deref())
        .bind(p.rate_per_second)
        .bind(p.rate_per_minute)
        .bind(p.context_window_k)
        .bind(&p.reasoning_effort)
        .bind(&p.thinking_type)
        .bind(p.priority)
        .bind(p.pool_exclude)
        .bind(p.streaming)
        .bind(p.max_tokens)
        .bind(&p.max_tokens_field)
        .bind(&p.session_header_key)
        .bind(&p.user_agent)
        .bind(retry.connect.attempts)
        .bind(retry.connect.interval_ms)
        .bind(retry.empty.attempts)
        .bind(retry.empty.interval_ms)
        .bind(retry.stream.attempts)
        .bind(retry.stream.interval_ms)
        .bind(p.price_input_per_m)
        .bind(p.price_output_per_m)
        .bind(p.price_cache_read_per_m)
        .bind(p.price_cache_write_per_m)
        .bind(p.id)
        .execute(self.db.pool())
        .await?;
        if res.rows_affected() == 0 {
            return Err(LlmError::NotFound(format!("llm profile {}", p.id)));
        }
        Ok(p.id)
    }

    /// Delete a non-default profile. Deleting the active profile is rejected.
    ///
    /// The Go implementation also repairs task failover cursors under a
    /// reference lock; that task-chain machinery is out of scope here, and the
    /// schema's `ON DELETE SET NULL` / `ON DELETE CASCADE` FKs handle
    /// references.
    pub async fn delete(&self, id: i64) -> Result<(), LlmError> {
        let res = sqlx::query("DELETE FROM llm_profiles WHERE id = $1 AND is_default = false")
            .bind(id)
            .execute(self.db.pool())
            .await?;
        if res.rows_affected() == 1 {
            return Ok(());
        }
        let is_default: Option<bool> =
            sqlx::query_scalar("SELECT is_default FROM llm_profiles WHERE id = $1")
                .bind(id)
                .fetch_optional(self.db.pool())
                .await?;
        match is_default {
            Some(true) => Err(LlmError::Invalid(
                "cannot delete the active LLM profile; activate another profile first".into(),
            )),
            _ => Err(LlmError::NotFound(format!("llm profile {id}"))),
        }
    }

    /// Make one profile the global default (single-default invariant).
    pub async fn set_active(&self, id: i64) -> Result<(), LlmError> {
        let mut tx = self.db.pool().begin().await?;
        sqlx::query("UPDATE llm_profiles SET is_default = false WHERE is_default")
            .execute(&mut *tx)
            .await?;
        let res = sqlx::query("UPDATE llm_profiles SET is_default = true WHERE id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        if res.rows_affected() != 1 {
            return Err(LlmError::NotFound(format!("llm profile {id}")));
        }
        tx.commit().await?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Circuit-breaker health
// ---------------------------------------------------------------------------

/// Default consecutive transient failures that trip the breaker
/// (`llmpool/health.go` `softTripAfter`).
pub const SOFT_TRIP_AFTER: i32 = 3;

/// Cooling-off window for the `trips`-th trip (1-based), port of
/// `llmpool/health.go`'s `backoff` ladder: 1m, 5m, then 30m for every later
/// trip.
pub fn backoff_for(trips: i32) -> ChronoDuration {
    let seconds = match trips {
        t if t <= 1 => 60,
        2 => 300,
        _ => 1800,
    };
    ChronoDuration::seconds(seconds)
}

/// One profile's circuit-breaker state (`llm_profile_health`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct LlmProfileHealth {
    pub profile_id: i64,
    pub fails: i32,
    pub trips: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub open_until: Option<DateTime<Utc>>,
    pub last_error: String,
    pub last_at: DateTime<Utc>,
}

impl LlmProfileHealth {
    /// Whether the breaker is currently open (profile should be skipped).
    pub fn is_open(&self) -> bool {
        self.is_open_at(Utc::now())
    }

    /// Whether the breaker is open at a given instant.
    pub fn is_open_at(&self, now: DateTime<Utc>) -> bool {
        self.open_until.map(|until| now < until).unwrap_or(false)
    }
}

/// `llm_profile_health` bookkeeping, port of `llmpool/health.go` +
/// `db/llmhealth.go`.
pub struct HealthStore {
    db: Db,
}

impl HealthStore {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    /// One profile's state, or `None` when no row exists.
    pub async fn get(&self, profile_id: i64) -> Result<Option<LlmProfileHealth>, LlmError> {
        let row = sqlx::query(
            "SELECT profile_id, fails, trips, open_until, last_error, last_at \
             FROM llm_profile_health WHERE profile_id = $1",
        )
        .bind(profile_id)
        .fetch_optional(self.db.pool())
        .await?;
        Ok(row.as_ref().map(scan_health))
    }

    /// Record a failed call. `hard` marks a deterministic failure (bad key, no
    /// credit) that opens the breaker immediately; transient failures need
    /// [`SOFT_TRIP_AFTER`] in a row. Returns whether this call opened the
    /// breaker.
    pub async fn record_failure(
        &self,
        profile_id: i64,
        error: &str,
        hard: bool,
    ) -> Result<bool, LlmError> {
        let mut tx = self.db.pool().begin().await?;
        // Materialize the row so the SELECT below can take a row lock and
        // serialize concurrent failures for the same profile.
        sqlx::query(
            "INSERT INTO llm_profile_health(\
             profile_id, fails, trips, open_until, last_error, last_at) \
             VALUES ($1, 0, 0, NULL, $2, now()) \
             ON CONFLICT (profile_id) DO NOTHING",
        )
        .bind(profile_id)
        .bind(error)
        .execute(&mut *tx)
        .await?;
        let row = sqlx::query(
            "SELECT fails, trips, open_until FROM llm_profile_health \
             WHERE profile_id = $1 FOR UPDATE",
        )
        .bind(profile_id)
        .fetch_one(&mut *tx)
        .await?;
        let mut fails = row.get::<i32, _>("fails") + 1;
        let mut trips = row.get::<i32, _>("trips");
        let mut open_until = row.get::<Option<DateTime<Utc>>, _>("open_until");
        let tripped = hard || fails >= SOFT_TRIP_AFTER;
        if tripped {
            trips += 1;
            open_until = Some(Utc::now() + backoff_for(trips));
            fails = 0;
        }
        sqlx::query(
            "UPDATE llm_profile_health SET fails = $2, trips = $3, open_until = $4, \
             last_error = $5, last_at = now() WHERE profile_id = $1",
        )
        .bind(profile_id)
        .bind(fails)
        .bind(trips)
        .bind(open_until)
        .bind(error)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(tripped)
    }

    /// Record a successful call: clears the profile's breaker state (Go's
    /// `Registry.Pass` forgets the entry entirely).
    pub async fn record_success(&self, profile_id: i64) -> Result<(), LlmError> {
        sqlx::query("DELETE FROM llm_profile_health WHERE profile_id = $1")
            .bind(profile_id)
            .execute(self.db.pool())
            .await?;
        Ok(())
    }

    /// Clear one profile's state — the UI's "recover now" action.
    pub async fn reset(&self, profile_id: i64) -> Result<(), LlmError> {
        self.record_success(profile_id).await
    }

    /// Profiles still in an unexpired cooling-off window, for startup restore
    /// (`db/llmhealth.go` `LoadLLMHealth`).
    pub async fn load_open(&self) -> Result<Vec<LlmProfileHealth>, LlmError> {
        let rows = sqlx::query(
            "SELECT profile_id, fails, trips, open_until, last_error, last_at \
             FROM llm_profile_health \
             WHERE open_until IS NOT NULL AND open_until > now()",
        )
        .fetch_all(self.db.pool())
        .await?;
        Ok(rows.iter().map(scan_health).collect())
    }
}

fn scan_health(row: &PgRow) -> LlmProfileHealth {
    LlmProfileHealth {
        profile_id: row.get("profile_id"),
        fails: row.get("fails"),
        trips: row.get("trips"),
        open_until: row.get("open_until"),
        last_error: row.get("last_error"),
        last_at: row.get("last_at"),
    }
}

// ---------------------------------------------------------------------------
// LLM records
// ---------------------------------------------------------------------------

/// One row of `llm_records`, port of `db.LLMRecord`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct LlmRecord {
    pub id: i64,
    pub ts: String,
    pub model: String,
    pub profile_name: String,
    pub session_id: String,
    pub task_id: String,
    pub worker: String,
    pub latency_ms: i32,
    pub input_tokens: i32,
    pub output_tokens: i32,
    pub cache_read: i32,
    pub cache_write: i32,
    pub status: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub error: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub request_body: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub response_body: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub raw_request: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub raw_response: String,
}

/// Filters for [`LlmRecordStore::list`].
#[derive(Debug, Clone, Default)]
pub struct LlmRecordFilter {
    pub task_id: Option<String>,
    pub model: Option<String>,
    pub limit: i64,
    pub offset: i64,
}

/// A page of records plus the unpaginated total.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct LlmRecordPage {
    pub records: Vec<LlmRecord>,
    pub total: i64,
}

/// One distinct task with its record count (`db.LLMTask`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct LlmTask {
    pub task_id: String,
    pub count: i64,
}

/// One model's aggregated token usage for a task (`db.ModelTokenStat`).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct ModelTokenStat {
    pub model: String,
    pub calls: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
}

const RECORD_COLUMNS: &str = "id, ts::text AS ts, \
     COALESCE(model,'') AS model, COALESCE(profile_name,'') AS profile_name, \
     COALESCE(session_id,'') AS session_id, COALESCE(task_id,'') AS task_id, \
     COALESCE(worker,'') AS worker, COALESCE(latency_ms,0) AS latency_ms, \
     COALESCE(input_tokens,0) AS input_tokens, \
     COALESCE(output_tokens,0) AS output_tokens, \
     COALESCE(cache_read,0) AS cache_read, COALESCE(cache_write,0) AS cache_write, \
     COALESCE(status,'') AS status, COALESCE(error,'') AS error";

const RECORD_COLUMNS_FULL: &str = "id, ts::text AS ts, \
     COALESCE(model,'') AS model, COALESCE(profile_name,'') AS profile_name, \
     COALESCE(session_id,'') AS session_id, COALESCE(task_id,'') AS task_id, \
     COALESCE(worker,'') AS worker, COALESCE(latency_ms,0) AS latency_ms, \
     COALESCE(input_tokens,0) AS input_tokens, \
     COALESCE(output_tokens,0) AS output_tokens, \
     COALESCE(cache_read,0) AS cache_read, COALESCE(cache_write,0) AS cache_write, \
     COALESCE(status,'') AS status, COALESCE(error,'') AS error, \
     COALESCE(request_body,'') AS request_body, \
     COALESCE(response_body,'') AS response_body, \
     COALESCE(raw_request,'') AS raw_request, \
     COALESCE(raw_response,'') AS raw_response";

fn scan_record(row: &PgRow) -> LlmRecord {
    LlmRecord {
        id: row.get("id"),
        ts: row.get("ts"),
        model: row.get("model"),
        profile_name: row.get("profile_name"),
        session_id: row.get("session_id"),
        task_id: row.get("task_id"),
        worker: row.get("worker"),
        latency_ms: row.get("latency_ms"),
        input_tokens: row.get("input_tokens"),
        output_tokens: row.get("output_tokens"),
        cache_read: row.get("cache_read"),
        cache_write: row.get("cache_write"),
        status: row.get("status"),
        error: row.get("error"),
        request_body: row.try_get("request_body").unwrap_or_default(),
        response_body: row.try_get("response_body").unwrap_or_default(),
        raw_request: row.try_get("raw_request").unwrap_or_default(),
        raw_response: row.try_get("raw_response").unwrap_or_default(),
    }
}

fn apply_record_filters<'a>(qb: &mut QueryBuilder<'a, Postgres>, filter: &LlmRecordFilter) {
    if let Some(task) = filter.task_id.as_deref().filter(|s| !s.is_empty()) {
        qb.push(" AND COALESCE(task_id, '') = ")
            .push_bind(task.to_string());
    }
    if let Some(model) = filter.model.as_deref().filter(|s| !s.is_empty()) {
        qb.push(" AND model = ").push_bind(model.to_string());
    }
}

/// `llm_records` read operations, port of `db/commands.go` record methods.
pub struct LlmRecordStore {
    db: Db,
}

impl LlmRecordStore {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    /// Paginated records, newest first, with the unpaginated total.
    pub async fn list(&self, filter: LlmRecordFilter) -> Result<LlmRecordPage, LlmError> {
        let mut count = QueryBuilder::new("SELECT COUNT(*) FROM llm_records WHERE 1 = 1");
        apply_record_filters(&mut count, &filter);
        let total: i64 = count.build_query_scalar().fetch_one(self.db.pool()).await?;

        let limit = if filter.limit <= 0 { 50 } else { filter.limit };
        let offset = if filter.offset < 0 { 0 } else { filter.offset };
        let mut qb = QueryBuilder::new("SELECT ");
        qb.push(RECORD_COLUMNS);
        qb.push(" FROM llm_records WHERE 1 = 1");
        apply_record_filters(&mut qb, &filter);
        qb.push(" ORDER BY id DESC LIMIT ")
            .push_bind(limit)
            .push(" OFFSET ")
            .push_bind(offset);
        let rows = qb.build().fetch_all(self.db.pool()).await?;
        let records = rows.iter().map(scan_record).collect();
        Ok(LlmRecordPage { records, total })
    }

    /// One record with full request/response bodies. `None` when absent.
    pub async fn get(&self, id: i64) -> Result<Option<LlmRecord>, LlmError> {
        let sql = format!(
            "SELECT {} FROM llm_records WHERE id = $1",
            RECORD_COLUMNS_FULL
        );
        let row = sqlx::query(&sql)
            .bind(id)
            .fetch_optional(self.db.pool())
            .await?;
        Ok(row.as_ref().map(scan_record))
    }

    /// Distinct non-empty task ids with counts, most recent first.
    pub async fn list_tasks(&self) -> Result<Vec<LlmTask>, LlmError> {
        let rows = sqlx::query(
            "SELECT task_id, COUNT(*) AS n FROM llm_records \
             WHERE COALESCE(task_id,'') <> '' \
             GROUP BY task_id ORDER BY MAX(id) DESC",
        )
        .fetch_all(self.db.pool())
        .await?;
        Ok(rows
            .iter()
            .map(|r| LlmTask {
                task_id: r.get("task_id"),
                count: r.get("n"),
            })
            .collect())
    }

    /// A task's token usage grouped by model, from the `llm_usage` ledger
    /// (Go's `TokenByModel`).
    pub async fn by_model(&self, task_id: &str) -> Result<Vec<ModelTokenStat>, LlmError> {
        token_by_model(&self.db, task_id).await
    }

    /// Delete every record for one exact `task_id`; returns rows deleted.
    pub async fn delete_by_task(&self, task: &str) -> Result<u64, LlmError> {
        let res = sqlx::query("DELETE FROM llm_records WHERE COALESCE(task_id,'') = $1")
            .bind(task)
            .execute(self.db.pool())
            .await?;
        Ok(res.rows_affected())
    }
}

async fn token_by_model(db: &Db, task_id: &str) -> Result<Vec<ModelTokenStat>, LlmError> {
    let rows = sqlx::query(
        "SELECT COALESCE(NULLIF(model,''),'(unknown)') AS model, COUNT(*) AS calls, \
         COALESCE(SUM(input_tokens),0) AS input_tokens, \
         COALESCE(SUM(output_tokens),0) AS output_tokens, \
         COALESCE(SUM(cache_read),0) AS cache_read_tokens, \
         COALESCE(SUM(cache_write),0) AS cache_write_tokens \
         FROM llm_usage WHERE COALESCE(task_id,'') = $1 \
         GROUP BY model \
         ORDER BY SUM(input_tokens) + SUM(output_tokens) DESC, model",
    )
    .bind(task_id)
    .fetch_all(db.pool())
    .await?;
    Ok(rows
        .iter()
        .map(|r| ModelTokenStat {
            model: r.get("model"),
            calls: r.get("calls"),
            input_tokens: r.get("input_tokens"),
            output_tokens: r.get("output_tokens"),
            cache_read_tokens: r.get("cache_read_tokens"),
            cache_write_tokens: r.get("cache_write_tokens"),
        })
        .collect())
}

// ---------------------------------------------------------------------------
// LLM usage
// ---------------------------------------------------------------------------

/// One (profile, UTC day) token bucket (`db.ProfileDayUsage`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ProfileDayUsage {
    pub profile_name: String,
    /// `YYYY-MM-DD` (UTC).
    pub date: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
}

/// One conversation's summed `kind='result'` token usage
/// (`db.ConvTokenSummary`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ConvTokenSummary {
    pub llm_profile_id: Option<i64>,
    pub created_at: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
}

/// Global token totals from the `llm_usage` ledger.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct LlmUsageTotal {
    pub calls: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
}

/// Per-profile token totals (`db.ProfileUsage`, minus the archived-task and
/// cost joins that have no Rust counterpart yet).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct ProfileUsage {
    pub profile_name: String,
    pub calls: i64,
    pub tasks: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
}

/// `llm_usage` aggregations, port of the read side of `db/llm_usage.go` and
/// `db/conversation.go`.
pub struct LlmUsageStore {
    db: Db,
}

impl LlmUsageStore {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    /// Per-(profile, day) token buckets for the past `days` days (default 365
    /// when `days <= 0`), port of `db.UsageDaily`.
    pub async fn daily(&self, days: i32) -> Result<Vec<ProfileDayUsage>, LlmError> {
        let days = if days <= 0 { 365 } else { days };
        let rows = sqlx::query(
            "SELECT COALESCE(profile_name,'') AS profile_name, \
             to_char(ts AT TIME ZONE 'UTC', 'YYYY-MM-DD') AS day, \
             COALESCE(SUM(input_tokens),0) AS input_tokens, \
             COALESCE(SUM(output_tokens),0) AS output_tokens, \
             COALESCE(SUM(cache_read),0) AS cache_read_tokens \
             FROM llm_usage \
             WHERE ts >= now() - ($1 * interval '1 day') \
             GROUP BY profile_name, day ORDER BY day",
        )
        .bind(days)
        .fetch_all(self.db.pool())
        .await?;
        Ok(rows
            .iter()
            .map(|r| ProfileDayUsage {
                profile_name: r.get("profile_name"),
                date: r.get("day"),
                input_tokens: r.get("input_tokens"),
                output_tokens: r.get("output_tokens"),
                cache_read_tokens: r.get("cache_read_tokens"),
            })
            .collect())
    }

    /// One row per conversation with its summed result-row token usage
    /// (`db.ConversationTokenSummaries`).
    pub async fn conversations(&self) -> Result<Vec<ConvTokenSummary>, LlmError> {
        let rows = sqlx::query(
            "SELECT c.llm_profile_id, c.created_at::text AS created_at, \
             COALESCE(sum(ca.input_tokens),0) AS input_tokens, \
             COALESCE(sum(ca.output_tokens),0) AS output_tokens, \
             COALESCE(sum(ca.cache_read_tokens),0) AS cache_read_tokens, \
             COALESCE(sum(ca.cache_write_tokens),0) AS cache_write_tokens \
             FROM conversations c \
             LEFT JOIN conversation_activities ca \
               ON ca.conversation_id = c.id AND ca.kind = 'result' \
             GROUP BY c.id",
        )
        .fetch_all(self.db.pool())
        .await?;
        Ok(rows
            .iter()
            .map(|r| ConvTokenSummary {
                llm_profile_id: r.get("llm_profile_id"),
                created_at: r.get("created_at"),
                input_tokens: r.get("input_tokens"),
                output_tokens: r.get("output_tokens"),
                cache_read_tokens: r.get("cache_read_tokens"),
                cache_write_tokens: r.get("cache_write_tokens"),
            })
            .collect())
    }

    /// All-time token totals across the whole ledger.
    pub async fn total(&self) -> Result<LlmUsageTotal, LlmError> {
        let row = sqlx::query(
            "SELECT COUNT(*) AS calls, \
             COALESCE(SUM(input_tokens),0) AS input_tokens, \
             COALESCE(SUM(output_tokens),0) AS output_tokens, \
             COALESCE(SUM(cache_read),0) AS cache_read_tokens, \
             COALESCE(SUM(cache_write),0) AS cache_write_tokens \
             FROM llm_usage",
        )
        .fetch_one(self.db.pool())
        .await?;
        Ok(LlmUsageTotal {
            calls: row.get("calls"),
            input_tokens: row.get("input_tokens"),
            output_tokens: row.get("output_tokens"),
            cache_read_tokens: row.get("cache_read_tokens"),
            cache_write_tokens: row.get("cache_write_tokens"),
        })
    }

    /// All-time token totals grouped by profile name, most-used first (Go's
    /// `UsageByProfile`, without the archived-task and cost joins).
    pub async fn by_profile(&self) -> Result<Vec<ProfileUsage>, LlmError> {
        let rows = sqlx::query(
            "SELECT COALESCE(profile_name,'') AS profile_name, COUNT(*) AS calls, \
             COUNT(DISTINCT task_id) AS tasks, \
             COALESCE(SUM(input_tokens),0) AS input_tokens, \
             COALESCE(SUM(output_tokens),0) AS output_tokens, \
             COALESCE(SUM(cache_read),0) AS cache_read_tokens, \
             COALESCE(SUM(cache_write),0) AS cache_write_tokens \
             FROM llm_usage GROUP BY profile_name \
             ORDER BY SUM(input_tokens) + SUM(output_tokens) DESC",
        )
        .fetch_all(self.db.pool())
        .await?;
        Ok(rows
            .iter()
            .map(|r| ProfileUsage {
                profile_name: r.get("profile_name"),
                calls: r.get("calls"),
                tasks: r.get("tasks"),
                input_tokens: r.get("input_tokens"),
                output_tokens: r.get("output_tokens"),
                cache_read_tokens: r.get("cache_read_tokens"),
                cache_write_tokens: r.get("cache_write_tokens"),
            })
            .collect())
    }

    /// A task's token usage grouped by model (Go's `TokenByModel`).
    pub async fn by_model(&self, task_id: &str) -> Result<Vec<ModelTokenStat>, LlmError> {
        token_by_model(&self.db, task_id).await
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_parse_is_case_and_space_insensitive() {
        assert_eq!(LlmFormat::parse("OpenAI").unwrap(), LlmFormat::OPENAI);
        assert_eq!(
            LlmFormat::parse("  anthropic ").unwrap(),
            LlmFormat::ANTHROPIC
        );
        assert_eq!(
            LlmFormat::parse("OPENAI-RESPONSES").unwrap(),
            LlmFormat::OPENAI_RESPONSES
        );
        assert_eq!(LlmFormat::parse("Ollama").unwrap(), LlmFormat::OLLAMA);
        assert!(LlmFormat::is_valid("ollama"));
        assert!(!LlmFormat::is_valid("gemini"));
        assert!(LlmFormat::parse("gemini").is_err());
    }

    #[test]
    fn provider_name_defaults_to_anthropic() {
        assert_eq!(LlmFormat::provider_name("openai"), "openai");
        assert_eq!(
            LlmFormat::provider_name("openai-responses"),
            "openai-responses"
        );
        assert_eq!(LlmFormat::provider_name("ollama"), "ollama");
        assert_eq!(LlmFormat::provider_name("anthropic"), "anthropic");
        assert_eq!(LlmFormat::provider_name("bogus"), "anthropic");
    }

    #[test]
    fn only_cloud_formats_need_a_key() {
        assert!(LlmFormat::needs_api_key("openai"));
        assert!(LlmFormat::needs_api_key("anthropic"));
        assert!(LlmFormat::needs_api_key("openai-responses"));
        assert!(!LlmFormat::needs_api_key("ollama"));
    }

    #[test]
    fn backoff_ladder_matches_go() {
        assert_eq!(backoff_for(1).num_seconds(), 60);
        assert_eq!(backoff_for(2).num_seconds(), 300);
        assert_eq!(backoff_for(3).num_seconds(), 1800);
        assert_eq!(backoff_for(99).num_seconds(), 1800);
    }

    #[test]
    fn health_open_window() {
        let now = Utc::now();
        let open = LlmProfileHealth {
            profile_id: 1,
            fails: 0,
            trips: 1,
            open_until: Some(now + ChronoDuration::seconds(60)),
            last_error: "status 429".into(),
            last_at: now,
        };
        assert!(open.is_open_at(now));
        assert!(!open.is_open_at(now + ChronoDuration::seconds(61)));

        let closed = LlmProfileHealth {
            open_until: None,
            ..open
        };
        assert!(!closed.is_open_at(now));
    }

    #[test]
    fn retry_rules_clamp_and_inherit() {
        let rule = RetryRule {
            attempts: 99,
            interval_ms: -5,
        }
        .clamped();
        assert_eq!(rule.attempts, RetryRule::MAX_ATTEMPTS);
        assert_eq!(rule.interval_ms, 0);

        let fallback = RetryRule {
            attempts: 3,
            interval_ms: 500,
        };
        let inherited = RetryRule::default().or(fallback);
        assert_eq!(inherited, fallback);

        let override_ = RetryOverride {
            connect: RetryRule {
                attempts: 1,
                interval_ms: 10,
            },
            ..RetryOverride::default()
        }
        .clamped();
        assert_eq!(override_.connect.attempts, 1);
        assert_eq!(override_.stream.attempts, 0);
    }

    #[test]
    fn profile_redacts_api_key() {
        let p = LlmProfile {
            id: 7,
            name: "default".into(),
            format: LlmFormat::ANTHROPIC.into(),
            model: "claude".into(),
            api_key: Some("sk-super-secret".into()),
            streaming: true,
            ..LlmProfile::default()
        };
        let debug = format!("{p:?}");
        assert!(
            !debug.contains("sk-super-secret"),
            "debug leaked key: {debug}"
        );
        assert!(debug.contains("***"));

        let json = serde_json::to_value(p.redact()).unwrap();
        assert!(json.get("api_key").is_none());
        assert_eq!(json["name"], "default");
        assert_eq!(json["streaming"], true);
    }

    #[test]
    fn api_key_hint_uses_last_four() {
        assert_eq!(api_key_hint("sk-1234").as_deref(), Some("\u{2026}1234"));
        assert_eq!(api_key_hint("abc"), None);
    }
}
