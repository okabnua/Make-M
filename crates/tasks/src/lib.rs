//! Task data layer — behaviour-preserving port of ARTEX `db/tasks.go`,
//! `db/task_categories.go` and `db/task_templates.go`.
//!
//! The crate exposes [`TaskStore`], a thin wrapper over the shared
//! [`database::Db`] pool, together with the [`Task`], [`TaskCategory`] and
//! [`TaskTemplate`] models and the [`TaskStatus`] helpers.
//!
//! Timestamps are read as Postgres `text` (`timestamptz::text`) to match the
//! original `::text` scans and are carried as `String` in the models.
//! Runtime SQL is executed with `sqlx::query` / `sqlx::query_scalar`; no
//! compile-time query macros are used.

use database::Db;
use serde::{Deserialize, Serialize};
use sqlx::{PgConnection, QueryBuilder, Row};
use std::collections::HashSet;
use std::str::FromStr;

/// Status constants matching `db/constants.go` (`created`, `running`, ...).
pub mod status {
    pub const CREATED: &str = "created";
    pub const RUNNING: &str = "running";
    pub const PAUSED: &str = "paused";
    pub const DONE: &str = "done";
    pub const FAILED: &str = "failed";
    pub const TIMEOUT: &str = "timeout";
}

/// The task lifecycle status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TaskStatus {
    Created,
    Running,
    Paused,
    Done,
    Failed,
    Timeout,
}

impl TaskStatus {
    /// The string stored in `tasks.status`.
    pub const fn as_str(self) -> &'static str {
        match self {
            TaskStatus::Created => status::CREATED,
            TaskStatus::Running => status::RUNNING,
            TaskStatus::Paused => status::PAUSED,
            TaskStatus::Done => status::DONE,
            TaskStatus::Failed => status::FAILED,
            TaskStatus::Timeout => status::TIMEOUT,
        }
    }

    /// A terminal (finished) state: done / failed / timeout.
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            TaskStatus::Done | TaskStatus::Failed | TaskStatus::Timeout
        )
    }

    /// Parse a status string; `None` for unknown values.
    pub fn parse(s: &str) -> Option<TaskStatus> {
        match s {
            status::CREATED => Some(TaskStatus::Created),
            status::RUNNING => Some(TaskStatus::Running),
            status::PAUSED => Some(TaskStatus::Paused),
            status::DONE => Some(TaskStatus::Done),
            status::FAILED => Some(TaskStatus::Failed),
            status::TIMEOUT => Some(TaskStatus::Timeout),
            _ => None,
        }
    }
}

impl std::fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for TaskStatus {
    type Err = TaskError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        TaskStatus::parse(s).ok_or_else(|| TaskError::Invalid(format!("unknown task status {s:?}")))
    }
}

/// Port of `db.IsTerminal`.
pub fn is_terminal_status(status: &str) -> bool {
    matches!(status, status::DONE | status::FAILED | status::TIMEOUT)
}

/// Errors returned by [`TaskStore`].
#[derive(Debug, thiserror::Error)]
pub enum TaskError {
    #[error("sql: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("not found")]
    NotFound,
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("invalid: {0}")]
    Invalid(String),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

/// One row of the `tasks` registry.
///
/// Field names and JSON keys mirror the Go `db.Task` struct for the columns
/// listed in the port scope. Nullable columns are `Option`; timestamps are the
/// Postgres `text` rendering (matching the Go `::text` scans).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    pub id: i64,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category_id: Option<i64>,
    pub description: String,
    pub goal: String,
    pub exploration_id: i64,
    pub status: String,
    pub paused: bool,
    pub queued: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queued_at: Option<String>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub queue_mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub llm_profile_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_llm_profile_id: Option<i64>,
    pub timeout_seconds: i32,
    pub plan_heartbeat_seconds: i32,
    pub plan_quiet_mode: bool,
    pub coverage_enabled: bool,
    pub target_gate: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pinned_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_run_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deadline_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paused_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deleted_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
    pub end_reason: String,
    pub created_at: String,
    pub updated_at: String,
}

/// Options for [`TaskStore::list`].
#[derive(Debug, Clone, Default)]
pub struct ListTasksOptions {
    /// Restrict to one lifecycle status.
    pub status: Option<String>,
    /// Maximum number of rows (Postgres `LIMIT`).
    pub limit: Option<i64>,
    /// Number of rows to skip (Postgres `OFFSET`).
    pub offset: Option<i64>,
}

/// Aggregate count of live tasks per status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusCount {
    pub status: String,
    pub count: i64,
}

/// Task-level intercept rule (port of `db.TaskInterceptRuleInput`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterceptRuleInput {
    pub enabled: bool,
    pub action: String,
    pub kind: String,
    pub pattern: String,
    pub note: String,
}

/// Options for [`TaskStore::create_with_options`] (port of `TaskCreateOptions`).
#[derive(Debug, Clone, Default)]
pub struct TaskCreateOptions {
    pub name: String,
    pub category_id: Option<i64>,
    pub source_task_ids: Vec<i64>,
    pub company_ids: Vec<i64>,
    pub llm_profile_ids: Vec<i64>,
    pub timeout_seconds: i32,
    pub plan_heartbeat_seconds: i32,
    pub plan_quiet_mode: Option<bool>,
    pub coverage_enabled: Option<bool>,
    pub intercept_rules: Vec<InterceptRuleInput>,
    pub llm_rotate: Option<bool>,
    pub target_gate: Option<bool>,
}

/// A row of `task_categories`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskCategory {
    pub id: i64,
    pub name: String,
    #[serde(skip)]
    pub nkey: String,
    pub task_count: i64,
    pub created_at: String,
    pub updated_at: String,
}

/// A row of `task_templates`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskTemplate {
    pub id: i64,
    pub name: String,
    #[serde(skip)]
    pub nkey: String,
    pub description: String,
    pub goal: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category_id: Option<i64>,
    pub intercept_rules: Vec<InterceptRuleInput>,
    pub created_at: String,
    pub updated_at: String,
}

/// Create/update payload for a task template (port of `TaskTemplateInput`).
#[derive(Debug, Clone, Default)]
pub struct TaskTemplateInput {
    pub name: String,
    pub description: String,
    pub goal: String,
    pub category_id: Option<i64>,
    pub intercept_rules: Vec<InterceptRuleInput>,
}

/// Partial update for a task template (port of `TaskTemplatePatch`).
#[derive(Debug, Clone, Default)]
pub struct TaskTemplatePatch {
    pub name: Option<String>,
    pub description: Option<String>,
    pub goal: Option<String>,
    pub category_id: Option<i64>,
    pub set_category_id: bool,
    pub intercept_rules: Vec<InterceptRuleInput>,
    pub set_intercept_rules: bool,
}

/// Floor for the planner heartbeat interval (port of `MinPlanHeartbeatSeconds`).
pub const MIN_PLAN_HEARTBEAT_SECONDS: i32 = 600;
/// Maximum number of direct source tasks (port of `MaxTaskSourceCount`).
pub const MAX_TASK_SOURCE_COUNT: usize = 8;
/// Maximum number of company scopes (port of `MaxTaskCompanyCount`).
pub const MAX_TASK_COMPANY_COUNT: usize = 32;
/// Maximum task category name length in runes (port of `MaxTaskCategoryNameRunes`).
pub const MAX_TASK_CATEGORY_NAME_RUNES: usize = 80;
/// Maximum task template name length in runes (port of `MaxTaskTemplateNameRunes`).
pub const MAX_TASK_TEMPLATE_NAME_RUNES: usize = 120;
/// Maximum task template text length in runes (port of `MaxTaskTemplateTextRunes`).
pub const MAX_TASK_TEMPLATE_TEXT_RUNES: usize = 16000;

/// Advisory lock key serializing company scope mutations (port of
/// `companyScopeMutationLock`).
const COMPANY_SCOPE_MUTATION_LOCK: i64 = 7337741003;

/// Columns selected for every [`Task`] read. Timestamp columns are cast to
/// `text`; nullable values stay `NULL` and decode into `Option<String>`.
const TASK_COLS: &str = "id, COALESCE(name,'') AS name, category_id, description, goal, \
exploration_id, status, paused, queued, queued_at::text AS queued_at, \
COALESCE(queue_mode,'') AS queue_mode, llm_profile_id, active_llm_profile_id, \
COALESCE(timeout_seconds,0) AS timeout_seconds, \
COALESCE(plan_heartbeat_seconds,300) AS plan_heartbeat_seconds, \
COALESCE(plan_quiet_mode,true) AS plan_quiet_mode, \
COALESCE(coverage_enabled,true) AS coverage_enabled, \
COALESCE(target_gate,false) AS target_gate, pinned_at::text AS pinned_at, \
first_run_at::text AS first_run_at, deadline_at::text AS deadline_at, \
paused_at::text AS paused_at, archived_at::text AS archived_at, \
deleted_at::text AS deleted_at, completed_at::text AS completed_at, \
COALESCE(end_reason,'') AS end_reason, created_at::text AS created_at, \
updated_at::text AS updated_at";

/// Columns selected for every [`TaskCategory`] read.
const TASK_CATEGORY_COLS: &str = "category.id AS id, category.name AS name, \
category.nkey AS nkey, \
count(task.id) FILTER (WHERE task.deleted_at IS NULL) AS task_count, \
category.created_at::text AS created_at, category.updated_at::text AS updated_at";

/// Columns selected for every [`TaskTemplate`] read.
const TASK_TEMPLATE_COLS: &str = "id, name, nkey, description, goal, category_id, \
intercept_rules::text AS intercept_rules, created_at::text AS created_at, \
updated_at::text AS updated_at";

fn task_from_row(row: &sqlx::postgres::PgRow) -> Result<Task, TaskError> {
    Ok(Task {
        id: row.try_get("id")?,
        name: row.try_get("name")?,
        category_id: row.try_get("category_id")?,
        description: row.try_get("description")?,
        goal: row.try_get("goal")?,
        exploration_id: row.try_get("exploration_id")?,
        status: row.try_get("status")?,
        paused: row.try_get("paused")?,
        queued: row.try_get("queued")?,
        queued_at: row.try_get("queued_at")?,
        queue_mode: row.try_get("queue_mode")?,
        llm_profile_id: row.try_get("llm_profile_id")?,
        active_llm_profile_id: row.try_get("active_llm_profile_id")?,
        timeout_seconds: row.try_get("timeout_seconds")?,
        plan_heartbeat_seconds: row.try_get("plan_heartbeat_seconds")?,
        plan_quiet_mode: row.try_get("plan_quiet_mode")?,
        coverage_enabled: row.try_get("coverage_enabled")?,
        target_gate: row.try_get("target_gate")?,
        pinned_at: row.try_get("pinned_at")?,
        first_run_at: row.try_get("first_run_at")?,
        deadline_at: row.try_get("deadline_at")?,
        paused_at: row.try_get("paused_at")?,
        archived_at: row.try_get("archived_at")?,
        deleted_at: row.try_get("deleted_at")?,
        completed_at: row.try_get("completed_at")?,
        end_reason: row.try_get("end_reason")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn task_category_from_row(row: &sqlx::postgres::PgRow) -> Result<TaskCategory, TaskError> {
    Ok(TaskCategory {
        id: row.try_get("id")?,
        name: row.try_get("name")?,
        nkey: row.try_get("nkey")?,
        task_count: row.try_get("task_count")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn task_template_from_row(row: &sqlx::postgres::PgRow) -> Result<TaskTemplate, TaskError> {
    let rules_text: String = row.try_get("intercept_rules")?;
    // JSON `null` (and an empty column) mean "no rules", matching Go's
    // json.Unmarshal into a nil slice.
    let intercept_rules: Vec<InterceptRuleInput> =
        if rules_text.trim().is_empty() || rules_text.trim() == "null" {
            Vec::new()
        } else {
            serde_json::from_str(&rules_text)?
        };
    Ok(TaskTemplate {
        id: row.try_get("id")?,
        name: row.try_get("name")?,
        nkey: row.try_get("nkey")?,
        description: row.try_get("description")?,
        goal: row.try_get("goal")?,
        category_id: row.try_get("category_id")?,
        intercept_rules,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn is_unique_violation(err: &sqlx::Error) -> bool {
    if let sqlx::Error::Database(db) = err {
        return db.code().as_deref() == Some("23505");
    }
    false
}

/// Normalize display whitespace: collapse runs of whitespace to a single space
/// (port of `strings.Join(strings.Fields(name), " ")`).
fn normalize_name(name: &str) -> String {
    name.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Normalize a task category name and its case-insensitive `nkey`.
fn normalize_category_name(name: &str) -> Result<(String, String), TaskError> {
    let name = normalize_name(name);
    if name.is_empty() {
        return Err(TaskError::Invalid("name is required".into()));
    }
    if name.chars().count() > MAX_TASK_CATEGORY_NAME_RUNES {
        return Err(TaskError::Invalid(format!(
            "name exceeds {MAX_TASK_CATEGORY_NAME_RUNES} characters"
        )));
    }
    let nkey = name.to_lowercase();
    Ok((name, nkey))
}

/// Port of `normalizeHeartbeat`: anything below the floor is raised to it.
fn normalize_heartbeat(sec: i32) -> i32 {
    if sec < MIN_PLAN_HEARTBEAT_SECONDS {
        MIN_PLAN_HEARTBEAT_SECONDS
    } else {
        sec
    }
}

/// Port of `NormalizeTaskCompanyIDs`: validate, dedupe, preserve first order.
fn normalize_company_ids(ids: &[i64]) -> Result<Vec<i64>, TaskError> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut seen = HashSet::with_capacity(ids.len().min(MAX_TASK_COMPANY_COUNT));
    let mut normalized = Vec::with_capacity(ids.len().min(MAX_TASK_COMPANY_COUNT));
    for &id in ids {
        if id <= 0 {
            return Err(TaskError::Invalid("company id must be positive".into()));
        }
        if !seen.insert(id) {
            continue;
        }
        normalized.push(id);
        if normalized.len() > MAX_TASK_COMPANY_COUNT {
            return Err(TaskError::Invalid(format!(
                "got more than {MAX_TASK_COMPANY_COUNT} unique companies"
            )));
        }
    }
    Ok(normalized)
}

fn normalize_template_input(
    input: TaskTemplateInput,
) -> Result<(TaskTemplateInput, String), TaskError> {
    let name = normalize_name(&input.name);
    let description = input.description.trim().to_string();
    let goal = input.goal.trim().to_string();
    if name.is_empty() {
        return Err(TaskError::Invalid("name is required".into()));
    }
    if name.chars().count() > MAX_TASK_TEMPLATE_NAME_RUNES {
        return Err(TaskError::Invalid(format!(
            "name exceeds {MAX_TASK_TEMPLATE_NAME_RUNES} characters"
        )));
    }
    if description.is_empty() {
        return Err(TaskError::Invalid("description is required".into()));
    }
    if description.chars().count() > MAX_TASK_TEMPLATE_TEXT_RUNES {
        return Err(TaskError::Invalid(format!(
            "description exceeds {MAX_TASK_TEMPLATE_TEXT_RUNES} characters"
        )));
    }
    if goal.is_empty() {
        return Err(TaskError::Invalid("goal is required".into()));
    }
    if goal.chars().count() > MAX_TASK_TEMPLATE_TEXT_RUNES {
        return Err(TaskError::Invalid(format!(
            "goal exceeds {MAX_TASK_TEMPLATE_TEXT_RUNES} characters"
        )));
    }
    let nkey = name.to_lowercase();
    Ok((
        TaskTemplateInput {
            name,
            description,
            goal,
            category_id: input.category_id,
            intercept_rules: input.intercept_rules,
        },
        nkey,
    ))
}

fn normalize_template_patch(
    mut patch: TaskTemplatePatch,
) -> Result<(TaskTemplatePatch, Option<String>), TaskError> {
    if patch.name.is_none()
        && patch.description.is_none()
        && patch.goal.is_none()
        && !patch.set_category_id
        && !patch.set_intercept_rules
    {
        return Err(TaskError::Invalid("no fields supplied".into()));
    }
    let mut nkey = None;
    if let Some(name) = patch.name.take() {
        let name = normalize_name(&name);
        if name.is_empty() {
            return Err(TaskError::Invalid("name is required".into()));
        }
        if name.chars().count() > MAX_TASK_TEMPLATE_NAME_RUNES {
            return Err(TaskError::Invalid(format!(
                "name exceeds {MAX_TASK_TEMPLATE_NAME_RUNES} characters"
            )));
        }
        nkey = Some(name.to_lowercase());
        patch.name = Some(name);
    }
    if let Some(description) = patch.description.take() {
        let description = description.trim().to_string();
        if description.is_empty() {
            return Err(TaskError::Invalid("description is required".into()));
        }
        if description.chars().count() > MAX_TASK_TEMPLATE_TEXT_RUNES {
            return Err(TaskError::Invalid(format!(
                "description exceeds {MAX_TASK_TEMPLATE_TEXT_RUNES} characters"
            )));
        }
        patch.description = Some(description);
    }
    if let Some(goal) = patch.goal.take() {
        let goal = goal.trim().to_string();
        if goal.is_empty() {
            return Err(TaskError::Invalid("goal is required".into()));
        }
        if goal.chars().count() > MAX_TASK_TEMPLATE_TEXT_RUNES {
            return Err(TaskError::Invalid(format!(
                "goal exceeds {MAX_TASK_TEMPLATE_TEXT_RUNES} characters"
            )));
        }
        patch.goal = Some(goal);
    }
    Ok((patch, nkey))
}

/// Port of `normalizeRuleAction`.
fn normalize_rule_action(action: &str) -> &str {
    if action == "allow" {
        "allow"
    } else {
        "block"
    }
}

/// Marshal intercept rules to a JSON array (never `null`).
fn marshal_rules(rules: &[InterceptRuleInput]) -> Result<String, TaskError> {
    if rules.is_empty() {
        Ok("[]".to_string())
    } else {
        Ok(serde_json::to_string(rules)?)
    }
}

/// Data access for tasks, categories and templates.
#[derive(Clone)]
pub struct TaskStore {
    db: Db,
}

impl TaskStore {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    /// Access the shared database handle.
    pub fn db(&self) -> &Db {
        &self.db
    }

    /// List alive tasks with pinned first, then newest ids
    /// (port of `ListTasks`, plus an optional status filter and pagination).
    pub async fn list(&self, opts: &ListTasksOptions) -> Result<Vec<Task>, TaskError> {
        if let Some(limit) = opts.limit {
            if limit < 0 {
                return Err(TaskError::Invalid("limit must be non-negative".into()));
            }
        }
        if let Some(offset) = opts.offset {
            if offset < 0 {
                return Err(TaskError::Invalid("offset must be non-negative".into()));
            }
        }

        let mut qb: QueryBuilder<sqlx::Postgres> = QueryBuilder::new(format!(
            "SELECT {TASK_COLS} FROM tasks WHERE deleted_at IS NULL"
        ));
        if let Some(status) = opts.status.as_deref() {
            qb.push(" AND status = ").push_bind(status.to_string());
        }
        qb.push(" ORDER BY (pinned_at IS NOT NULL) DESC, pinned_at DESC NULLS LAST, id DESC");
        if let Some(limit) = opts.limit {
            qb.push(" LIMIT ").push_bind(limit);
        }
        if let Some(offset) = opts.offset {
            qb.push(" OFFSET ").push_bind(offset);
        }
        let rows = qb.build().fetch_all(self.db.pool()).await?;
        rows.iter().map(task_from_row).collect()
    }

    /// Return one alive task, or `None` when it is missing/deleted
    /// (port of `GetTask`).
    pub async fn get(&self, id: i64) -> Result<Option<Task>, TaskError> {
        let sql = format!("SELECT {TASK_COLS} FROM tasks WHERE id=$1 AND deleted_at IS NULL");
        let row = sqlx::query(&sql)
            .bind(id)
            .fetch_optional(self.db.pool())
            .await?;
        row.as_ref().map(task_from_row).transpose()
    }

    /// Create a task with a single optional LLM profile
    /// (port of `CreateTask`).
    pub async fn create(
        &self,
        description: &str,
        goal: &str,
        llm_profile_id: Option<i64>,
        timeout_seconds: i32,
        plan_heartbeat_seconds: i32,
    ) -> Result<Task, TaskError> {
        let llm_profile_ids = llm_profile_id.into_iter().collect::<Vec<_>>();
        self.create_with_options(
            description,
            goal,
            TaskCreateOptions {
                llm_profile_ids,
                timeout_seconds,
                plan_heartbeat_seconds,
                ..Default::default()
            },
        )
        .await
    }

    /// Create an exploration, its origin fact and the task atomically
    /// (port of `CreateTaskWithOptions`).
    pub async fn create_with_options(
        &self,
        description: &str,
        goal: &str,
        opts: TaskCreateOptions,
    ) -> Result<Task, TaskError> {
        if opts.source_task_ids.len() > MAX_TASK_SOURCE_COUNT {
            return Err(TaskError::Invalid(format!(
                "too many source tasks: got {}, maximum is {MAX_TASK_SOURCE_COUNT}",
                opts.source_task_ids.len()
            )));
        }
        let company_ids = normalize_company_ids(&opts.company_ids)?;

        let mut tx = self.db.pool().begin().await?;

        let exp_id: i64 = sqlx::query_scalar(
            "INSERT INTO explorations(description, goal) VALUES ($1,$2) RETURNING id",
        )
        .bind(description)
        .bind(goal)
        .fetch_one(&mut *tx)
        .await?;

        let origin_payload = serde_json::json!({
            "summary": format!("Task origin: {description}; Goal: {goal}"),
            "description": description,
            "goal": goal,
        });
        let origin_payload = serde_json::to_string(&origin_payload)?;
        sqlx::query(
            "INSERT INTO exploration_nodes(exploration_id, kind, payload, priority, state, origin) \
             VALUES ($1, 'fact', $2::jsonb, 0, 'origin', 'system')",
        )
        .bind(exp_id)
        .bind(origin_payload)
        .execute(&mut *tx)
        .await?;

        let timeout_seconds = if opts.timeout_seconds < 0 {
            0
        } else {
            opts.timeout_seconds
        };
        let plan_heartbeat_seconds = normalize_heartbeat(opts.plan_heartbeat_seconds);
        let coverage_enabled = opts.coverage_enabled.unwrap_or(true);

        if let Some(category_id) = opts.category_id {
            if category_id <= 0 {
                return Err(TaskError::Invalid("category id must be positive".into()));
            }
            let exists: Option<String> =
                sqlx::query_scalar("SELECT name FROM task_categories WHERE id=$1")
                    .bind(category_id)
                    .fetch_optional(&mut *tx)
                    .await?;
            if exists.is_none() {
                return Err(TaskError::NotFound);
            }
        }

        let active_profile_id = opts.llm_profile_ids.first().copied();
        let plan_quiet_mode = opts.plan_quiet_mode.unwrap_or(true);
        let llm_rotate = opts.llm_rotate.unwrap_or(false);
        let target_gate = opts.target_gate.unwrap_or(false);

        let task_id: i64 = sqlx::query_scalar(
            "INSERT INTO tasks(name, category_id, description, goal, exploration_id, \
             llm_profile_id, active_llm_profile_id, timeout_seconds, plan_heartbeat_seconds, \
             plan_quiet_mode, coverage_enabled, llm_rotate, target_gate) \
             VALUES ($1,$2,$3,$4,$5,$6,$6,$7,$8,$9,$10,$11,$12) RETURNING id",
        )
        .bind(&opts.name)
        .bind(opts.category_id)
        .bind(description)
        .bind(goal)
        .bind(exp_id)
        .bind(active_profile_id)
        .bind(timeout_seconds)
        .bind(plan_heartbeat_seconds)
        .bind(plan_quiet_mode)
        .bind(coverage_enabled)
        .bind(llm_rotate)
        .bind(target_gate)
        .fetch_one(&mut *tx)
        .await?;

        insert_task_relations(&mut tx, task_id, &opts.source_task_ids).await?;
        insert_task_companies(&mut tx, task_id, &company_ids).await?;
        insert_task_llm_profiles(&mut tx, task_id, &opts.llm_profile_ids).await?;
        insert_task_intercept_rules(&mut tx, task_id, &opts.intercept_rules).await?;

        tx.commit().await?;

        self.get(task_id).await?.ok_or(TaskError::NotFound)
    }

    /// Update a task's lifecycle status, stamping/clearing `completed_at`
    /// (port of `SetStatus`).
    pub async fn update_status(&self, id: i64, status: &str) -> Result<(), TaskError> {
        sqlx::query(
            "UPDATE tasks \
             SET status = $1, \
                 completed_at = CASE WHEN $1 IN ('done','failed','timeout') \
                     THEN COALESCE(completed_at, now()) ELSE NULL END, \
                 end_reason = CASE WHEN $1 IN ('done','failed','timeout') \
                     THEN end_reason ELSE '' END \
             WHERE id = $2",
        )
        .bind(status)
        .bind(id)
        .execute(self.db.pool())
        .await?;
        Ok(())
    }

    /// Mark a task deleted (`deleted_at = now()`). Returns `true` when a live
    /// row was updated. The original hard delete lives in the archive path.
    pub async fn soft_delete(&self, id: i64) -> Result<bool, TaskError> {
        let result =
            sqlx::query("UPDATE tasks SET deleted_at = now() WHERE id = $1 AND deleted_at IS NULL")
                .bind(id)
                .execute(self.db.pool())
                .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Count live tasks grouped by status, ordered by status
    /// (port of the status aggregation used by the task list).
    pub async fn count_by_status(&self) -> Result<Vec<StatusCount>, TaskError> {
        let rows = sqlx::query(
            "SELECT status, count(*) AS count FROM tasks \
             WHERE deleted_at IS NULL GROUP BY status ORDER BY status",
        )
        .fetch_all(self.db.pool())
        .await?;
        rows.iter()
            .map(|row| {
                Ok(StatusCount {
                    status: row.try_get("status")?,
                    count: row.try_get("count")?,
                })
            })
            .collect()
    }

    /// List every task category with its live task count
    /// (port of `ListTaskCategories`).
    pub async fn list_task_categories(&self) -> Result<Vec<TaskCategory>, TaskError> {
        let sql = format!(
            "SELECT {TASK_CATEGORY_COLS} FROM task_categories category \
             LEFT JOIN tasks task ON task.category_id=category.id \
             GROUP BY category.id ORDER BY category.name, category.id"
        );
        let rows = sqlx::query(&sql).fetch_all(self.db.pool()).await?;
        rows.iter().map(task_category_from_row).collect()
    }

    /// Return one task category, or `None` (port of `GetTaskCategory`).
    pub async fn get_task_category(&self, id: i64) -> Result<Option<TaskCategory>, TaskError> {
        let sql = format!(
            "SELECT {TASK_CATEGORY_COLS} FROM task_categories category \
             LEFT JOIN tasks task ON task.category_id=category.id \
             WHERE category.id=$1 GROUP BY category.id"
        );
        let row = sqlx::query(&sql)
            .bind(id)
            .fetch_optional(self.db.pool())
            .await?;
        row.as_ref().map(task_category_from_row).transpose()
    }

    /// Create a category; a visually equivalent name conflicts
    /// (port of `CreateTaskCategory`).
    pub async fn create_task_category(&self, name: &str) -> Result<TaskCategory, TaskError> {
        let (name, nkey) = normalize_category_name(name)?;
        let row = sqlx::query(
            "WITH inserted AS ( \
                 INSERT INTO task_categories(name, nkey) VALUES ($1,$2) \
                 ON CONFLICT (nkey) DO NOTHING \
                 RETURNING id, name, nkey, created_at, updated_at \
             ) \
             SELECT inserted.id, inserted.name, inserted.nkey, 0::bigint AS task_count, \
                    inserted.created_at::text AS created_at, \
                    inserted.updated_at::text AS updated_at \
             FROM inserted",
        )
        .bind(name)
        .bind(nkey)
        .fetch_optional(self.db.pool())
        .await?;
        let row = row.ok_or(TaskError::Conflict(
            "task category name already exists".into(),
        ))?;
        task_category_from_row(&row)
    }

    /// Rename a category (port of `RenameTaskCategory`).
    pub async fn rename_task_category(
        &self,
        id: i64,
        name: &str,
    ) -> Result<TaskCategory, TaskError> {
        let (name, nkey) = normalize_category_name(name)?;
        let result = sqlx::query(
            "WITH updated AS ( \
                 UPDATE task_categories SET name=$2, nkey=$3 WHERE id=$1 \
                 RETURNING id, name, nkey, created_at, updated_at \
             ) \
             SELECT updated.id, updated.name, updated.nkey, \
                    (SELECT count(*) FROM tasks \
                     WHERE category_id=updated.id AND deleted_at IS NULL) AS task_count, \
                    updated.created_at::text AS created_at, \
                    updated.updated_at::text AS updated_at \
             FROM updated",
        )
        .bind(id)
        .bind(name)
        .bind(nkey)
        .fetch_optional(self.db.pool())
        .await;
        match result {
            Ok(Some(row)) => task_category_from_row(&row),
            Ok(None) => Err(TaskError::NotFound),
            Err(e) if is_unique_violation(&e) => Err(TaskError::Conflict(
                "task category name already exists".into(),
            )),
            Err(e) => Err(e.into()),
        }
    }

    /// Delete a category; affected tasks become uncategorized
    /// (port of `DeleteTaskCategory`). Returns whether a row was deleted.
    pub async fn delete_task_category(&self, id: i64) -> Result<bool, TaskError> {
        let result = sqlx::query("DELETE FROM task_categories WHERE id=$1")
            .bind(id)
            .execute(self.db.pool())
            .await?;
        Ok(result.rows_affected() > 0)
    }

    /// List every template, most recently maintained first
    /// (port of `ListTaskTemplates`).
    pub async fn list_task_templates(&self) -> Result<Vec<TaskTemplate>, TaskError> {
        let sql = format!(
            "SELECT {TASK_TEMPLATE_COLS} FROM task_templates ORDER BY updated_at DESC, id DESC"
        );
        let rows = sqlx::query(&sql).fetch_all(self.db.pool()).await?;
        rows.iter().map(task_template_from_row).collect()
    }

    /// Return one template, or `None` (port of `GetTaskTemplate`).
    pub async fn get_task_template(&self, id: i64) -> Result<Option<TaskTemplate>, TaskError> {
        let sql = format!("SELECT {TASK_TEMPLATE_COLS} FROM task_templates WHERE id=$1");
        let row = sqlx::query(&sql)
            .bind(id)
            .fetch_optional(self.db.pool())
            .await?;
        row.as_ref().map(task_template_from_row).transpose()
    }

    /// Create a template; a visually equivalent name conflicts
    /// (port of `CreateTaskTemplate`).
    pub async fn create_task_template(
        &self,
        input: TaskTemplateInput,
    ) -> Result<TaskTemplate, TaskError> {
        let (input, nkey) = normalize_template_input(input)?;
        let rules_json = marshal_rules(&input.intercept_rules)?;
        let row = sqlx::query(&format!(
            "INSERT INTO task_templates(name, nkey, description, goal, category_id, intercept_rules) \
             VALUES ($1,$2,$3,$4,$5,$6::jsonb) \
             ON CONFLICT (nkey) DO NOTHING \
             RETURNING {TASK_TEMPLATE_COLS}"
        ))
        .bind(input.name)
        .bind(nkey)
        .bind(input.description)
        .bind(input.goal)
        .bind(input.category_id)
        .bind(rules_json)
        .fetch_optional(self.db.pool())
        .await?;
        let row = row.ok_or(TaskError::Conflict(
            "task template name already exists".into(),
        ))?;
        task_template_from_row(&row)
    }

    /// Replace the editable fields of a template (port of `UpdateTaskTemplate`).
    pub async fn update_task_template(
        &self,
        id: i64,
        input: TaskTemplateInput,
    ) -> Result<TaskTemplate, TaskError> {
        let (input, _) = normalize_template_input(input)?;
        self.patch_task_template(
            id,
            TaskTemplatePatch {
                name: Some(input.name),
                description: Some(input.description),
                goal: Some(input.goal),
                ..Default::default()
            },
        )
        .await
    }

    /// Atomically change only the supplied fields
    /// (port of `PatchTaskTemplate`).
    pub async fn patch_task_template(
        &self,
        id: i64,
        patch: TaskTemplatePatch,
    ) -> Result<TaskTemplate, TaskError> {
        let (patch, nkey) = normalize_template_patch(patch)?;
        let rules_json = marshal_rules(&patch.intercept_rules)?;
        let result = sqlx::query(&format!(
            "UPDATE task_templates \
             SET name=CASE WHEN $2 THEN $3::text ELSE name END, \
                 nkey=CASE WHEN $2 THEN $4::text ELSE nkey END, \
                 description=CASE WHEN $5 THEN $6::text ELSE description END, \
                 goal=CASE WHEN $7 THEN $8::text ELSE goal END, \
                 category_id=CASE WHEN $9 THEN $10::bigint ELSE category_id END, \
                 intercept_rules=CASE WHEN $11 THEN $12::jsonb ELSE intercept_rules END \
             WHERE id=$1 RETURNING {TASK_TEMPLATE_COLS}"
        ))
        .bind(id)
        .bind(patch.name.is_some())
        .bind(patch.name)
        .bind(nkey)
        .bind(patch.description.is_some())
        .bind(patch.description)
        .bind(patch.goal.is_some())
        .bind(patch.goal)
        .bind(patch.set_category_id)
        .bind(patch.category_id)
        .bind(patch.set_intercept_rules)
        .bind(rules_json)
        .fetch_optional(self.db.pool())
        .await;
        match result {
            Ok(Some(row)) => task_template_from_row(&row),
            Ok(None) => Err(TaskError::NotFound),
            Err(e) if is_unique_violation(&e) => Err(TaskError::Conflict(
                "task template name already exists".into(),
            )),
            Err(e) => Err(e.into()),
        }
    }

    /// Delete a template; returns whether a row was deleted
    /// (port of `DeleteTaskTemplate`).
    pub async fn delete_task_template(&self, id: i64) -> Result<bool, TaskError> {
        let result = sqlx::query("DELETE FROM task_templates WHERE id=$1")
            .bind(id)
            .execute(self.db.pool())
            .await?;
        Ok(result.rows_affected() > 0)
    }
}

async fn insert_task_relations(
    conn: &mut PgConnection,
    task_id: i64,
    source_ids: &[i64],
) -> Result<(), TaskError> {
    let mut seen = HashSet::with_capacity(source_ids.len());
    for &source_id in source_ids {
        if source_id <= 0 || source_id == task_id || !seen.insert(source_id) {
            return Err(TaskError::Invalid(format!(
                "invalid or duplicate source task id {source_id}"
            )));
        }
        let affected = sqlx::query(
            "INSERT INTO task_relations(task_id, source_task_id) \
             SELECT $1, id FROM tasks WHERE id=$2 AND deleted_at IS NULL",
        )
        .bind(task_id)
        .bind(source_id)
        .execute(&mut *conn)
        .await?
        .rows_affected();
        if affected != 1 {
            return Err(TaskError::Conflict(format!(
                "source task {source_id} not found"
            )));
        }
    }
    Ok(())
}

async fn insert_task_companies(
    conn: &mut PgConnection,
    task_id: i64,
    company_ids: &[i64],
) -> Result<(), TaskError> {
    if company_ids.is_empty() {
        return Ok(());
    }
    // Serialize company scope edits so the creation-time asset snapshot matches
    // the committed attribution state.
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(COMPANY_SCOPE_MUTATION_LOCK)
        .execute(&mut *conn)
        .await?;

    let inserted: i64 = sqlx::query_scalar(
        "WITH requested(company_id, position) AS ( \
             SELECT company_id, position \
             FROM unnest($2::bigint[]) WITH ORDINALITY AS requested(company_id, position) \
         ), inserted AS ( \
             INSERT INTO task_scope(task_id, kind, company_id, source, reason) \
             SELECT $1, 'company', companies.id, 'manual', \
                    'associated with company at task creation' \
             FROM requested \
             JOIN companies ON companies.id=requested.company_id \
             ORDER BY requested.position \
             RETURNING company_id \
         ) \
         SELECT count(*) FROM inserted",
    )
    .bind(task_id)
    .bind(company_ids.to_vec())
    .fetch_one(&mut *conn)
    .await?;
    if inserted != company_ids.len() as i64 {
        return Err(TaskError::Conflict(
            "one or more companies do not exist".into(),
        ));
    }

    sqlx::query(
        "UPDATE assets \
         SET task_ids=CASE \
             WHEN $1=ANY(task_ids) THEN task_ids \
             ELSE array_append(task_ids, $1) \
         END \
         WHERE company_id=ANY($2::bigint[])",
    )
    .bind(task_id)
    .bind(company_ids.to_vec())
    .execute(&mut *conn)
    .await?;

    sqlx::query(
        "INSERT INTO task_asset_links(task_id, asset_id, source, source_summary) \
         SELECT $1, asset.id, $3, 'associated with company at task creation: ' || company.name \
         FROM assets asset \
         JOIN companies company ON company.id=asset.company_id \
         WHERE asset.company_id=ANY($2::bigint[]) \
           AND $1=ANY(asset.task_ids) \
         ON CONFLICT (task_id, asset_id) DO UPDATE \
         SET source=EXCLUDED.source, \
             source_summary=EXCLUDED.source_summary, \
             source_node_id=NULL",
    )
    .bind(task_id)
    .bind(company_ids.to_vec())
    .bind("company")
    .execute(&mut *conn)
    .await?;
    Ok(())
}

async fn insert_task_llm_profiles(
    conn: &mut PgConnection,
    task_id: i64,
    profile_ids: &[i64],
) -> Result<(), TaskError> {
    let mut seen = HashSet::with_capacity(profile_ids.len());
    for (position, &profile_id) in profile_ids.iter().enumerate() {
        if profile_id <= 0 || !seen.insert(profile_id) {
            return Err(TaskError::Invalid(format!(
                "invalid or duplicate LLM profile id {profile_id}"
            )));
        }
        sqlx::query(
            "INSERT INTO task_llm_profiles(task_id, profile_id, position) VALUES ($1,$2,$3)",
        )
        .bind(task_id)
        .bind(profile_id)
        .bind(position as i32)
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

async fn insert_task_intercept_rules(
    conn: &mut PgConnection,
    task_id: i64,
    rules: &[InterceptRuleInput],
) -> Result<(), TaskError> {
    for rule in rules {
        sqlx::query(
            "INSERT INTO task_intercept_rules(task_id, enabled, action, kind, pattern, note) \
             VALUES ($1,$2,$3,$4,$5,$6)",
        )
        .bind(task_id)
        .bind(rule.enabled)
        .bind(normalize_rule_action(&rule.action))
        .bind(&rule.kind)
        .bind(&rule.pattern)
        .bind(&rule.note)
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_round_trip() {
        for status in [
            TaskStatus::Created,
            TaskStatus::Running,
            TaskStatus::Paused,
            TaskStatus::Done,
            TaskStatus::Failed,
            TaskStatus::Timeout,
        ] {
            assert_eq!(TaskStatus::parse(status.as_str()), Some(status));
            assert_eq!(status.to_string(), status.as_str());
        }
    }

    #[test]
    fn status_constants_match_go() {
        assert_eq!(status::CREATED, "created");
        assert_eq!(status::RUNNING, "running");
        assert_eq!(status::PAUSED, "paused");
        assert_eq!(status::DONE, "done");
        assert_eq!(status::FAILED, "failed");
        assert_eq!(status::TIMEOUT, "timeout");
    }

    #[test]
    fn status_terminal() {
        assert!(TaskStatus::Done.is_terminal());
        assert!(TaskStatus::Failed.is_terminal());
        assert!(TaskStatus::Timeout.is_terminal());
        assert!(!TaskStatus::Created.is_terminal());
        assert!(!TaskStatus::Running.is_terminal());
        assert!(!TaskStatus::Paused.is_terminal());

        assert!(is_terminal_status("done"));
        assert!(is_terminal_status("failed"));
        assert!(is_terminal_status("timeout"));
        assert!(!is_terminal_status("running"));
        assert!(!is_terminal_status("created"));
    }

    #[test]
    fn status_unknown() {
        assert_eq!(TaskStatus::parse("nope"), None);
        assert!(TaskStatus::from_str("nope").is_err());
        assert!(TaskStatus::from_str("done").is_ok());
    }

    #[test]
    fn normalize_heartbeat_floor() {
        assert_eq!(normalize_heartbeat(0), MIN_PLAN_HEARTBEAT_SECONDS);
        assert_eq!(normalize_heartbeat(-5), MIN_PLAN_HEARTBEAT_SECONDS);
        assert_eq!(normalize_heartbeat(599), MIN_PLAN_HEARTBEAT_SECONDS);
        assert_eq!(normalize_heartbeat(600), 600);
        assert_eq!(normalize_heartbeat(900), 900);
    }

    #[test]
    fn category_name_normalization() {
        let (name, nkey) = normalize_category_name("  Foo   Bar  ").unwrap();
        assert_eq!(name, "Foo Bar");
        assert_eq!(nkey, "foo bar");
        assert!(normalize_category_name("   ").is_err());
        let long = "a".repeat(MAX_TASK_CATEGORY_NAME_RUNES + 1);
        assert!(normalize_category_name(&long).is_err());
        let exact = "a".repeat(MAX_TASK_CATEGORY_NAME_RUNES);
        assert!(normalize_category_name(&exact).is_ok());
    }

    #[test]
    fn template_input_normalization() {
        let (input, nkey) = normalize_template_input(TaskTemplateInput {
            name: "  My  Template ".into(),
            description: "  desc  ".into(),
            goal: " goal ".into(),
            category_id: Some(3),
            intercept_rules: vec![],
        })
        .unwrap();
        assert_eq!(input.name, "My Template");
        assert_eq!(input.description, "desc");
        assert_eq!(input.goal, "goal");
        assert_eq!(input.category_id, Some(3));
        assert_eq!(nkey, "my template");

        assert!(normalize_template_input(TaskTemplateInput {
            name: "x".into(),
            description: "".into(),
            goal: "g".into(),
            category_id: None,
            intercept_rules: vec![],
        })
        .is_err());
        assert!(normalize_template_input(TaskTemplateInput {
            name: "x".into(),
            description: "d".into(),
            goal: "".into(),
            category_id: None,
            intercept_rules: vec![],
        })
        .is_err());
    }

    #[test]
    fn template_patch_requires_fields() {
        assert!(normalize_template_patch(TaskTemplatePatch::default()).is_err());
        let (patch, nkey) = normalize_template_patch(TaskTemplatePatch {
            name: Some("  New  Name ".into()),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(patch.name.as_deref(), Some("New Name"));
        assert_eq!(nkey.as_deref(), Some("new name"));
    }

    #[test]
    fn company_ids_normalization() {
        assert_eq!(normalize_company_ids(&[]).unwrap(), Vec::<i64>::new());
        assert_eq!(normalize_company_ids(&[1, 2, 2, 3]).unwrap(), vec![1, 2, 3]);
        assert!(normalize_company_ids(&[0]).is_err());
        assert!(normalize_company_ids(&[-1]).is_err());
        let too_many: Vec<i64> = (1..=MAX_TASK_COMPANY_COUNT as i64 + 1).collect();
        assert!(normalize_company_ids(&too_many).is_err());
    }

    #[test]
    fn rule_action_normalization() {
        assert_eq!(normalize_rule_action("allow"), "allow");
        assert_eq!(normalize_rule_action("block"), "block");
        assert_eq!(normalize_rule_action(""), "block");
        assert_eq!(normalize_rule_action("ALLOW"), "block");
    }

    #[test]
    fn marshal_rules_is_array() {
        assert_eq!(marshal_rules(&[]).unwrap(), "[]");
        let rules = vec![InterceptRuleInput {
            enabled: true,
            action: "allow".into(),
            kind: "exact_domain".into(),
            pattern: "example.com".into(),
            note: "n".into(),
        }];
        let json = marshal_rules(&rules).unwrap();
        assert!(json.starts_with('['));
        let parsed: Vec<InterceptRuleInput> = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, rules);
    }
}
