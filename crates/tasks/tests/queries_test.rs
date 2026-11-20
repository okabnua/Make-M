//! Integration test for the task data layer against a real Postgres.
//! Requires `ARTEX_PG_DSN` (or the local test container) and skips gracefully
//! when Postgres is unreachable.

use database::open;
use tasks::{
    InterceptRuleInput, ListTasksOptions, TaskCreateOptions, TaskStatus, TaskStore,
    TaskTemplateInput,
};
use tokio::sync::Mutex;

/// Serializes the integration tests so they do not interfere through the shared
/// `itest_%` rows on one Postgres instance.
static LOCK: Mutex<()> = Mutex::const_new(());

fn dsn() -> Option<String> {
    std::env::var("ARTEX_PG_DSN")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| Some("postgres://artex:artex@127.0.0.1:55432/artex?sslmode=disable".into()))
}

async fn cleanup(db: &database::Db) {
    for sql in [
        "DELETE FROM tasks WHERE name LIKE 'itest_%'",
        "DELETE FROM task_templates WHERE name LIKE 'itest_%'",
        "DELETE FROM task_categories WHERE name LIKE 'itest_%'",
    ] {
        sqlx::query(sql)
            .execute(db.pool())
            .await
            .expect("cleanup query");
    }
}

#[tokio::test]
async fn task_crud_round_trip() {
    let _guard = LOCK.lock().await;
    let Some(dsn) = dsn() else { return };
    let db = match open(&dsn).await {
        Ok(db) => db,
        Err(e) => {
            eprintln!("skipping: postgres unavailable ({e})");
            return;
        }
    };
    cleanup(&db).await;
    let store = TaskStore::new(db.clone());

    // Categories.
    let category = store
        .create_task_category("itest category")
        .await
        .expect("create category");
    assert_eq!(category.name, "itest category");
    assert_eq!(category.nkey, "itest category");
    assert_eq!(category.task_count, 0);

    // Create a task in that category.
    let created = store
        .create_with_options(
            "itest description",
            "itest goal",
            TaskCreateOptions {
                name: "itest create".into(),
                category_id: Some(category.id),
                timeout_seconds: -1,
                plan_heartbeat_seconds: 0,
                ..Default::default()
            },
        )
        .await
        .expect("create task");
    assert!(created.id > 0);
    assert!(created.exploration_id > 0);
    assert_eq!(created.name, "itest create");
    assert_eq!(created.category_id, Some(category.id));
    assert_eq!(created.description, "itest description");
    assert_eq!(created.goal, "itest goal");
    assert_eq!(created.status, "created");
    assert!(!created.paused);
    assert!(!created.queued);
    assert_eq!(created.timeout_seconds, 0);
    assert_eq!(created.plan_heartbeat_seconds, 600);
    assert!(created.plan_quiet_mode);
    assert!(created.coverage_enabled);
    assert!(!created.target_gate);
    assert!(created.completed_at.is_none());

    // List with a status filter.
    let listed = store
        .list(&ListTasksOptions {
            status: Some("created".into()),
            limit: Some(1000),
            offset: None,
        })
        .await
        .expect("list");
    let listed_task = listed
        .iter()
        .find(|t| t.id == created.id)
        .expect("created task in list");
    assert_eq!(listed_task.category_id, Some(category.id));

    // Get.
    let got = store.get(created.id).await.expect("get").expect("task");
    assert_eq!(got.id, created.id);
    assert_eq!(got.name, "itest create");

    // Update status to a terminal state.
    store
        .update_status(created.id, TaskStatus::Done.as_str())
        .await
        .expect("update status");
    let done = store.get(created.id).await.expect("get").expect("task");
    assert_eq!(done.status, "done");
    assert!(done.completed_at.is_some());

    // Count by status.
    let counts = store.count_by_status().await.expect("count");
    let done_count = counts
        .iter()
        .find(|c| c.status == "done")
        .map(|c| c.count)
        .unwrap_or(0);
    assert!(done_count >= 1, "expected at least one done task");

    // Category task_count reflects the move (task is still alive).
    let category = store
        .get_task_category(category.id)
        .await
        .expect("get category")
        .expect("category");
    assert_eq!(category.task_count, 1);

    // Soft delete.
    assert!(store.soft_delete(created.id).await.expect("soft delete"));
    assert!(store.get(created.id).await.expect("get").is_none());
    assert!(!store
        .soft_delete(created.id)
        .await
        .expect("soft delete again"));

    // Category rename + delete.
    let renamed = store
        .rename_task_category(category.id, "itest category renamed")
        .await
        .expect("rename category");
    assert_eq!(renamed.name, "itest category renamed");
    let categories = store.list_task_categories().await.expect("list categories");
    assert!(categories.iter().any(|c| c.id == category.id));
    assert!(store
        .delete_task_category(category.id)
        .await
        .expect("delete category"));

    // Templates.
    let template = store
        .create_task_template(TaskTemplateInput {
            name: "itest template".into(),
            description: "template description".into(),
            goal: "template goal".into(),
            category_id: None,
            intercept_rules: vec![InterceptRuleInput {
                enabled: true,
                action: "allow".into(),
                kind: "exact_domain".into(),
                pattern: "example.com".into(),
                note: "itest".into(),
            }],
        })
        .await
        .expect("create template");
    assert_eq!(template.name, "itest template");
    assert_eq!(template.intercept_rules.len(), 1);
    assert_eq!(template.intercept_rules[0].action, "allow");

    let fetched = store
        .get_task_template(template.id)
        .await
        .expect("get template")
        .expect("template");
    assert_eq!(fetched.intercept_rules.len(), 1);

    let templates = store.list_task_templates().await.expect("list templates");
    assert!(templates.iter().any(|t| t.id == template.id));

    // UpdateTaskTemplate preserves intercept rules (SetInterceptRules is false,
    // matching Go's UpdateTaskTemplate -> PatchTaskTemplate).
    let updated = store
        .update_task_template(
            template.id,
            TaskTemplateInput {
                name: "itest template v2".into(),
                description: "new description".into(),
                goal: "new goal".into(),
                category_id: None,
                intercept_rules: vec![],
            },
        )
        .await
        .expect("update template");
    assert_eq!(updated.name, "itest template v2");
    assert_eq!(updated.intercept_rules.len(), 1);

    // An explicit patch with SetInterceptRules clears the snapshot.
    let patched = store
        .patch_task_template(
            template.id,
            tasks::TaskTemplatePatch {
                set_intercept_rules: true,
                intercept_rules: vec![],
                ..Default::default()
            },
        )
        .await
        .expect("patch template");
    assert!(patched.intercept_rules.is_empty());

    assert!(store
        .delete_task_template(template.id)
        .await
        .expect("delete template"));
    assert!(store
        .get_task_template(template.id)
        .await
        .expect("get deleted template")
        .is_none());

    cleanup(&db).await;
}

#[tokio::test]
async fn create_task_with_options_conflicts() {
    let _guard = LOCK.lock().await;
    let Some(dsn) = dsn() else { return };
    let db = match open(&dsn).await {
        Ok(db) => db,
        Err(e) => {
            eprintln!("skipping: postgres unavailable ({e})");
            return;
        }
    };
    cleanup(&db).await;
    let store = TaskStore::new(db.clone());

    // Category id must be positive.
    let err = store
        .create_with_options(
            "d",
            "g",
            TaskCreateOptions {
                category_id: Some(0),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(err, tasks::TaskError::Invalid(_)));

    // Non-existent category.
    let err = store
        .create_with_options(
            "d",
            "g",
            TaskCreateOptions {
                category_id: Some(i64::MAX),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(err, tasks::TaskError::NotFound));

    // Too many source tasks.
    let err = store
        .create_with_options(
            "d",
            "g",
            TaskCreateOptions {
                source_task_ids: vec![1, 2, 3, 4, 5, 6, 7, 8, 9],
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(err, tasks::TaskError::Invalid(_)));

    // Invalid source task id.
    let err = store
        .create_with_options(
            "d",
            "g",
            TaskCreateOptions {
                source_task_ids: vec![0],
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(err, tasks::TaskError::Invalid(_)));

    cleanup(&db).await;
}
