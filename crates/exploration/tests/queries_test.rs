//! Integration test: exploration read queries against a real Postgres.
//! Requires ARTEX_PG_DSN; skips when Postgres is unreachable.

use chrono::Utc;
use exploration::{edge_rel, node_kind, node_state, ExplorationStore};
use sqlx::Row;

fn dsn() -> Option<String> {
    std::env::var("ARTEX_PG_DSN")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| Some("postgres://artex:artex@127.0.0.1:55432/artex?sslmode=disable".into()))
}

#[tokio::test]
async fn graph_activity_and_token_queries() {
    let Some(dsn) = dsn() else { return };
    let db = match database::open(&dsn).await {
        Ok(d) => d,
        Err(e) => {
            eprintln!("skipping: postgres unavailable ({e})");
            return;
        }
    };

    let store = ExplorationStore::new(db.clone());
    let pool = db.pool();

    // Unique marker so the test is idempotent against a shared database.
    let marker = format!("exploration-test-{}", std::process::id());

    // Clean any prior rows for this marker.
    sqlx::query("DELETE FROM explorations WHERE goal = $1")
        .bind(&marker)
        .execute(pool)
        .await
        .unwrap();

    // Create a minimal exploration.
    let exp_id: i64 =
        sqlx::query("INSERT INTO explorations(description, goal) VALUES ($1, $2) RETURNING id")
            .bind(&marker)
            .bind(&marker)
            .fetch_one(pool)
            .await
            .unwrap()
            .get("id");

    // Origin fact (task root), an open intent, a confirmed fact.
    let origin_id: i64 = sqlx::query(
        "INSERT INTO exploration_nodes(exploration_id, kind, payload, priority, state, origin) \
VALUES ($1, $2, $3::jsonb, $4, $5, $6) RETURNING id",
    )
    .bind(exp_id)
    .bind(node_kind::FACT)
    .bind(r#"{"summary":"seed"}"#)
    .bind(0i32)
    .bind(node_state::ORIGIN)
    .bind("seed")
    .fetch_one(pool)
    .await
    .unwrap()
    .get("id");

    let intent_id: i64 = sqlx::query(
        "INSERT INTO exploration_nodes(exploration_id, kind, payload, priority, state) \
VALUES ($1, $2, $3::jsonb, $4, 'open') RETURNING id",
    )
    .bind(exp_id)
    .bind(node_kind::INTENT)
    .bind(r#"{"summary":"probe"}"#)
    .bind(5i32)
    .fetch_one(pool)
    .await
    .unwrap()
    .get("id");

    let fact_id: i64 = sqlx::query(
        "INSERT INTO exploration_nodes(exploration_id, kind, payload, priority, state) \
VALUES ($1, $2, $3::jsonb, $4, 'confirmed') RETURNING id",
    )
    .bind(exp_id)
    .bind(node_kind::FACT)
    .bind(r#"{"summary":"found"}"#)
    .bind(0i32)
    .fetch_one(pool)
    .await
    .unwrap()
    .get("id");

    // Edges: intent spawns fact, fact derived_from origin.
    sqlx::query(
        "INSERT INTO exploration_edges(exploration_id, src_id, dst_id, rel) VALUES ($1,$2,$3,$4)",
    )
    .bind(exp_id)
    .bind(intent_id)
    .bind(fact_id)
    .bind(edge_rel::SPAWNS)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO exploration_edges(exploration_id, src_id, dst_id, rel) VALUES ($1,$2,$3,$4)",
    )
    .bind(exp_id)
    .bind(fact_id)
    .bind(origin_id)
    .bind(edge_rel::DERIVED_FROM)
    .execute(pool)
    .await
    .unwrap();

    // Activity: a tool_use row for the intent, a result row with tokens.
    let act1: i64 = sqlx::query(
        "INSERT INTO activity(exploration_id, node_id, worker, kind, tool, tool_use_id, is_error, summary, detail, metadata) \
VALUES ($1,$2,'worker','tool_use','nmap','tu-1',false,'scanning','full detail here','{}'::jsonb) RETURNING id",
    )
    .bind(exp_id)
    .bind(intent_id)
    .fetch_one(pool)
    .await
    .unwrap()
    .get("id");

    let act2: i64 = sqlx::query(
        "INSERT INTO activity(exploration_id, node_id, worker, kind, is_error, summary, detail, metadata, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens) \
VALUES ($1,$2,'worker','result',false,'done','result detail','{}'::jsonb,100,20,5,3) RETURNING id",
    )
    .bind(exp_id)
    .bind(intent_id)
    .fetch_one(pool)
    .await
    .unwrap()
    .get("id");

    // A main-agent result row on segment 1.
    sqlx::query(
        "INSERT INTO activity(exploration_id, worker, kind, main_seg, metadata, input_tokens, output_tokens) \
VALUES ($1,'mainagent','result',1,'{}'::jsonb,7,8)",
    )
    .bind(exp_id)
    .execute(pool)
    .await
    .unwrap();

    // A second main session segment so main_sessions returns 1 then 0.
    sqlx::query("INSERT INTO main_sessions(exploration_id, seq) VALUES ($1, 1)")
        .bind(exp_id)
        .execute(pool)
        .await
        .unwrap();

    // ---- graph ----
    let graph = store.graph(exp_id).await.unwrap();
    assert_eq!(graph.nodes.len(), 3, "graph returns all three nodes");
    assert_eq!(graph.edges.len(), 2, "graph returns both edges");
    let origin = graph
        .nodes
        .iter()
        .find(|n| n.id == origin_id)
        .expect("origin present");
    assert_eq!(origin.state, node_state::ORIGIN);
    assert_eq!(origin.payload["summary"], "seed");

    // ---- nodes (kind filter) ----
    let facts = store
        .nodes(exp_id, Some(node_kind::FACT), 100, 0)
        .await
        .unwrap();
    assert_eq!(facts.len(), 2, "two fact nodes");
    assert!(facts.iter().all(|n| n.kind == node_kind::FACT));

    // ---- frontier ----
    let frontier = store.frontier(exp_id).await.unwrap();
    assert_eq!(frontier.len(), 1);
    assert_eq!(frontier[0].id, intent_id);
    assert_eq!(frontier[0].state, "open");

    // ---- intents ----
    let intents = store.intents(exp_id, None).await.unwrap();
    assert_eq!(intents.len(), 1);
    assert_eq!(intents[0].id, intent_id);

    // ---- activity (forward read) ----
    let page = store.activity(exp_id, 0, None, 100).await.unwrap();
    assert_eq!(page.items.len(), 3, "three activity rows");
    assert!(
        page.items.iter().all(|a| a.detail.is_empty()),
        "list omits detail"
    );
    assert_eq!(page.cursor, page.items.last().unwrap().seq);
    let result = page
        .items
        .iter()
        .find(|a| a.kind == "result" && a.node_id == Some(intent_id))
        .expect("worker result present");
    assert_eq!(result.input_tokens, Some(100));
    assert_eq!(result.output_tokens, Some(20));

    // Activity scoped to one intent: only the two worker rows.
    let scoped = store
        .activity(exp_id, 0, Some(intent_id), 100)
        .await
        .unwrap();
    assert_eq!(scoped.items.len(), 2);

    // Since-cursor read: nothing after the last row.
    let tail = store
        .activity(exp_id, page.cursor, None, 100)
        .await
        .unwrap();
    assert!(tail.items.is_empty());
    assert_eq!(tail.cursor, page.cursor);

    // ---- activity_at (single record, with detail) ----
    let one = store.activity_at(exp_id, act1).await.unwrap().expect("row");
    assert_eq!(one.seq, act1);
    assert_eq!(one.detail, "full detail here");
    assert!(store
        .activity_at(exp_id, 999_999_999)
        .await
        .unwrap()
        .is_none());

    // ---- activity_detail ----
    let detail = store.activity_detail(exp_id, act2).await.unwrap();
    assert_eq!(detail.as_deref(), Some("result detail"));
    assert!(store
        .activity_detail(exp_id, 999_999_999)
        .await
        .unwrap()
        .is_none());

    // ---- activity_max_id ----
    let max = store.activity_max_id(exp_id).await.unwrap();
    assert!(max >= act2, "max id is at least the last inserted row");

    // ---- main_sessions ----
    let sessions = store.main_sessions(exp_id).await.unwrap();
    assert_eq!(sessions.len(), 2);
    assert_eq!(sessions[0].seq, 1, "stored segment first (newest)");
    assert!(sessions[0].created_at.is_some());
    assert_eq!(sessions[1].seq, 0, "implicit original segment last");
    assert!(sessions[1].created_at.is_none());

    // ---- token usage ----
    let report = store.token_usage(exp_id).await.unwrap();
    // workers: only kind='result' rows; mainagent (7+8) and worker (100+20+5+3).
    assert_eq!(report.workers.len(), 2);
    let worker = report
        .workers
        .iter()
        .find(|w| w.worker == "worker")
        .expect("worker aggregate");
    assert_eq!(worker.input_tokens, 100);
    assert_eq!(worker.output_tokens, 20);
    assert_eq!(worker.cache_read_tokens, 5);
    assert_eq!(worker.cache_write_tokens, 3);

    // total = worker + mainagent.
    assert_eq!(report.total.input_tokens, 107);
    assert_eq!(report.total.output_tokens, 28);

    // sessions: main:1 and intent:<id>.
    let main_sess = report
        .sessions
        .iter()
        .find(|s| s.session == "main:1")
        .expect("main session aggregate");
    assert_eq!(main_sess.input_tokens, 7);
    let intent_sess = report
        .sessions
        .iter()
        .find(|s| s.session == format!("intent:{intent_id}"))
        .expect("intent session aggregate");
    assert_eq!(intent_sess.input_tokens, 100);

    // ---- cleanup ----
    sqlx::query("DELETE FROM explorations WHERE id = $1")
        .bind(exp_id)
        .execute(pool)
        .await
        .unwrap();

    // Sanity: cascade removed the nodes.
    let remaining: i64 =
        sqlx::query("SELECT COUNT(*)::bigint AS n FROM exploration_nodes WHERE exploration_id=$1")
            .bind(exp_id)
            .fetch_one(pool)
            .await
            .unwrap()
            .get("n");
    assert_eq!(remaining, 0);

    // Keep chrono import used across builds even if timestamps change.
    let _ = Utc::now();
}
