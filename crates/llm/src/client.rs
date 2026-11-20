use crate::{LlmError, LlmFormat, LlmProfile};
use serde_json::{json, Value};
use std::fmt;
use std::time::Duration;

pub const DEFAULT_OLLAMA_BASE_URL: &str = "http://localhost:11434/v1";
pub const OLLAMA_PLACEHOLDER_KEY: &str = "ollama";

const DEFAULT_OPENAI_BASE_URL: &str = "https://api.openai.com/v1";
const DEFAULT_ANTHROPIC_BASE_URL: &str = "https://api.anthropic.com";
const DEFAULT_ANTHROPIC_MAX_TOKENS: u32 = 4096;
const DEFAULT_TIMEOUT_SECS: u64 = 120;
const MAX_ERROR_BODY: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Default)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub thinking_type: Option<String>,
    pub reasoning_effort: Option<String>,
    pub tools: Option<Vec<ToolDef>>,
    pub system: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChatResponse {
    pub content: String,
    pub tool_calls: Vec<ToolCall>,
    pub usage: TokenUsage,
    pub stop_reason: Option<String>,
}

pub struct LlmClient {
    format: &'static str,
    base_url: String,
    api_key: String,
    model: String,
    max_tokens: i32,
    max_tokens_field: String,
    thinking_type: String,
    reasoning_effort: String,
    user_agent: String,
    http: reqwest::Client,
}

impl LlmClient {
    pub fn from_profile(p: &LlmProfile) -> Result<Self, LlmError> {
        let format = LlmFormat::parse(&p.format)?;
        let base_url = normalize_base_url(format, p.base_url.as_deref());
        let api_key = match format {
            LlmFormat::OLLAMA => {
                let key = p.api_key.as_deref().map(str::trim).unwrap_or("");
                if key.is_empty() {
                    OLLAMA_PLACEHOLDER_KEY.to_string()
                } else {
                    key.to_string()
                }
            }
            _ => p.api_key.clone().unwrap_or_default(),
        };
        let model = if p.model.trim().is_empty() {
            default_model(format).to_string()
        } else {
            p.model.clone()
        };
        let proxy = p.proxy.as_deref().map(str::trim).filter(|s| !s.is_empty());
        let mut builder =
            reqwest::Client::builder().timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS));
        match proxy {
            Some(proxy) => {
                let parsed = reqwest::Url::parse(proxy)
                    .map_err(|_| LlmError::Invalid("llm: invalid proxy URL".into()))?;
                if parsed.scheme() != "http" && parsed.scheme() != "https" {
                    return Err(LlmError::Invalid(format!(
                        "llm: unsupported proxy scheme {:?} (use http or https)",
                        parsed.scheme()
                    )));
                }
                builder = builder.proxy(
                    reqwest::Proxy::all(proxy)
                        .map_err(|_| LlmError::Invalid("llm: invalid proxy URL".into()))?,
                );
            }
            None => builder = builder.no_proxy(),
        }
        Ok(Self {
            format,
            base_url,
            api_key,
            model,
            max_tokens: p.max_tokens,
            max_tokens_field: p.max_tokens_field.clone(),
            thinking_type: p.thinking_type.clone(),
            reasoning_effort: p.reasoning_effort.clone(),
            user_agent: p.user_agent.clone(),
            http: builder.build()?,
        })
    }

    pub fn format(&self) -> &'static str {
        self.format
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn endpoint(&self) -> String {
        match self.format {
            LlmFormat::ANTHROPIC => format!("{}/v1/messages", self.base_url),
            LlmFormat::OPENAI_RESPONSES => format!("{}/responses", self.base_url),
            _ => format!("{}/chat/completions", self.base_url),
        }
    }

    pub async fn chat(&self, req: &ChatRequest) -> Result<ChatResponse, LlmError> {
        let body = self.build_body(req)?;
        let response = self
            .http
            .post(self.endpoint())
            .headers(self.headers()?)
            .json(&body)
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(LlmError::Api {
                status: status.as_u16(),
                body: clip_body(&body),
            });
        }
        let text = response.text().await?;
        let value: Value = serde_json::from_str(&text)
            .map_err(|e| LlmError::Parse(format!("invalid json response: {e}")))?;
        self.parse_response(&value)
    }

    fn headers(&self) -> Result<reqwest::header::HeaderMap, LlmError> {
        let mut headers = reqwest::header::HeaderMap::new();
        if !self.api_key.is_empty() {
            let value = reqwest::header::HeaderValue::from_str(&self.api_key).map_err(|_| {
                LlmError::Invalid("llm: api key is not a valid header value".into())
            })?;
            match self.format {
                LlmFormat::ANTHROPIC => {
                    headers.insert("x-api-key", value);
                    headers.insert(
                        "anthropic-version",
                        reqwest::header::HeaderValue::from_static("2023-06-01"),
                    );
                }
                _ => {
                    let bearer =
                        reqwest::header::HeaderValue::from_str(&format!("Bearer {}", self.api_key))
                            .map_err(|_| {
                                LlmError::Invalid("llm: api key is not a valid header value".into())
                            })?;
                    headers.insert(reqwest::header::AUTHORIZATION, bearer);
                }
            }
        }
        if !self.user_agent.is_empty() {
            let ua = reqwest::header::HeaderValue::from_str(&self.user_agent).map_err(|_| {
                LlmError::Invalid("llm: user agent is not a valid header value".into())
            })?;
            headers.insert(reqwest::header::USER_AGENT, ua);
        }
        Ok(headers)
    }

    fn build_body(&self, req: &ChatRequest) -> Result<Value, LlmError> {
        let model = if req.model.trim().is_empty() {
            self.model.as_str()
        } else {
            req.model.as_str()
        };
        let max_tokens = req.max_tokens.or(if self.max_tokens > 0 {
            Some(self.max_tokens as u32)
        } else {
            None
        });
        let thinking = resolve_setting(&req.thinking_type, &self.thinking_type);
        let effort = resolve_setting(&req.reasoning_effort, &self.reasoning_effort);
        match self.format {
            LlmFormat::ANTHROPIC => {
                let (system, messages) = anthropic_messages(req);
                let mut body = json!({
                    "model": model,
                    "system": system,
                    "messages": messages,
                    "max_tokens": max_tokens.unwrap_or(DEFAULT_ANTHROPIC_MAX_TOKENS),
                });
                if let Some(temperature) = req.temperature {
                    body["temperature"] = json!(temperature);
                }
                if let Some(tools) = req.tools.as_deref().filter(|t| !t.is_empty()) {
                    body["tools"] = json!(anthropic_tools(tools));
                }
                if let Some(thinking) = thinking {
                    body["thinking"] = json!({ "type": thinking });
                }
                if let Some(effort) = effort {
                    body["output_config"] = json!({ "effort": effort });
                }
                Ok(body)
            }
            LlmFormat::OPENAI_RESPONSES => {
                let input = openai_messages(req);
                let mut body = json!({ "model": model, "input": input });
                if let Some(max_tokens) = max_tokens {
                    body["max_output_tokens"] = json!(max_tokens);
                }
                if let Some(temperature) = req.temperature {
                    body["temperature"] = json!(temperature);
                }
                if let Some(tools) = req.tools.as_deref().filter(|t| !t.is_empty()) {
                    body["tools"] = json!(responses_tools(tools));
                }
                if let Some(effort) = effort {
                    body["reasoning"] = json!({ "effort": effort });
                }
                Ok(body)
            }
            _ => {
                let messages = openai_messages(req);
                let mut body = json!({ "model": model, "messages": messages });
                if let Some(max_tokens) = max_tokens {
                    if self.max_tokens_field == "max_completion_tokens" {
                        body["max_completion_tokens"] = json!(max_tokens);
                    } else {
                        body["max_tokens"] = json!(max_tokens);
                    }
                }
                if let Some(temperature) = req.temperature {
                    body["temperature"] = json!(temperature);
                }
                if let Some(tools) = req.tools.as_deref().filter(|t| !t.is_empty()) {
                    body["tools"] = json!(openai_tools(tools));
                }
                if let Some(thinking) = thinking {
                    body["thinking"] = json!({ "type": thinking });
                }
                if let Some(effort) = effort {
                    body["reasoning_effort"] = json!(effort);
                }
                Ok(body)
            }
        }
    }

    fn parse_response(&self, value: &Value) -> Result<ChatResponse, LlmError> {
        match self.format {
            LlmFormat::ANTHROPIC => parse_anthropic(value),
            LlmFormat::OPENAI_RESPONSES => parse_responses(value),
            _ => parse_openai(value),
        }
    }
}

impl fmt::Debug for LlmClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LlmClient")
            .field("format", &self.format)
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("max_tokens", &self.max_tokens)
            .field("max_tokens_field", &self.max_tokens_field)
            .field("thinking_type", &self.thinking_type)
            .field("reasoning_effort", &self.reasoning_effort)
            .field(
                "api_key",
                &if self.api_key.is_empty() {
                    "none"
                } else {
                    "***"
                },
            )
            .finish()
    }
}

fn default_model(format: &str) -> &'static str {
    match format {
        LlmFormat::OPENAI => "gpt-4o",
        LlmFormat::OPENAI_RESPONSES => "gpt-5",
        LlmFormat::OLLAMA => "llama3.2",
        _ => "claude-opus-4-8",
    }
}

fn normalize_base_url(format: &str, raw: Option<&str>) -> String {
    let raw = raw.unwrap_or("").trim().trim_end_matches('/');
    match format {
        LlmFormat::OLLAMA => normalize_ollama_base_url(raw),
        LlmFormat::OPENAI => {
            let base = strip_suffix(raw, "/chat/completions").to_string();
            if base.is_empty() {
                DEFAULT_OPENAI_BASE_URL.to_string()
            } else {
                base
            }
        }
        LlmFormat::OPENAI_RESPONSES => {
            let base = strip_suffix(raw, "/responses").to_string();
            if base.is_empty() {
                DEFAULT_OPENAI_BASE_URL.to_string()
            } else {
                base
            }
        }
        _ => {
            let base = strip_suffix(raw, "/v1/messages").to_string();
            if base.is_empty() {
                DEFAULT_ANTHROPIC_BASE_URL.to_string()
            } else {
                base
            }
        }
    }
}

fn normalize_ollama_base_url(raw: &str) -> String {
    let base = strip_suffix(raw, "/chat/completions");
    if base.is_empty() {
        return DEFAULT_OLLAMA_BASE_URL.to_string();
    }
    if base.ends_with("/v1") {
        base.to_string()
    } else {
        format!("{base}/v1")
    }
}

fn strip_suffix<'a>(raw: &'a str, suffix: &str) -> &'a str {
    raw.strip_suffix(suffix)
        .unwrap_or(raw)
        .trim_end_matches('/')
}

fn resolve_setting<'a>(request: &'a Option<String>, profile: &'a str) -> Option<&'a str> {
    match request {
        Some(value) => non_empty(value),
        None => non_empty(profile),
    }
}

fn non_empty(value: &str) -> Option<&str> {
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

fn openai_messages(req: &ChatRequest) -> Vec<Value> {
    let mut messages = Vec::with_capacity(req.messages.len() + 1);
    if let Some(system) = req.system.as_deref().filter(|s| !s.trim().is_empty()) {
        messages.push(json!({ "role": "system", "content": system }));
    }
    for message in &req.messages {
        messages.push(json!({
            "role": message.role.as_str(),
            "content": message.content.as_str(),
        }));
    }
    messages
}

fn anthropic_messages(req: &ChatRequest) -> (String, Vec<Value>) {
    let mut system_parts = Vec::new();
    if let Some(system) = req.system.as_deref().filter(|s| !s.trim().is_empty()) {
        system_parts.push(system.to_string());
    }
    let mut messages = Vec::with_capacity(req.messages.len());
    for message in &req.messages {
        let role = match message.role.as_str() {
            "assistant" => "assistant",
            "system" => {
                system_parts.push(message.content.clone());
                continue;
            }
            _ => "user",
        };
        messages.push(json!({
            "role": role,
            "content": message.content.as_str(),
        }));
    }
    (system_parts.join("\n"), messages)
}

fn openai_tools(tools: &[ToolDef]) -> Vec<Value> {
    tools
        .iter()
        .map(|tool| {
            json!({
                "type": "function",
                "function": {
                    "name": tool.name.as_str(),
                    "description": tool.description.as_str(),
                    "parameters": tool.input_schema.clone(),
                }
            })
        })
        .collect()
}

fn anthropic_tools(tools: &[ToolDef]) -> Vec<Value> {
    tools
        .iter()
        .map(|tool| {
            json!({
                "name": tool.name.as_str(),
                "description": tool.description.as_str(),
                "input_schema": tool.input_schema.clone(),
            })
        })
        .collect()
}

fn responses_tools(tools: &[ToolDef]) -> Vec<Value> {
    tools
        .iter()
        .map(|tool| {
            json!({
                "type": "function",
                "name": tool.name.as_str(),
                "description": tool.description.as_str(),
                "parameters": tool.input_schema.clone(),
            })
        })
        .collect()
}

fn parse_openai(value: &Value) -> Result<ChatResponse, LlmError> {
    let choice = value
        .pointer("/choices/0")
        .ok_or_else(|| LlmError::Parse("openai: response has no choices[0]".into()))?;
    let message = choice
        .get("message")
        .ok_or_else(|| LlmError::Parse("openai: choice has no message".into()))?;
    let content = message
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let tool_calls = message
        .get("tool_calls")
        .and_then(Value::as_array)
        .map(|calls| calls.iter().filter_map(parse_openai_tool_call).collect())
        .unwrap_or_default();
    let usage = value.get("usage");
    Ok(ChatResponse {
        content,
        tool_calls,
        usage: TokenUsage {
            input_tokens: field_u64(usage, "prompt_tokens"),
            output_tokens: field_u64(usage, "completion_tokens"),
            cache_read_tokens: path_u64(usage, "/prompt_tokens_details/cached_tokens")
                .max(field_u64(usage, "cache_read_input_tokens")),
            cache_write_tokens: field_u64(usage, "cache_creation_input_tokens"),
        },
        stop_reason: string_field(choice, "finish_reason"),
    })
}

fn parse_openai_tool_call(value: &Value) -> Option<ToolCall> {
    let function = value.get("function")?;
    let name = function.get("name").and_then(Value::as_str)?.to_string();
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let arguments = function
        .get("arguments")
        .map(parse_arguments)
        .unwrap_or(Value::Null);
    Some(ToolCall {
        id,
        name,
        arguments,
    })
}

fn parse_anthropic(value: &Value) -> Result<ChatResponse, LlmError> {
    let mut content = String::new();
    let mut tool_calls = Vec::new();
    if let Some(blocks) = value.get("content").and_then(Value::as_array) {
        for block in blocks {
            match block.get("type").and_then(Value::as_str) {
                Some("text") => {
                    if let Some(text) = block.get("text").and_then(Value::as_str) {
                        content.push_str(text);
                    }
                }
                Some("tool_use") => {
                    let name = match block.get("name").and_then(Value::as_str) {
                        Some(name) => name.to_string(),
                        None => continue,
                    };
                    let id = block
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    tool_calls.push(ToolCall {
                        id,
                        name,
                        arguments: block.get("input").cloned().unwrap_or(Value::Null),
                    });
                }
                _ => {}
            }
        }
    }
    let usage = value.get("usage");
    Ok(ChatResponse {
        content,
        tool_calls,
        usage: TokenUsage {
            input_tokens: field_u64(usage, "input_tokens"),
            output_tokens: field_u64(usage, "output_tokens"),
            cache_read_tokens: field_u64(usage, "cache_read_input_tokens"),
            cache_write_tokens: field_u64(usage, "cache_creation_input_tokens"),
        },
        stop_reason: string_field(value, "stop_reason"),
    })
}

fn parse_responses(value: &Value) -> Result<ChatResponse, LlmError> {
    let mut content = String::new();
    let mut tool_calls = Vec::new();
    if let Some(items) = value.get("output").and_then(Value::as_array) {
        for item in items {
            match item.get("type").and_then(Value::as_str) {
                Some("function_call") => {
                    let name = match item.get("name").and_then(Value::as_str) {
                        Some(name) => name.to_string(),
                        None => continue,
                    };
                    let id = item
                        .get("call_id")
                        .and_then(Value::as_str)
                        .or_else(|| item.get("id").and_then(Value::as_str))
                        .unwrap_or_default()
                        .to_string();
                    let arguments = item
                        .get("arguments")
                        .map(parse_arguments)
                        .unwrap_or(Value::Null);
                    tool_calls.push(ToolCall {
                        id,
                        name,
                        arguments,
                    });
                }
                Some("message") => {
                    if let Some(parts) = item.get("content").and_then(Value::as_array) {
                        for part in parts {
                            match part.get("type").and_then(Value::as_str) {
                                Some("output_text") | Some("text") => {
                                    if let Some(text) = part.get("text").and_then(Value::as_str) {
                                        content.push_str(text);
                                    }
                                }
                                _ => {}
                            }
                        }
                    } else if let Some(text) = item.get("content").and_then(Value::as_str) {
                        content.push_str(text);
                    }
                }
                _ => {}
            }
        }
    }
    let usage = value.get("usage");
    let stop_reason = match value.get("status").and_then(Value::as_str) {
        Some("incomplete") => path_string(value, "/incomplete_details/reason")
            .or_else(|| Some("incomplete".to_string())),
        Some(status) => Some(status.to_string()),
        None => None,
    };
    Ok(ChatResponse {
        content,
        tool_calls,
        usage: TokenUsage {
            input_tokens: field_u64(usage, "input_tokens"),
            output_tokens: field_u64(usage, "output_tokens"),
            cache_read_tokens: path_u64(usage, "/input_tokens_details/cached_tokens"),
            cache_write_tokens: field_u64(usage, "cache_creation_input_tokens"),
        },
        stop_reason,
    })
}

fn parse_arguments(value: &Value) -> Value {
    match value {
        Value::String(raw) => {
            serde_json::from_str(raw).unwrap_or_else(|_| Value::String(raw.clone()))
        }
        other => other.clone(),
    }
}

fn field_u64(value: Option<&Value>, key: &str) -> u64 {
    value
        .and_then(|v| v.get(key))
        .and_then(Value::as_u64)
        .unwrap_or(0)
}

fn path_u64(value: Option<&Value>, path: &str) -> u64 {
    value
        .and_then(|v| v.pointer(path))
        .and_then(Value::as_u64)
        .unwrap_or(0)
}

fn string_field(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_string)
}

fn path_string(value: &Value, path: &str) -> Option<String> {
    value
        .pointer(path)
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn clip_body(body: &str) -> String {
    if body.len() <= MAX_ERROR_BODY {
        return body.to_string();
    }
    let mut end = MAX_ERROR_BODY;
    while end > 0 && !body.is_char_boundary(end) {
        end -= 1;
    }
    format!(
        "{}\u{2026}(truncated, {} bytes total)",
        &body[..end],
        body.len()
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn profile(format: &str) -> LlmProfile {
        LlmProfile {
            id: 1,
            name: "test".into(),
            format: format.into(),
            base_url: Some("https://example.test/v1".into()),
            model: "test-model".into(),
            api_key: Some("sk-secret".into()),
            ..LlmProfile::default()
        }
    }

    fn request() -> ChatRequest {
        ChatRequest {
            model: "req-model".into(),
            messages: vec![ChatMessage {
                role: "user".into(),
                content: "hi".into(),
            }],
            max_tokens: Some(64),
            temperature: Some(0.5),
            ..ChatRequest::default()
        }
    }

    fn tool() -> ToolDef {
        ToolDef {
            name: "lookup".into(),
            description: "look something up".into(),
            input_schema: json!({"type": "object", "properties": {"q": {"type": "string"}}}),
        }
    }

    #[test]
    fn openai_request_shape() {
        let mut p = profile(LlmFormat::OPENAI);
        p.base_url = Some("https://example.test/v1/".into());
        p.thinking_type = "enabled".into();
        p.reasoning_effort = "high".into();
        let client = LlmClient::from_profile(&p).unwrap();
        let mut req = request();
        req.system = Some("be terse".into());
        req.tools = Some(vec![tool()]);

        let body = client.build_body(&req).unwrap();
        assert_eq!(body["model"], "req-model");
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][0]["content"], "be terse");
        assert_eq!(body["messages"][1]["role"], "user");
        assert_eq!(body["messages"][1]["content"], "hi");
        assert_eq!(body["max_tokens"].as_u64(), Some(64));
        assert_eq!(body["temperature"].as_f64(), Some(0.5));
        assert_eq!(body["thinking"]["type"], "enabled");
        assert_eq!(body["reasoning_effort"], "high");
        assert_eq!(body["tools"][0]["type"], "function");
        assert_eq!(body["tools"][0]["function"]["name"], "lookup");
        assert_eq!(body["tools"][0]["function"]["parameters"]["type"], "object");
        assert_eq!(
            client.endpoint(),
            "https://example.test/v1/chat/completions"
        );
    }

    #[test]
    fn openai_max_completion_tokens_field() {
        let mut p = profile(LlmFormat::OPENAI);
        p.max_tokens_field = "max_completion_tokens".into();
        let client = LlmClient::from_profile(&p).unwrap();
        let body = client.build_body(&request()).unwrap();
        assert_eq!(body["max_completion_tokens"].as_u64(), Some(64));
        assert!(body.get("max_tokens").is_none());
    }

    #[test]
    fn openai_omits_empty_thinking_and_effort() {
        let client = LlmClient::from_profile(&profile(LlmFormat::OPENAI)).unwrap();
        let mut req = request();
        req.thinking_type = Some(String::new());
        req.reasoning_effort = Some(String::new());
        let body = client.build_body(&req).unwrap();
        assert!(body.get("thinking").is_none());
        assert!(body.get("reasoning_effort").is_none());
    }

    #[test]
    fn anthropic_request_shape() {
        let mut p = profile(LlmFormat::ANTHROPIC);
        p.base_url = Some("https://example.test".into());
        p.thinking_type = "enabled".into();
        p.reasoning_effort = "medium".into();
        let client = LlmClient::from_profile(&p).unwrap();
        let mut req = request();
        req.system = Some("sys".into());
        req.messages.push(ChatMessage {
            role: "assistant".into(),
            content: "hello".into(),
        });
        req.messages.push(ChatMessage {
            role: "tool".into(),
            content: "result".into(),
        });
        req.tools = Some(vec![tool()]);

        let body = client.build_body(&req).unwrap();
        assert_eq!(body["model"], "req-model");
        assert_eq!(body["system"], "sys");
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["messages"][1]["role"], "assistant");
        assert_eq!(body["messages"][2]["role"], "user");
        assert_eq!(body["max_tokens"].as_u64(), Some(64));
        assert_eq!(body["temperature"].as_f64(), Some(0.5));
        assert_eq!(body["thinking"]["type"], "enabled");
        assert_eq!(body["output_config"]["effort"], "medium");
        assert_eq!(body["tools"][0]["name"], "lookup");
        assert_eq!(body["tools"][0]["input_schema"]["type"], "object");
        assert_eq!(client.endpoint(), "https://example.test/v1/messages");
    }

    #[test]
    fn anthropic_defaults_max_tokens_when_unset() {
        let client = LlmClient::from_profile(&profile(LlmFormat::ANTHROPIC)).unwrap();
        let mut req = request();
        req.max_tokens = None;
        let body = client.build_body(&req).unwrap();
        assert_eq!(body["max_tokens"].as_u64(), Some(4096));
    }

    #[test]
    fn responses_request_shape() {
        let mut p = profile(LlmFormat::OPENAI_RESPONSES);
        p.reasoning_effort = "low".into();
        let client = LlmClient::from_profile(&p).unwrap();
        let mut req = request();
        req.system = Some("sys".into());
        req.tools = Some(vec![tool()]);

        let body = client.build_body(&req).unwrap();
        assert_eq!(body["model"], "req-model");
        assert_eq!(body["input"][0]["role"], "system");
        assert_eq!(body["input"][1]["role"], "user");
        assert_eq!(body["input"][1]["content"], "hi");
        assert_eq!(body["max_output_tokens"].as_u64(), Some(64));
        assert_eq!(body["reasoning"]["effort"], "low");
        assert_eq!(body["tools"][0]["type"], "function");
        assert_eq!(body["tools"][0]["name"], "lookup");
        assert_eq!(body["tools"][0]["parameters"]["type"], "object");
        assert_eq!(client.endpoint(), "https://example.test/v1/responses");
    }

    #[test]
    fn model_falls_back_to_profile() {
        let client = LlmClient::from_profile(&profile(LlmFormat::OPENAI)).unwrap();
        let mut req = request();
        req.model = String::new();
        let body = client.build_body(&req).unwrap();
        assert_eq!(body["model"], "test-model");
    }

    #[test]
    fn ollama_defaults_base_url_and_placeholder_key() {
        let mut p = profile(LlmFormat::OLLAMA);
        p.base_url = None;
        p.api_key = None;
        let client = LlmClient::from_profile(&p).unwrap();
        assert_eq!(client.format(), LlmFormat::OLLAMA);
        assert_eq!(client.base_url(), DEFAULT_OLLAMA_BASE_URL);
        assert_eq!(
            client.endpoint(),
            "http://localhost:11434/v1/chat/completions"
        );
        assert_eq!(client.api_key, OLLAMA_PLACEHOLDER_KEY);
    }

    #[test]
    fn ollama_normalizes_base_url_and_blank_key() {
        let mut p = profile(LlmFormat::OLLAMA);
        p.base_url = Some("http://host:11434/".into());
        p.api_key = Some("   ".into());
        let client = LlmClient::from_profile(&p).unwrap();
        assert_eq!(client.base_url(), "http://host:11434/v1");

        p.base_url = Some("http://host:11434/v1/chat/completions".into());
        let client = LlmClient::from_profile(&p).unwrap();
        assert_eq!(client.base_url(), "http://host:11434/v1");
        assert_eq!(client.api_key, OLLAMA_PLACEHOLDER_KEY);
    }

    #[test]
    fn ollama_uses_openai_wire_shape() {
        let mut p = profile(LlmFormat::OLLAMA);
        p.base_url = None;
        p.api_key = None;
        let client = LlmClient::from_profile(&p).unwrap();
        let body = client.build_body(&request()).unwrap();
        assert_eq!(body["model"], "req-model");
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["max_tokens"].as_u64(), Some(64));

        let headers = client.headers().unwrap();
        assert_eq!(
            headers
                .get(reqwest::header::AUTHORIZATION)
                .unwrap()
                .to_str()
                .unwrap(),
            "Bearer ollama"
        );
    }

    #[test]
    fn cloud_bases_default_when_empty() {
        let mut p = profile(LlmFormat::OPENAI);
        p.base_url = None;
        assert_eq!(
            LlmClient::from_profile(&p).unwrap().base_url(),
            DEFAULT_OPENAI_BASE_URL
        );

        let mut p = profile(LlmFormat::ANTHROPIC);
        p.base_url = None;
        assert_eq!(
            LlmClient::from_profile(&p).unwrap().base_url(),
            DEFAULT_ANTHROPIC_BASE_URL
        );
    }

    #[test]
    fn headers_match_format() {
        let anthropic = LlmClient::from_profile(&profile(LlmFormat::ANTHROPIC)).unwrap();
        let headers = anthropic.headers().unwrap();
        assert_eq!(
            headers.get("x-api-key").unwrap().to_str().unwrap(),
            "sk-secret"
        );
        assert_eq!(
            headers.get("anthropic-version").unwrap().to_str().unwrap(),
            "2023-06-01"
        );
        assert!(headers.get(reqwest::header::AUTHORIZATION).is_none());

        let openai = LlmClient::from_profile(&profile(LlmFormat::OPENAI)).unwrap();
        let headers = openai.headers().unwrap();
        assert_eq!(
            headers
                .get(reqwest::header::AUTHORIZATION)
                .unwrap()
                .to_str()
                .unwrap(),
            "Bearer sk-secret"
        );
        assert!(headers.get("x-api-key").is_none());
    }

    #[test]
    fn debug_redacts_api_key() {
        let client = LlmClient::from_profile(&profile(LlmFormat::OPENAI)).unwrap();
        let debug = format!("{client:?}");
        assert!(!debug.contains("sk-secret"), "debug leaked key: {debug}");
        assert!(debug.contains("***"));
    }

    #[test]
    fn unsupported_proxy_scheme_is_rejected() {
        let mut p = profile(LlmFormat::OPENAI);
        p.proxy = Some("socks5://127.0.0.1:1080".into());
        assert!(matches!(
            LlmClient::from_profile(&p),
            Err(LlmError::Invalid(_))
        ));
    }

    #[test]
    fn parses_openai_response() {
        let value = json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "hello",
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {"name": "lookup", "arguments": "{\"q\":\"rust\"}"}
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 5,
                "prompt_tokens_details": {"cached_tokens": 4},
                "cache_creation_input_tokens": 2
            }
        });
        let parsed = parse_openai(&value).unwrap();
        assert_eq!(parsed.content, "hello");
        assert_eq!(parsed.tool_calls.len(), 1);
        assert_eq!(parsed.tool_calls[0].id, "call_1");
        assert_eq!(parsed.tool_calls[0].name, "lookup");
        assert_eq!(parsed.tool_calls[0].arguments["q"], "rust");
        assert_eq!(parsed.usage.input_tokens, 10);
        assert_eq!(parsed.usage.output_tokens, 5);
        assert_eq!(parsed.usage.cache_read_tokens, 4);
        assert_eq!(parsed.usage.cache_write_tokens, 2);
        assert_eq!(parsed.stop_reason.as_deref(), Some("tool_calls"));
    }

    #[test]
    fn parses_anthropic_response() {
        let value = json!({
            "content": [
                {"type": "text", "text": "hi "},
                {"type": "text", "text": "there"},
                {"type": "tool_use", "id": "t1", "name": "lookup", "input": {"q": "x"}}
            ],
            "usage": {
                "input_tokens": 3,
                "output_tokens": 2,
                "cache_read_input_tokens": 7,
                "cache_creation_input_tokens": 1
            },
            "stop_reason": "tool_use"
        });
        let parsed = parse_anthropic(&value).unwrap();
        assert_eq!(parsed.content, "hi there");
        assert_eq!(parsed.tool_calls.len(), 1);
        assert_eq!(parsed.tool_calls[0].id, "t1");
        assert_eq!(parsed.tool_calls[0].name, "lookup");
        assert_eq!(parsed.tool_calls[0].arguments["q"], "x");
        assert_eq!(parsed.usage.input_tokens, 3);
        assert_eq!(parsed.usage.output_tokens, 2);
        assert_eq!(parsed.usage.cache_read_tokens, 7);
        assert_eq!(parsed.usage.cache_write_tokens, 1);
        assert_eq!(parsed.stop_reason.as_deref(), Some("tool_use"));
    }

    #[test]
    fn parses_responses_response() {
        let value = json!({
            "output": [
                {"type": "message", "content": [{"type": "output_text", "text": "done"}]},
                {"type": "function_call", "call_id": "fc1", "name": "lookup", "arguments": "{\"a\":1}"}
            ],
            "usage": {
                "input_tokens": 8,
                "output_tokens": 4,
                "input_tokens_details": {"cached_tokens": 2}
            },
            "status": "completed"
        });
        let parsed = parse_responses(&value).unwrap();
        assert_eq!(parsed.content, "done");
        assert_eq!(parsed.tool_calls.len(), 1);
        assert_eq!(parsed.tool_calls[0].id, "fc1");
        assert_eq!(parsed.tool_calls[0].name, "lookup");
        assert_eq!(parsed.tool_calls[0].arguments["a"], 1);
        assert_eq!(parsed.usage.input_tokens, 8);
        assert_eq!(parsed.usage.output_tokens, 4);
        assert_eq!(parsed.usage.cache_read_tokens, 2);
        assert_eq!(parsed.stop_reason.as_deref(), Some("completed"));
    }

    #[test]
    fn responses_incomplete_reason_is_stop_reason() {
        let value = json!({
            "output": [],
            "status": "incomplete",
            "incomplete_details": {"reason": "max_output_tokens"}
        });
        let parsed = parse_responses(&value).unwrap();
        assert_eq!(parsed.stop_reason.as_deref(), Some("max_output_tokens"));
    }

    #[test]
    fn openai_missing_choice_is_parse_error() {
        assert!(matches!(parse_openai(&json!({})), Err(LlmError::Parse(_))));
    }
}
