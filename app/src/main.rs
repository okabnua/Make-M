//! artex — Rust backend binary (port of `cmd/artex/main.go`).
//!
//! Startup: load config, resolve DSN, open the database (schema + migrations +
//! seed), load/create the JWT key, initialize the setup token, and serve the
//! HTTP API. Signal-driven graceful shutdown.

use std::net::SocketAddr;
use std::sync::Arc;

use api::{init_setup_token, router, AppState};
use config::Config;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    // Flags: -addr (default 127.0.0.1:8787).
    let addr: SocketAddr = std::env::args()
        .skip_while(|a| a != "-addr")
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| "127.0.0.1:8787".parse().unwrap());

    // Config + DSN.
    let _cfg = Config::default();
    let (dsn, source) = config::postgres_dsn()?;
    tracing::info!(%source, dsn = %config::redact(&dsn), "starting artex");

    // Open the database (create if missing, apply schema + migrations + seed).
    let db = database::open(&dsn).await?;
    tracing::info!("database ready");

    // JWT key.
    let base = config::base_dir();
    let key_dir = config::key_dir();
    let jwt_key = auth::load_or_create_jwt_key(&key_dir, &base, &base)?;

    let state = AppState::new(db, jwt_key, env!("CARGO_PKG_VERSION").to_string());
    if init_setup_token(&state).await.is_some() {
        // token already logged by init_setup_token
    }

    let app = router(state);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, "listening");

    // Graceful shutdown on Ctrl-C / SIGTERM.
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;

    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut sig) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            sig.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    tracing::info!("shutting down");
}

// Silence unused import warning for Arc (used in state via AppState).
#[allow(dead_code)]
fn _assert_send_sync(_: Arc<()>) {}
