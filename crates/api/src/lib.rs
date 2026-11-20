//! HTTP API layer — port of ARTEX `server/` routing.
//!
//! This crate implements the axum Router, JWT auth middleware, and the first
//! vertical slice of handlers (health + auth). The remaining ~140 routes from
//! API-INVENTORY.md are added incrementally, each verified against the original.

use assets::{AssetStore, CompanyStore};
use auth::{AuthError, LoginLimiter};
use axum::{
    extract::{ConnectInfo, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use database::Db;

/// Shared application state.
#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    pub jwt_key: Arc<Vec<u8>>,
    pub login_limiter: Arc<Mutex<LoginLimiter>>,
    pub setup_token: Arc<Mutex<Option<String>>>,
    pub build_version: Arc<String>,
    pub assets: Arc<AssetStore>,
    pub companies: Arc<CompanyStore>,
}

impl AppState {
    pub fn new(db: Db, jwt_key: Vec<u8>, build_version: String) -> Self {
        let assets = Arc::new(AssetStore::new(db.clone()));
        let companies = Arc::new(CompanyStore::new(db.clone()));
        Self {
            db,
            jwt_key: Arc::new(jwt_key),
            login_limiter: Arc::new(Mutex::new(LoginLimiter::new())),
            setup_token: Arc::new(Mutex::new(None)),
            build_version: Arc::new(build_version),
            assets,
            companies,
        }
    }
}

/// Uniform error reply when a password-related read fails.
const ERR_DATA_SOURCE_UNAVAILABLE: &str = "Data source temporarily unavailable, please retry later";

/// Build the full router with the current (growing) set of routes.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/auth/status", get(auth_status))
        .route("/api/auth/init", post(auth_init))
        .route("/api/auth/login", post(auth_login))
        .route("/api/auth/change-password", post(auth_change_password))
        .route("/api/assets", get(list_assets))
        .route("/api/assets/counts", get(asset_counts))
        .route("/api/companies", get(list_companies))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            require_auth,
        ))
        .with_state(state)
}

fn json_response(code: StatusCode, v: Value) -> Response {
    (code, Json(v)).into_response()
}

fn err_response(code: StatusCode, msg: &str) -> Response {
    json_response(code, json!({ "error": msg }))
}

/// Extract the JWT from Authorization: Bearer, artex_token cookie, or ?token=.
fn extract_token(headers: &HeaderMap, query_token: Option<&str>) -> Option<String> {
    if let Some(h) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    {
        if let Some(rest) = h.strip_prefix("Bearer ") {
            return Some(rest.to_string());
        }
    }
    if let Some(c) = headers.get(header::COOKIE).and_then(|v| v.to_str().ok()) {
        for part in c.split(';') {
            let part = part.trim();
            if let Some(v) = part.strip_prefix("artex_token=") {
                if !v.is_empty() {
                    return Some(v.to_string());
                }
            }
        }
    }
    query_token.map(|s| s.to_string())
}

/// JWT validation middleware. `/api/auth/*` and `/api/health` are exempt.
async fn require_auth(
    State(state): State<AppState>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let path = req.uri().path().to_string();
    if path.starts_with("/api/auth/") || path == "/api/health" {
        return next.run(req).await;
    }
    let token = extract_token(req.headers(), req.uri().query());
    match token {
        None => err_response(StatusCode::UNAUTHORIZED, "Unauthorized"),
        Some(tok) if auth::verify_jwt(&tok, &state.jwt_key).is_none() => {
            err_response(StatusCode::UNAUTHORIZED, "token is invalid or expired")
        }
        Some(_) => next.run(req).await,
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

async fn health(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let token = extract_token(&headers, None);
    let valid = token
        .as_deref()
        .map(|t| auth::verify_jwt(t, &state.jwt_key).is_some())
        .unwrap_or(false);
    if !valid {
        return json_response(StatusCode::OK, json!({ "ok": true }));
    }
    json_response(
        StatusCode::OK,
        json!({ "ok": true, "service": "artex", "version": state.build_version.as_str() }),
    )
}

async fn auth_status(State(state): State<AppState>) -> Response {
    match state.db.get_setting(auth::AUTH_PASS_KEY).await {
        Ok(hash) => json_response(
            StatusCode::OK,
            json!({ "initialized": hash.map(|h| !h.is_empty()).unwrap_or(false) }),
        ),
        Err(_) => err_response(StatusCode::SERVICE_UNAVAILABLE, ERR_DATA_SOURCE_UNAVAILABLE),
    }
}

#[derive(Deserialize)]
struct InitRequest {
    password: Option<String>,
    setup_token: Option<String>,
}

async fn auth_init(State(state): State<AppState>, Json(req): Json<InitRequest>) -> Response {
    match state.db.get_setting(auth::AUTH_PASS_KEY).await {
        Ok(Some(h)) if !h.is_empty() => {
            return err_response(StatusCode::FORBIDDEN, "Password is already set")
        }
        Ok(_) => {}
        Err(_) => {
            return err_response(StatusCode::SERVICE_UNAVAILABLE, ERR_DATA_SOURCE_UNAVAILABLE)
        }
    }
    let password = req.password.unwrap_or_default();
    if password.is_empty() {
        return err_response(StatusCode::BAD_REQUEST, "Password cannot be empty");
    }
    let expected = state.setup_token.lock().map(|g| g.clone()).unwrap_or(None);
    let got = req.setup_token.unwrap_or_default();
    if !auth::check_setup_token(&got, expected.as_deref().unwrap_or("")) {
        return err_response(
            StatusCode::FORBIDDEN,
            "Invalid setup token. The server log prints it at startup while no password is set",
        );
    }
    if let Some(msg) = auth::validate_password(&password) {
        return err_response(StatusCode::BAD_REQUEST, &msg);
    }
    let hash = match auth::hash_password(&password) {
        Ok(h) => h,
        Err(_) => {
            return err_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Password encryption failed",
            )
        }
    };
    match state
        .db
        .insert_setting_if_absent(auth::AUTH_PASS_KEY, &hash)
        .await
    {
        Ok(true) => {}
        Ok(false) => return err_response(StatusCode::FORBIDDEN, "Password is already set"),
        Err(e) => {
            return err_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("Save failed: {e}"),
            )
        }
    }
    if let Ok(mut g) = state.setup_token.lock() {
        *g = None;
    }
    match auth::sign_jwt(&state.jwt_key) {
        Ok(tok) => json_response(StatusCode::OK, json!({ "token": tok })),
        Err(_) => err_response(StatusCode::INTERNAL_SERVER_ERROR, "token generation failed"),
    }
}

#[derive(Deserialize)]
struct LoginRequest {
    username: Option<String>,
    password: Option<String>,
}

async fn auth_login(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(req): Json<LoginRequest>,
) -> Response {
    let trusted: Vec<auth::TrustedProxy> = Vec::new(); // configured later via env
    let xff = headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .map(|v| vec![v.to_string()])
        .unwrap_or_default();
    let ip = auth::client_ip(&peer, &xff, &trusted);

    let wait = state
        .login_limiter
        .lock()
        .map(|mut l| l.allow(&ip))
        .unwrap_or(None);
    if let Some(wait) = wait {
        // Ceil(wait / 1s), matching Go's (wait + time.Second - 1) / time.Second.
        let secs = (wait.as_secs_f64().ceil() as u64).max(1);
        let mut resp = err_response(
            StatusCode::TOO_MANY_REQUESTS,
            &format!("Too many login attempts, try again in {secs} seconds"),
        );
        resp.headers_mut()
            .insert(header::RETRY_AFTER, secs.to_string().parse().unwrap());
        return resp;
    }

    if req.username.as_deref() != Some(auth::USERNAME) {
        return err_response(StatusCode::UNAUTHORIZED, "Incorrect username or password");
    }
    let hash = match state.db.get_setting(auth::AUTH_PASS_KEY).await {
        Ok(Some(h)) if !h.is_empty() => h,
        Ok(_) => {
            return err_response(
                StatusCode::FORBIDDEN,
                "Password not initialized; set a password first",
            )
        }
        Err(_) => {
            return err_response(StatusCode::SERVICE_UNAVAILABLE, ERR_DATA_SOURCE_UNAVAILABLE)
        }
    };
    let password = req.password.unwrap_or_default();
    if !auth::verify_password(&password, &hash) {
        return err_response(StatusCode::UNAUTHORIZED, "Incorrect username or password");
    }
    if let Ok(mut l) = state.login_limiter.lock() {
        l.reset(&ip);
    }
    match auth::sign_jwt(&state.jwt_key) {
        Ok(tok) => json_response(StatusCode::OK, json!({ "token": tok })),
        Err(_) => err_response(StatusCode::INTERNAL_SERVER_ERROR, "token generation failed"),
    }
}

#[derive(Deserialize)]
struct ChangePasswordRequest {
    old_password: Option<String>,
    new_password: Option<String>,
}

async fn auth_change_password(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<ChangePasswordRequest>,
) -> Response {
    let token = extract_token(&headers, None);
    if token
        .as_deref()
        .map(|t| auth::verify_jwt(t, &state.jwt_key).is_none())
        .unwrap_or(true)
    {
        return err_response(StatusCode::UNAUTHORIZED, "Unauthorized");
    }
    let new_password = req.new_password.unwrap_or_default();
    if new_password.is_empty() {
        return err_response(StatusCode::BAD_REQUEST, "New password cannot be empty");
    }
    if let Some(msg) = auth::validate_password(&new_password) {
        return err_response(StatusCode::BAD_REQUEST, &msg);
    }
    let hash = match state.db.get_setting(auth::AUTH_PASS_KEY).await {
        Ok(Some(h)) if !h.is_empty() => h,
        Ok(_) => {
            return err_response(
                StatusCode::FORBIDDEN,
                "Password not initialized; set a password first",
            )
        }
        Err(_) => {
            return err_response(StatusCode::SERVICE_UNAVAILABLE, ERR_DATA_SOURCE_UNAVAILABLE)
        }
    };
    let old_password = req.old_password.unwrap_or_default();
    if !auth::verify_password(&old_password, &hash) {
        return err_response(StatusCode::UNAUTHORIZED, "Current password is wrong");
    }
    let new_hash = match auth::hash_password(&new_password) {
        Ok(h) => h,
        Err(_) => {
            return err_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Password encryption failed",
            )
        }
    };
    match state.db.set_setting(auth::AUTH_PASS_KEY, &new_hash).await {
        Ok(()) => json_response(StatusCode::OK, json!({ "ok": true })),
        Err(e) => err_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("Save failed: {e}"),
        ),
    }
}

// ---------------------------------------------------------------------------
// Assets & companies handlers
// ---------------------------------------------------------------------------

const DEFAULT_ASSET_PAGE_SIZE: i64 = 50;
const MAX_ASSET_PAGE_SIZE: i64 = 500;

#[derive(Deserialize)]
struct AssetQuery {
    #[serde(rename = "type")]
    typ: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
    company_id: Option<i64>,
    task_id: Option<i64>,
}

async fn list_assets(State(state): State<AppState>, Query(q): Query<AssetQuery>) -> Response {
    let mut limit = q.limit.unwrap_or(0);
    if limit <= 0 {
        limit = DEFAULT_ASSET_PAGE_SIZE;
    } else if limit > MAX_ASSET_PAGE_SIZE {
        limit = MAX_ASSET_PAGE_SIZE;
    }
    let offset = q.offset.unwrap_or(0).max(0);
    let typ = q.typ.clone().unwrap_or_default();

    // Resolve the filter the same way the original listAssets does.
    let (assets, total) = if let Some(cid) = q.company_id.filter(|c| *c > 0) {
        let t = state
            .assets
            .count_by_company(cid, if typ.is_empty() { None } else { Some(&typ) })
            .await
            .unwrap_or(0);
        (Vec::new(), t)
    } else if let Some(tid) = q.task_id.filter(|t| *t > 0) {
        let total = state
            .assets
            .count_by_task(tid, if typ.is_empty() { None } else { Some(&typ) })
            .await;
        (Vec::new(), total.unwrap_or(0))
    } else {
        let typ = if typ.is_empty() {
            "root_domain".to_string()
        } else {
            typ
        };
        match state.assets.query_by_type(&typ, limit, offset).await {
            Ok(assets) => {
                let total = state.assets.count_by_type(&typ).await.unwrap_or(0);
                (assets, total)
            }
            Err(e) => return err_response(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
        }
    };

    let count = assets.len();
    json_response(
        StatusCode::OK,
        json!({ "count": count, "total": total, "assets": assets }),
    )
}

async fn asset_counts(State(state): State<AppState>) -> Response {
    match state.assets.counts_by_type().await {
        Ok(counts) => json_response(StatusCode::OK, serde_json::to_value(counts).unwrap()),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

async fn list_companies(State(state): State<AppState>) -> Response {
    match state.companies.list().await {
        Ok(companies) => json_response(StatusCode::OK, serde_json::to_value(companies).unwrap()),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

/// Initialize the one-time setup token when no admin password is set; returns it
/// so the caller (main) can log it.
pub async fn init_setup_token(state: &AppState) -> Option<String> {
    match state.db.get_setting(auth::AUTH_PASS_KEY).await {
        Ok(Some(h)) if !h.is_empty() => return None,
        Ok(_) => {}
        Err(_) => return None,
    }
    let tok = auth::generate_setup_token();
    if let Ok(mut g) = state.setup_token.lock() {
        *g = Some(tok.clone());
    }
    tracing::info!(
        "no admin password is set. Setup token for the /setup page: {}",
        tok
    );
    Some(tok)
}

/// Map an AuthError to a string (for callers that surface it).
pub fn auth_error_string(e: AuthError) -> String {
    e.to_string()
}
