//! Integration test: settings data layer against a real Postgres. Requires
//! `ARTEX_PG_DSN` (or the local test container) and skips gracefully when
//! Postgres is unreachable.

use database::open;
use settings::SettingsStore;

fn dsn() -> Option<String> {
    std::env::var("ARTEX_PG_DSN")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| Some("postgres://artex:artex@127.0.0.1:55432/artex?sslmode=disable".into()))
}

const TEST_KEY: &str = "itest_settings_key";
const TEST_BOOL_KEY: &str = "itest_settings_bool";

async fn cleanup(db: &database::Db) {
    sqlx::query("DELETE FROM settings WHERE key LIKE 'itest_%' OR key = 'tools.exec'")
        .execute(db.pool())
        .await
        .expect("cleanup query");
}

#[tokio::test]
async fn settings_round_trip_and_stats() {
    let Some(dsn) = dsn() else { return };
    let db = match open(&dsn).await {
        Ok(db) => db,
        Err(e) => {
            eprintln!("skipping: postgres unavailable ({e})");
            return;
        }
    };
    cleanup(&db).await;
    let store = SettingsStore::new(db.clone());

    // get/set round trip.
    assert_eq!(store.get(TEST_KEY).await.unwrap(), None);
    store.set(TEST_KEY, "hello").await.unwrap();
    assert_eq!(store.get(TEST_KEY).await.unwrap().as_deref(), Some("hello"));

    // get_bool defaults + set_bool.
    assert!(store.get_bool(TEST_BOOL_KEY, true).await.unwrap());
    store.set_bool(TEST_BOOL_KEY, false).await.unwrap();
    assert!(!store.get_bool(TEST_BOOL_KEY, true).await.unwrap());
    store.set(TEST_BOOL_KEY, "1").await.unwrap();
    assert!(store.get_bool(TEST_BOOL_KEY, false).await.unwrap());

    // Typed getters reflect stored values.
    store
        .set_bool(settings::KEY_LLM_RECORD, true)
        .await
        .unwrap();
    assert!(store.llm_record_enabled().await.unwrap());
    store.set("workers", "7").await.unwrap();
    assert_eq!(store.worker_concurrency().await.unwrap(), 7);
    store.set("workers", "0").await.unwrap();
    assert_eq!(store.worker_concurrency().await.unwrap(), 3);
    store.set("web_search_backend", "tavily").await.unwrap();
    assert_eq!(store.web_search_backend().await.unwrap(), "tavily");

    // set_worker_concurrency rejects non-positive values.
    assert!(store.set_worker_concurrency(0).await.is_err());

    // list_all returns the test key but never secret keys.
    store
        .set_secret(settings::KEY_BRAVE_API_KEY, "secret-brave")
        .await
        .unwrap();
    store
        .set(settings::KEY_WEB_SEARCH_PROXY, "http://p:1")
        .await
        .unwrap();
    store
        .set(settings::KEY_GLOBAL_PROXY, "socks5://h:1")
        .await
        .unwrap();
    store.set("tools.exec", "{}").await.unwrap();

    let all = store.list_all().await.unwrap();
    assert_eq!(all.get(TEST_KEY).map(String::as_str), Some("hello"));
    for secret in [
        settings::KEY_BRAVE_API_KEY,
        settings::KEY_WEB_SEARCH_PROXY,
        settings::KEY_GLOBAL_PROXY,
        "tools.exec",
    ] {
        assert!(
            !all.contains_key(secret),
            "secret key {secret} must be hidden"
        );
    }

    // stats() runs and agrees with a direct llm_profiles count.
    let stats = store.stats().await.unwrap();
    let expected: i64 = sqlx::query_scalar("SELECT count(*) FROM llm_profiles")
        .fetch_one(db.pool())
        .await
        .unwrap();
    assert_eq!(stats.llm_configured, expected > 0);
    assert_eq!(stats.engine_mode, "idle");
    assert_eq!(
        stats.assets,
        stats.asset_counts.values().copied().sum::<i64>()
    );
    assert_eq!(
        stats.tasks_total,
        stats.tasks_by_status.values().copied().sum::<i64>()
    );

    cleanup(&db).await;
}
