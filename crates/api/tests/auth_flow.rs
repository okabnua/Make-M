//! End-to-end API auth flow test: health, init, login, change-password, JWT
//! protection. Requires ARTEX_PG_DSN; skips when Postgres is unreachable.
//!
//! Serves the real router over a real TCP socket and drives it with reqwest.

use api::{init_setup_token, router, AppState};
use auth::LoginLimiter;
use database::open;
use serde_json::Value;

fn dsn() -> Option<String> {
    std::env::var("ARTEX_PG_DSN")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| Some("postgres://artex:artex@127.0.0.1:55432/artex?sslmode=disable".into()))
}

struct TestServer {
    base: String,
    handle: tokio::task::JoinHandle<()>,
}

async fn spawn_server(setup_token: Option<String>) -> Option<TestServer> {
    let dsn = dsn()?;
    let db = open(&dsn).await.ok()?;
    // Remove the password key so init can claim it (insert_if_absent semantics).
    sqlx::query("DELETE FROM settings WHERE key = 'auth.password_hash'")
        .execute(db.pool())
        .await
        .ok();
    let key = b"0123456789012345678901234567890123456789".to_vec();
    let state = AppState::new(db, key, "test".into());
    if let Some(tok) = setup_token {
        if let Ok(mut g) = state.setup_token.lock() {
            *g = Some(tok);
        }
    } else {
        init_setup_token(&state).await;
    }
    let app = router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.ok()?;
    let addr = listener.local_addr().ok()?;
    let handle = tokio::spawn(async move {
        let _ = axum::serve(
            listener,
            app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await;
    });
    Some(TestServer {
        base: format!("http://{addr}"),
        handle,
    })
}

#[tokio::test]
async fn auth_flow_end_to_end() {
    let server = spawn_server(Some("known-setup-token".into())).await;
    let Some(server) = server else {
        eprintln!("skipping: postgres unavailable");
        return;
    };
    let base = server.base.clone();
    let client = reqwest::Client::new();

    // health without token -> {ok:true} only.
    let h: Value = client
        .get(format!("{base}/api/health"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(h["ok"], true);
    assert!(h.get("service").is_none());

    // auth status -> not initialized.
    let s: Value = client
        .get(format!("{base}/api/auth/status"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(s["initialized"], false);

    // init with wrong setup token -> 403.
    let code = client
        .post(format!("{base}/api/auth/init"))
        .json(&serde_json::json!({"password": "correct horse battery", "setup_token": "wrong"}))
        .send()
        .await
        .unwrap()
        .status()
        .as_u16();
    assert_eq!(code, 403);

    // init with short password (but correct setup token) -> 400.
    let code = client
        .post(format!("{base}/api/auth/init"))
        .json(&serde_json::json!({"password": "short", "setup_token": "known-setup-token"}))
        .send()
        .await
        .unwrap()
        .status()
        .as_u16();
    assert_eq!(code, 400);

    // init with valid password + setup token -> 200 + token.
    let init: Value = client
        .post(format!("{base}/api/auth/init"))
        .json(&serde_json::json!({"password": "correct horse battery", "setup_token": "known-setup-token"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let token = init["token"].as_str().expect("token in init response");

    // status -> initialized.
    let s: Value = client
        .get(format!("{base}/api/auth/status"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(s["initialized"], true);

    // health WITH token -> service + version.
    let h: Value = client
        .get(format!("{base}/api/health"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(h["service"], "artex");

    // Protected route without token -> 401.
    let code = client
        .get(format!("{base}/api/settings"))
        .send()
        .await
        .unwrap()
        .status()
        .as_u16();
    assert_eq!(code, 401);

    // Login wrong password -> 401.
    let code = client
        .post(format!("{base}/api/auth/login"))
        .json(&serde_json::json!({"username": "ARTEX", "password": "wrong"}))
        .send()
        .await
        .unwrap()
        .status()
        .as_u16();
    assert_eq!(code, 401);

    // Login correct -> token.
    let login: Value = client
        .post(format!("{base}/api/auth/login"))
        .json(&serde_json::json!({"username": "ARTEX", "password": "correct horse battery"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(login["token"].as_str().is_some());

    // change-password with valid token + correct old password -> ok.
    let cp = client
        .post(format!("{base}/api/auth/change-password"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&serde_json::json!({"old_password": "correct horse battery", "new_password": "new secure password"}))
        .send()
        .await
        .unwrap();
    assert_eq!(cp.status().as_u16(), 200);

    // Login with the new password -> token.
    let login2: Value = client
        .post(format!("{base}/api/auth/login"))
        .json(&serde_json::json!({"username": "ARTEX", "password": "new secure password"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(login2["token"].as_str().is_some());

    // JWT from the server verifies against the same key (proves HS256 parity).
    assert!(auth::verify_jwt(token, b"0123456789012345678901234567890123456789").is_some());

    server.handle.abort();
}

#[tokio::test]
async fn password_validation_and_login_limiter_units() {
    assert!(auth::validate_password("short").is_some());
    assert!(auth::validate_password("a long enough password").is_none());
    let mut l = LoginLimiter::new();
    for _ in 0..10 {
        assert!(l.allow("1.1.1.1").is_none());
    }
    assert!(l.allow("1.1.1.1").is_some());
}
