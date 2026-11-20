//! Remote MCP clients + server/tool stores — port of ARTEX `mcphttp/`.
//!
//! Implements the Model Context Protocol over **Streamable HTTP** (the primary
//! transport): `initialize` handshake, `tools/list`, `tools/call`, session
//! handling, bearer auth. The legacy SSE transport from `client.go` is not
//! ported (modern servers use Streamable HTTP); a documented limitation.

use database::Db;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::Row;
use std::collections::HashMap;
use std::time::Duration;

/// Prefix for tool names exposed by MCP servers (`mcp__server__tool`).
pub fn tool_name(server: &str, name: &str) -> String {
    format!("mcp__{server}__{name}")
}

#[derive(Debug, thiserror::Error)]
pub enum McpError {
    #[error("sql: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("http: {0}")]
    Http(#[from] reqwest::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("rpc error {code}: {message}")]
    Rpc { code: i64, message: String },
    #[error("transport: {0}")]
    Transport(String),
    #[error("not found")]
    NotFound,
}

// ---------------------------------------------------------------------------
// JSON-RPC 2.0
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: Option<u64>,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl JsonRpcRequest {
    fn new(id: Option<u64>, method: &str, params: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            method: method.into(),
            params,
        }
    }
}

// ---------------------------------------------------------------------------
// Server + tool models
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct McpServer {
    pub id: i64,
    pub name: String,
    /// stdio / http / sse
    pub transport: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    pub enabled: bool,
    pub insecure: bool,
    pub timeout_seconds: i32,
    pub last_connect_at: Option<String>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct McpTool {
    pub server_id: i64,
    pub tool_name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub schema: Value,
}

/// DB-backed server + tool cache store.
pub struct McpStore {
    db: Db,
}

const SERVER_COLS: &str = "id, name, transport, command, args, env, url, enabled, insecure, \
timeout_seconds, last_connect_at::text AS last_connect_at, last_error";

impl McpStore {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    pub async fn list(&self) -> Result<Vec<McpServer>, McpError> {
        let rows = sqlx::query(&format!(
            "SELECT {SERVER_COLS} FROM mcp_servers ORDER BY name"
        ))
        .fetch_all(self.db.pool())
        .await?;
        rows.iter().map(server_from_row).collect()
    }

    pub async fn get(&self, id: i64) -> Result<Option<McpServer>, McpError> {
        let row = sqlx::query(&format!(
            "SELECT {SERVER_COLS} FROM mcp_servers WHERE id = $1"
        ))
        .bind(id)
        .fetch_optional(self.db.pool())
        .await?;
        row.as_ref().map(server_from_row).transpose()
    }

    pub async fn get_by_name(&self, name: &str) -> Result<Option<McpServer>, McpError> {
        let row = sqlx::query(&format!(
            "SELECT {SERVER_COLS} FROM mcp_servers WHERE name = $1"
        ))
        .bind(name)
        .fetch_optional(self.db.pool())
        .await?;
        row.as_ref().map(server_from_row).transpose()
    }

    pub async fn create(&self, s: &McpServer) -> Result<i64, McpError> {
        let args = serde_json::to_value(&s.args)?;
        let env = serde_json::to_value(s.env.clone())?;
        let row = sqlx::query(
            "INSERT INTO mcp_servers(name, transport, command, args, env, url, enabled, insecure, timeout_seconds) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9) RETURNING id",
        )
        .bind(&s.name)
        .bind(&s.transport)
        .bind(&s.command)
        .bind(&args)
        .bind(&env)
        .bind(&s.url)
        .bind(s.enabled)
        .bind(s.insecure)
        .bind(s.timeout_seconds)
        .fetch_one(self.db.pool())
        .await?;
        Ok(row.get("id"))
    }

    pub async fn update(&self, s: &McpServer) -> Result<(), McpError> {
        let args = serde_json::to_value(&s.args)?;
        let env = serde_json::to_value(s.env.clone())?;
        sqlx::query(
            "UPDATE mcp_servers SET name=$2, transport=$3, command=$4, args=$5, env=$6, url=$7, \
             enabled=$8, insecure=$9, timeout_seconds=$10 WHERE id=$1",
        )
        .bind(s.id)
        .bind(&s.name)
        .bind(&s.transport)
        .bind(&s.command)
        .bind(&args)
        .bind(&env)
        .bind(&s.url)
        .bind(s.enabled)
        .bind(s.insecure)
        .bind(s.timeout_seconds)
        .execute(self.db.pool())
        .await?;
        Ok(())
    }

    pub async fn delete(&self, id: i64) -> Result<bool, McpError> {
        let res = sqlx::query("DELETE FROM mcp_servers WHERE id = $1")
            .bind(id)
            .execute(self.db.pool())
            .await?;
        Ok(res.rows_affected() > 0)
    }

    /// Replace a server's cached tool list.
    pub async fn upsert_tools(&self, server_id: i64, tools: &[McpTool]) -> Result<(), McpError> {
        let mut tx = self.db.pool().begin().await?;
        sqlx::query("DELETE FROM mcp_tools_cache WHERE server_id = $1")
            .bind(server_id)
            .execute(&mut *tx)
            .await?;
        for t in tools {
            let schema = t.schema.to_string();
            sqlx::query(
                "INSERT INTO mcp_tools_cache(server_id, tool_name, description, schema) \
                 VALUES ($1,$2,$3,$4::jsonb)",
            )
            .bind(server_id)
            .bind(&t.tool_name)
            .bind(&t.description)
            .bind(&schema)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn list_tools(&self, server_id: i64) -> Result<Vec<McpTool>, McpError> {
        let rows = sqlx::query(
            "SELECT server_id, tool_name, description, schema::text AS schema \
             FROM mcp_tools_cache WHERE server_id = $1 ORDER BY tool_name",
        )
        .bind(server_id)
        .fetch_all(self.db.pool())
        .await?;
        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            let schema: String = r.get("schema");
            out.push(McpTool {
                server_id: r.get("server_id"),
                tool_name: r.get("tool_name"),
                description: r.get("description"),
                schema: serde_json::from_str(&schema).unwrap_or(Value::Null),
            });
        }
        Ok(out)
    }
}

fn server_from_row(r: &sqlx::postgres::PgRow) -> Result<McpServer, McpError> {
    let args: String = r.get::<Option<String>, _>("args").unwrap_or_default();
    let env: String = r.get::<Option<String>, _>("env").unwrap_or_default();
    let command: Option<String> = r.get("command");
    Ok(McpServer {
        id: r.get("id"),
        name: r.get("name"),
        transport: r.get("transport"),
        command,
        args: serde_json::from_str::<Vec<String>>(&args).unwrap_or_default(),
        env: serde_json::from_str::<HashMap<String, String>>(&env).unwrap_or_default(),
        url: r.get("url"),
        enabled: r.get("enabled"),
        insecure: r.get("insecure"),
        timeout_seconds: r.get("timeout_seconds"),
        last_connect_at: r.get("last_connect_at"),
        last_error: r.get("last_error"),
    })
}

// ---------------------------------------------------------------------------
// Streamable HTTP client
// ---------------------------------------------------------------------------

/// A connected MCP client bound to one server's Streamable HTTP endpoint.
pub struct McpClient {
    server_name: String,
    endpoint: String,
    headers: HashMap<String, String>,
    /// Retained for parity/debugging; TLS verification is already applied to
    /// the reqwest client at construction.
    #[allow(dead_code)]
    insecure: bool,
    #[allow(dead_code)]
    timeout: Duration,
    session_id: Option<String>,
    next_id: u64,
    client: reqwest::Client,
}

impl McpClient {
    /// Build a client for an `http`/`streamable-http` server.
    pub fn new(server: &McpServer) -> Result<Self, McpError> {
        let endpoint = server
            .url
            .clone()
            .ok_or_else(|| McpError::Transport("http server has no url".into()))?;
        let mut headers = HashMap::new();
        // Auth from env `AUTH_HEADER`/`AUTH_VALUE` conventions is left to the
        // caller; `normalizeHeaders` in Go strips invalid ones. We accept the
        // server env `AUTHORIZATION` if present.
        if let Some(auth) = server.env.get("AUTHORIZATION") {
            headers.insert("authorization".into(), auth.clone());
        }
        let client = reqwest::Client::builder()
            .danger_accept_invalid_certs(server.insecure)
            .timeout(Duration::from_secs(server.timeout_seconds.max(1) as u64))
            .build()?;
        Ok(Self {
            server_name: server.name.clone(),
            endpoint,
            headers,
            insecure: server.insecure,
            timeout: Duration::from_secs(server.timeout_seconds.max(1) as u64),
            session_id: None,
            next_id: 1,
            client,
        })
    }

    /// Initialize the session (MCP `initialize` + `notifications/initialized`).
    pub async fn connect(&mut self) -> Result<(), McpError> {
        let resp = self
            .call(
                "initialize",
                Some(json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "clientInfo": { "name": "artex", "version": "1.0" }
                })),
            )
            .await?;
        let _ = resp;
        // notifications/initialized (no id, no response expected).
        let _ = self
            .notify("notifications/initialized", Some(json!({})))
            .await;
        Ok(())
    }

    /// List tools exposed by the server.
    pub async fn list_tools(&mut self) -> Result<Vec<McpTool>, McpError> {
        let resp = self.call("tools/list", Some(json!({}))).await?;
        let tools = resp
            .get("tools")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        Ok(tools
            .into_iter()
            .filter_map(|t| {
                Some(McpTool {
                    server_id: 0,
                    tool_name: t.get("name")?.as_str()?.to_string(),
                    description: t
                        .get("description")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                    schema: t.get("inputSchema").cloned().unwrap_or(Value::Null),
                })
            })
            .collect())
    }

    /// Invoke a tool by name with JSON args.
    pub async fn call_tool(&mut self, tool: &str, args: Value) -> Result<Value, McpError> {
        self.call(
            "tools/call",
            Some(json!({ "name": tool, "arguments": args })),
        )
        .await
    }

    async fn call(&mut self, method: &str, params: Option<Value>) -> Result<Value, McpError> {
        let id = self.next_id;
        self.next_id += 1;
        let req = JsonRpcRequest::new(Some(id), method, params);
        let resp = self.roundtrip(req, true).await?;
        if let Some(err) = resp.error {
            return Err(McpError::Rpc {
                code: err.code,
                message: err.message,
            });
        }
        Ok(resp.result.unwrap_or(Value::Null))
    }

    async fn notify(&mut self, method: &str, params: Option<Value>) -> Result<(), McpError> {
        let req = JsonRpcRequest::new(None, method, params);
        let _ = self.roundtrip(req, false).await?;
        Ok(())
    }

    async fn roundtrip(
        &mut self,
        body: JsonRpcRequest,
        expect_resp: bool,
    ) -> Result<JsonRpcResponse, McpError> {
        let mut req = self
            .client
            .post(&self.endpoint)
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream");
        for (k, v) in &self.headers {
            req = req.header(k, v);
        }
        if let Some(sid) = &self.session_id {
            req = req.header("mcp-session-id", sid);
        }
        let resp = req.json(&body).send().await?;
        let status = resp.status();
        if let Some(sid) = resp
            .headers()
            .get("mcp-session-id")
            .and_then(|v| v.to_str().ok())
        {
            self.session_id = Some(sid.to_string());
        }
        if !status.is_success() {
            return Err(McpError::Transport(format!(
                "mcp {} status {}",
                self.server_name, status
            )));
        }
        if !expect_resp {
            return Ok(JsonRpcResponse {
                jsonrpc: "2.0".into(),
                id: body.id,
                result: None,
                error: None,
            });
        }
        // Streamable HTTP may answer with JSON or an SSE stream; parse both.
        let ctype = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let bytes = resp.bytes().await?;
        let text = String::from_utf8_lossy(&bytes).to_string();
        let json_text = if ctype.contains("text/event-stream") {
            parse_sse_data(&text).unwrap_or(text)
        } else {
            text
        };
        let parsed: JsonRpcResponse = serde_json::from_str(&json_text)?;
        Ok(parsed)
    }
}

/// Extract the `data:` payload of the first SSE event carrying JSON.
fn parse_sse_data(text: &str) -> Option<String> {
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("data:") {
            return Some(rest.trim().to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_name_format() {
        assert_eq!(tool_name("github", "search"), "mcp__github__search");
    }

    #[test]
    fn jsonrpc_roundtrip() {
        let req = JsonRpcRequest::new(Some(1), "tools/list", Some(json!({})));
        let s = serde_json::to_string(&req).unwrap();
        let back: JsonRpcRequest = serde_json::from_str(&s).unwrap();
        assert_eq!(back.method, "tools/list");
        assert_eq!(back.id, Some(1));
        assert_eq!(back.jsonrpc, "2.0");
    }

    #[test]
    fn jsonrpc_response_with_error() {
        let raw = r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"nope"}}"#;
        let r: JsonRpcResponse = serde_json::from_str(raw).unwrap();
        let e = r.error.unwrap();
        assert_eq!(e.code, -32601);
        assert_eq!(e.message, "nope");
    }

    #[test]
    fn sse_data_extraction() {
        let s = "event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}\n\n";
        assert_eq!(
            parse_sse_data(s).unwrap(),
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}"
        );
    }

    #[test]
    fn server_args_env_roundtrip() {
        let s = McpServer {
            args: vec!["--headless".into()],
            env: HashMap::from([("K".to_string(), "V".to_string())]),
            ..Default::default()
        };
        let a = Value::from(s.args.clone());
        let back: Vec<String> = serde_json::from_value(a).unwrap();
        assert_eq!(back, vec!["--headless".to_string()]);
    }
}
