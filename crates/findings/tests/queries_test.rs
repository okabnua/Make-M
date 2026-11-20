//! Integration test: findings queries against a real Postgres. Requires
//! ARTEX_PG_DSN; skips when Postgres is unreachable.

use database::open;
use findings::{FindingError, FindingFilter, FindingStore, SEVERITY_HIGH, STATUS_CONFIRMED};

fn dsn() -> Option<String> {
    std::env::var("ARTEX_PG_DSN")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| Some("postgres://artex:artex@127.0.0.1:55432/artex?sslmode=disable".into()))
}

#[tokio::test]
async fn findings_crud_roundtrip() {
    let Some(dsn) = dsn() else { return };
    let db = match open(&dsn).await {
        Ok(d) => d,
        Err(e) => {
            eprintln!("skipping: postgres unavailable ({e})");
            return;
        }
    };
    let store = FindingStore::new(db.clone());

    // Clean any prior test data so the test is idempotent.
    sqlx::query("DELETE FROM findings WHERE name LIKE 'itest_%'")
        .execute(db.pool())
        .await
        .unwrap();

    // Create (no task/node; assets anchored).
    let id = store
        .create(
            0,
            0,
            "itest_vulnclass",
            "itest finding",
            SEVERITY_HIGH,
            "a summary",
            "some evidence",
            "itest_worker",
            &[1, 2],
        )
        .await
        .unwrap();
    assert!(id > 0);

    // Get returns the row with report + parsed asset_ids.
    let f = store.get(id).await.unwrap().expect("created finding");
    assert_eq!(f.id, id);
    assert_eq!(f.name, "itest finding");
    assert_eq!(f.severity, SEVERITY_HIGH);
    assert_eq!(f.status, "pending");
    assert_eq!(f.asset_ids, vec![1, 2]);
    assert_eq!(f.task_id, None);
    assert!(f.updated_at.is_none());

    // List by vulnclass filter.
    let filter = FindingFilter {
        vulnclass: "itest_vulnclass".into(),
        ..Default::default()
    };
    let (items, total) = store.list(&filter, 10, 0).await.unwrap();
    assert!(total >= 1, "at least the created finding matches");
    assert!(items.iter().any(|x| x.id == id));

    // Stats reflect the created finding.
    let st = store.stats().await.unwrap();
    assert!(st.total >= 1);
    assert!(st.vulnclasses.iter().any(|v| v == "itest_vulnclass"));

    // list_groups: our finding has no task, so it lands in the unassigned bucket.
    let (groups, group_total, finding_total) = store.list_groups(&filter, 10, 0).await.unwrap();
    assert_eq!(group_total, 1);
    assert_eq!(finding_total, 1);
    assert!(groups.iter().any(|g| g.task_id.is_none() && g.count == 1));

    // Update severity (+ node sync path is a no-op here, node_id is NULL).
    let n = store.set_severity(id, "critical").await.unwrap();
    assert_eq!(n, 1);
    let f = store.get(id).await.unwrap().unwrap();
    assert_eq!(f.severity, "critical");

    // Update status.
    let n = store.set_status(id, STATUS_CONFIRMED).await.unwrap();
    assert_eq!(n, 1);
    let f = store.get(id).await.unwrap().unwrap();
    assert_eq!(f.status, STATUS_CONFIRMED);

    // Retest create + list (conversation_id stays NULL; snapshot is stored).
    let retest = store.create_retest(id, "please recheck").await.unwrap();
    assert_eq!(retest.finding_id, id);
    assert_eq!(retest.status, "pending");
    assert_eq!(retest.conversation_id, None);
    let retests = store.list_retests(id).await.unwrap();
    assert_eq!(retests.len(), 1);
    assert_eq!(retests[0].id, retest.id);

    // create_retest on a missing finding is NotFound.
    assert!(matches!(
        store.create_retest(i64::MAX, "").await,
        Err(FindingError::NotFound)
    ));

    // Soft delete removes the row.
    let d = store.soft_delete(id).await.unwrap();
    assert_eq!(d, 1);
    assert!(store.get(id).await.unwrap().is_none());

    // Deleting again is a no-op (0 rows).
    let d = store.soft_delete(id).await.unwrap();
    assert_eq!(d, 0);

    // Cleanup any retest rows left behind (FK cascade already removed them, but
    // be explicit for idempotency).
    sqlx::query("DELETE FROM findings WHERE name LIKE 'itest_%'")
        .execute(db.pool())
        .await
        .unwrap();
}
