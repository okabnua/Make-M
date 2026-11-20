//! Integration test: LLM profile / health / record / usage queries against a
//! real Postgres. Requires ARTEX_PG_DSN; skips when Postgres is unreachable.

use database::open;
use llm::{
    backoff_for, HealthStore, LlmError, LlmFormat, LlmProfile, LlmProfileStore, LlmRecordFilter,
    LlmRecordStore, LlmUsageStore, RetryOverride, RetryRule, SOFT_TRIP_AFTER,
};

fn dsn() -> Option<String> {
    std::env::var("ARTEX_PG_DSN")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| Some("postgres://artex:artex@127.0.0.1:55432/artex?sslmode=disable".into()))
}

#[tokio::test]
async fn profiles_health_records_and_usage() {
    let Some(dsn) = dsn() else { return };
    let db = match open(&dsn).await {
        Ok(d) => d,
        Err(e) => {
            eprintln!("skipping: postgres unavailable ({e})");
            return;
        }
    };

    // Clean any prior test data so the test is idempotent.
    for stmt in [
        "DELETE FROM llm_profiles WHERE name LIKE 'itest_%'",
        "DELETE FROM llm_records WHERE task_id = 'itest_task'",
        "DELETE FROM llm_usage WHERE task_id = 'itest_task'",
        "DELETE FROM conversations WHERE agent_key = 'itest_agent'",
    ] {
        sqlx::query(stmt).execute(db.pool()).await.unwrap();
    }

    let profiles = LlmProfileStore::new(db.clone());
    let health = HealthStore::new(db.clone());
    let records = LlmRecordStore::new(db.clone());
    let usage = LlmUsageStore::new(db.clone());

    // ---- profiles --------------------------------------------------------
    let p = LlmProfile {
        name: "itest_default".into(),
        format: LlmFormat::ANTHROPIC.into(),
        base_url: Some("http://127.0.0.1:1/v1".into()),
        model: "claude-itest".into(),
        api_key: Some("sk-itest-secret".into()),
        rate_per_second: 1.5,
        rate_per_minute: 30.0,
        context_window_k: 200,
        thinking_type: "enabled".into(),
        reasoning_effort: "medium".into(),
        priority: 7,
        streaming: true,
        max_tokens: 4096,
        max_tokens_field: "max_completion_tokens".into(),
        session_header_key: "x-session".into(),
        user_agent: "itest-agent".into(),
        retry: RetryOverride {
            connect: RetryRule {
                attempts: 2,
                interval_ms: 250,
            },
            ..RetryOverride::default()
        },
        price_input_per_m: 1.25,
        price_output_per_m: 2.5,
        ..LlmProfile::default()
    };
    let id = profiles.create(&p).await.unwrap();
    assert!(id > 0);

    let listed = profiles.list().await.unwrap();
    let found = listed.iter().find(|x| x.id == id).unwrap();
    assert_eq!(found.name, "itest_default");
    assert_eq!(found.model, "claude-itest");
    assert_eq!(found.context_window_k, 200);
    assert_eq!(found.max_tokens_field, "max_completion_tokens");
    assert_eq!(found.retry.connect.attempts, 2);
    assert_eq!(found.retry.connect.interval_ms, 250);
    assert_eq!(found.price_input_per_m, 1.25);
    assert!(found.api_key.is_none(), "list must not expose api_key");
    assert_eq!(found.api_key_hint.as_deref(), Some("\u{2026}cret"));
    assert!(!found.created_at.is_empty());
    assert!(!found.updated_at.is_empty());

    let got = profiles.get(id).await.unwrap().unwrap();
    assert_eq!(got.api_key.as_deref(), Some("sk-itest-secret"));
    let by_name = profiles
        .get_by_name("itest_default")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(by_name.id, id);
    assert_eq!(by_name.api_key.as_deref(), Some("sk-itest-secret"));

    // update keeps the key when api_key is None (Go behaviour)
    let mut upd = got.clone();
    upd.model = "claude-itest-2".into();
    upd.api_key = None;
    profiles.update(&upd).await.unwrap();
    let got2 = profiles.get(id).await.unwrap().unwrap();
    assert_eq!(got2.model, "claude-itest-2");
    assert_eq!(got2.api_key.as_deref(), Some("sk-itest-secret"));

    // ---- health ----------------------------------------------------------
    sqlx::query("DELETE FROM llm_profile_health WHERE profile_id = $1")
        .bind(id)
        .execute(db.pool())
        .await
        .unwrap();
    assert!(health.get(id).await.unwrap().is_none());

    assert!(!health
        .record_failure(id, "status 429", false)
        .await
        .unwrap());
    assert!(!health
        .record_failure(id, "status 429", false)
        .await
        .unwrap());
    assert!(
        health
            .record_failure(id, "status 429", false)
            .await
            .unwrap(),
        "the SOFT_TRIP_AFTER-th transient failure trips"
    );
    let h = health.get(id).await.unwrap().unwrap();
    assert_eq!(h.fails, 0);
    assert_eq!(h.trips, 1);
    assert_eq!(h.last_error, "status 429");
    assert!(h.open_until.is_some());
    assert!(h.is_open());
    assert!(health
        .load_open()
        .await
        .unwrap()
        .iter()
        .any(|x| x.profile_id == id));

    health.record_success(id).await.unwrap();
    assert!(health.get(id).await.unwrap().is_none());

    // hard failures trip immediately
    assert!(health.record_failure(id, "status 401", true).await.unwrap());
    let h = health.get(id).await.unwrap().unwrap();
    assert_eq!(h.trips, 1);
    let until = h.open_until.expect("hard failure opens the breaker");
    assert!(until > h.last_at);
    assert!(h.is_open());
    health.reset(id).await.unwrap();
    assert!(health.get(id).await.unwrap().is_none());

    // ---- records ---------------------------------------------------------
    sqlx::query(
        "INSERT INTO llm_records(model, profile_name, session_id, task_id, worker, \
         latency_ms, input_tokens, output_tokens, cache_read, cache_write, status, \
         request_body, response_body) \
         VALUES ('claude-itest', 'itest_default', 'exp1-worker', 'itest_task', 'worker', \
         12, 100, 20, 5, 1, 'ok', 'req', 'resp')",
    )
    .execute(db.pool())
    .await
    .unwrap();
    // Metering row used by by_model and the usage aggregations below.
    sqlx::query(
        "INSERT INTO llm_usage(task_id, worker, model, profile_name, latency_ms, \
         input_tokens, output_tokens, cache_read, cache_write, status) \
         VALUES ('itest_task', 'worker', 'claude-itest', 'itest_default', 10, \
         100, 20, 5, 1, 'ok')",
    )
    .execute(db.pool())
    .await
    .unwrap();

    let page = records
        .list(LlmRecordFilter {
            task_id: Some("itest_task".into()),
            model: Some("claude-itest".into()),
            limit: 10,
            offset: 0,
        })
        .await
        .unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.records.len(), 1);
    let rid = page.records[0].id;
    assert_eq!(page.records[0].task_id, "itest_task");
    assert_eq!(page.records[0].input_tokens, 100);

    let rec = records.get(rid).await.unwrap().unwrap();
    assert_eq!(rec.request_body, "req");
    assert_eq!(rec.response_body, "resp");
    assert!(!rec.ts.is_empty());

    let tasks = records.list_tasks().await.unwrap();
    assert!(tasks.iter().any(|t| t.task_id == "itest_task"));

    let models = records.by_model("itest_task").await.unwrap();
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].model, "claude-itest");
    assert_eq!(models[0].input_tokens, 100);
    assert_eq!(models[0].output_tokens, 20);

    assert_eq!(records.delete_by_task("itest_task").await.unwrap(), 1);
    let empty = records
        .list(LlmRecordFilter {
            task_id: Some("itest_task".into()),
            ..LlmRecordFilter::default()
        })
        .await
        .unwrap();
    assert_eq!(empty.total, 0);

    // ---- usage -----------------------------------------------------------
    let total = usage.total().await.unwrap();
    assert!(total.calls >= 1);
    assert!(total.input_tokens >= 100);

    let daily = usage.daily(7).await.unwrap();
    assert!(
        daily
            .iter()
            .any(|d| d.profile_name == "itest_default" && d.input_tokens >= 100),
        "daily should include the inserted bucket"
    );

    let by_profile = usage.by_profile().await.unwrap();
    let prof = by_profile
        .iter()
        .find(|x| x.profile_name == "itest_default");
    assert!(prof.is_some(), "by_profile should include itest_default");

    let conv_id: i64 = sqlx::query_scalar(
        "INSERT INTO conversations(agent_key, title, llm_profile_id) \
         VALUES ('itest_agent', 'itest conversation', $1) RETURNING id",
    )
    .bind(id)
    .fetch_one(db.pool())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO conversation_activities(conversation_id, worker, kind, \
         input_tokens, output_tokens, cache_read_tokens, cache_write_tokens) \
         VALUES ($1, 'worker', 'result', 10, 5, 1, 1)",
    )
    .bind(conv_id)
    .execute(db.pool())
    .await
    .unwrap();
    let convs = usage.conversations().await.unwrap();
    let conv = convs.iter().find(|c| c.llm_profile_id == Some(id)).unwrap();
    assert_eq!(conv.input_tokens, 10);
    assert_eq!(conv.output_tokens, 5);
    sqlx::query("DELETE FROM conversations WHERE id = $1")
        .bind(conv_id)
        .execute(db.pool())
        .await
        .unwrap();

    sqlx::query("DELETE FROM llm_usage WHERE task_id = 'itest_task'")
        .execute(db.pool())
        .await
        .unwrap();

    // ---- delete ----------------------------------------------------------
    profiles.delete(id).await.unwrap();
    assert!(profiles.get(id).await.unwrap().is_none());
    assert!(matches!(
        profiles.delete(id).await,
        Err(LlmError::NotFound(_))
    ));

    // The constants the health store ports stay in sync with the Go source.
    assert_eq!(SOFT_TRIP_AFTER, 3);
    assert_eq!(backoff_for(1).num_seconds(), 60);
    assert_eq!(backoff_for(2).num_seconds(), 300);
    assert_eq!(backoff_for(3).num_seconds(), 1800);
}
