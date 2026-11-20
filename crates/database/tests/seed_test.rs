//! Integration test: seed_builtins populates the same builtin rows the original
//! Go db.seedBuiltins does, and is idempotent. Requires ARTEX_PG_DSN; skips when
//! Postgres is unreachable.

use database::{open, seed};

fn dsn() -> Option<String> {
    std::env::var("ARTEX_PG_DSN")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| Some("postgres://artex:artex@127.0.0.1:55432/artex?sslmode=disable".into()))
}

async fn scalar_i64(db: &database::Db, q: &str) -> i64 {
    sqlx::query_scalar(q).fetch_one(db.pool()).await.unwrap()
}

#[tokio::test]
async fn seed_matches_original_and_is_idempotent() {
    let Some(dsn) = dsn() else { return };
    let db = match open(&dsn).await {
        Ok(d) => d,
        Err(e) => {
            eprintln!("skipping: postgres unavailable ({e})");
            return;
        }
    };

    // 6 builtin agents.
    assert_eq!(
        scalar_i64(&db, "SELECT count(*) FROM agents WHERE builtin = true").await,
        6
    );
    // 8 prompt vars (goals:1, planner:2, mainagent:3, worker:2).
    assert_eq!(
        scalar_i64(&db, "SELECT count(*) FROM agent_prompt_vars").await,
        8
    );
    // 20 intercept rules (17 V1 + 2 V2 + 1 V3).
    assert_eq!(
        scalar_i64(&db, "SELECT count(*) FROM intercept_rules").await,
        20
    );
    // 4 asset intercept rules.
    assert_eq!(
        scalar_i64(&db, "SELECT count(*) FROM asset_intercept_rules").await,
        4
    );
    // browser MCP (disabled) present.
    assert_eq!(
        scalar_i64(
            &db,
            "SELECT count(*) FROM mcp_servers WHERE name = 'browser'"
        )
        .await,
        1
    );

    // Idempotent: a second open must not change any count.
    let before = scalar_i64(&db, "SELECT count(*) FROM intercept_rules").await;
    let db2 = open(&dsn).await.expect("second open");
    let after = scalar_i64(&db2, "SELECT count(*) FROM intercept_rules").await;
    assert_eq!(before, after, "re-open must not double-seed");

    // The target-gate exemptions seed wrote the JSON setting with 5 entries.
    let raw: String =
        sqlx::query_scalar("SELECT value FROM settings WHERE key = 'target_gate_exempt_hosts'")
            .fetch_one(db2.pool())
            .await
            .expect("target_gate_exempt_hosts setting");
    let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(parsed.as_array().unwrap().len(), 5);
}

#[tokio::test]
async fn seed_settings_helpers() {
    let Some(dsn) = dsn() else { return };
    let db = match open(&dsn).await {
        Ok(d) => d,
        Err(e) => {
            eprintln!("skipping: postgres unavailable ({e})");
            return;
        }
    };
    let mut conn = db.pool().acquire().await.unwrap();
    seed::set_setting(&mut conn, "itest_key", "v1")
        .await
        .unwrap();
    assert_eq!(
        seed::get_setting(&mut conn, "itest_key").await.unwrap(),
        Some("v1".into())
    );
    // insert_if_absent keeps the existing value.
    assert!(
        !seed::insert_setting_if_absent(&mut conn, "itest_key", "v2")
            .await
            .unwrap()
    );
    assert_eq!(
        seed::get_setting(&mut conn, "itest_key").await.unwrap(),
        Some("v1".into())
    );
    sqlx::query("DELETE FROM settings WHERE key = 'itest_key'")
        .execute(&mut *conn)
        .await
        .unwrap();
}
