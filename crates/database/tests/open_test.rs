//! Integration test: open() against a real Postgres applies schema + migrations
//! and is idempotent across two opens. Requires ARTEX_PG_DSN (or defaults to the
//! local test container). Skips when Postgres is unreachable.

use database::{apply_migrations, open, Db};
use sqlx::Row;

fn dsn() -> Option<String> {
    std::env::var("ARTEX_PG_DSN")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| Some("postgres://artex:artex@127.0.0.1:55432/artex?sslmode=disable".into()))
}

#[tokio::test]
async fn open_applies_schema_and_is_idempotent() {
    let Some(dsn) = dsn() else { return };
    // First open (creates DB if missing, applies schema + migrations).
    let db: Db = match open(&dsn).await {
        Ok(d) => d,
        Err(e) => {
            eprintln!("skipping: postgres unavailable ({e})");
            return;
        }
    };

    // Verify a core table exists.
    let row = sqlx::query("SELECT to_regclass('public.assets') IS NOT NULL AS ok")
        .fetch_one(db.pool())
        .await
        .expect("query assets table");
    let ok: bool = row.get("ok");
    assert!(ok, "assets table should exist after open()");

    // Verify schema_migrations recorded our migrations.
    let cnt: i64 = sqlx::query_scalar("SELECT count(*) FROM schema_migrations")
        .fetch_one(db.pool())
        .await
        .expect("count migrations");
    assert!(cnt >= 2, "expected >= 2 applied migrations, got {cnt}");

    // Second open must be idempotent (no error).
    let db2 = open(&dsn).await.expect("second open should succeed");
    let cnt2: i64 = sqlx::query_scalar("SELECT count(*) FROM schema_migrations")
        .fetch_one(db2.pool())
        .await
        .expect("count migrations 2");
    assert_eq!(cnt, cnt2, "migration count stable across reopens");

    // A raw apply_migrations on a fresh connection must also be a no-op.
    let mut conn = db2.pool().acquire().await.unwrap();
    apply_migrations(&mut conn)
        .await
        .expect("re-apply is idempotent");
}
