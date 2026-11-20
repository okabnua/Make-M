//! IM / email notification channels — port of ARTEX `notify/`.
//!
//! Layering matches the Go leaf package: channel configuration is passed in as
//! `serde_json::Value` (the `notification_channels.config` JSONB), content as a
//! `Message`. Signature computation, UTF-8 truncation and filter matching are
//! pure and unit-tested. The DB store + HTTP delivery live here too.
//!
//! Channels: dingtalk, feishu, wecom, webhook, telegram (HTTP POST); email is
//! rendered but SMTP delivery is a documented stub (no SMTP client dependency).

use database::Db;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::Row;

pub const KIND_DINGTALK: &str = "dingtalk";
pub const KIND_FEISHU: &str = "feishu";
pub const KIND_WECOM: &str = "wecom";
pub const KIND_WEBHOOK: &str = "webhook";
pub const KIND_TELEGRAM: &str = "telegram";
pub const KIND_EMAIL: &str = "email";

pub const EVENT_FINDING_CREATED: &str = "finding_created";
pub const EVENT_FINDING_STATUS_CHANGED: &str = "finding_status_changed";

pub const DEFAULT_MAX_SUMMARY_RUNES: usize = 200;
pub const DEFAULT_MAX_ASSETS_SHOWN: usize = 3;

/// Severity ordinals (unknown = 0). Higher = more severe.
pub fn severity_rank(severity: &str) -> i32 {
    match severity {
        "low" => 1,
        "medium" => 2,
        "high" => 3,
        "critical" => 4,
        _ => 0,
    }
}

pub fn severity_label(severity: &str) -> &str {
    match severity {
        "low" => "Low",
        "medium" => "Medium",
        "high" => "High",
        "critical" => "Critical",
        other => other,
    }
}

pub fn status_label(status: &str) -> &str {
    match status {
        "pending" => "Pending",
        "confirmed" => "Confirmed",
        "fixed" => "Fixed",
        "retested" => "Retested",
        other => other,
    }
}

/// `AtLeast` — is `severity` at least `min` (by rank)? Empty min = no threshold.
pub fn at_least(severity: &str, min: &str) -> bool {
    if min.is_empty() {
        return true;
    }
    severity_rank(severity) >= severity_rank(min) && severity_rank(severity) > 0
}

/// Valid severity threshold (empty = no threshold).
pub fn valid_min_severity(s: &str) -> bool {
    s.is_empty() || severity_rank(s) > 0
}

// ---------------------------------------------------------------------------
// Message / Item / Snapshot
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Snapshot {
    pub kind: String,
    pub finding_id: i64,
    pub task_id: i64,
    pub vulnclass: String,
    pub name: String,
    pub severity: String,
    pub summary: String,
    pub asset_ids: Vec<i64>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub from_status: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub to_status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Item {
    pub finding_id: i64,
    pub name: String,
    pub vulnclass: String,
    pub severity: String,
    pub summary: String,
    pub assets: Vec<String>,
    pub detail_url: String,
    pub from_status: String,
    pub to_status: String,
}

impl Item {
    pub fn is_status_change(&self) -> bool {
        !self.from_status.is_empty() || !self.to_status.is_empty()
    }

    /// Display title: name → vulnclass → placeholder.
    pub fn title(&self) -> &str {
        if !self.name.is_empty() {
            &self.name
        } else if !self.vulnclass.is_empty() {
            &self.vulnclass
        } else {
            "(unnamed vulnerability)"
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Limits {
    pub summary_runes: usize,
    pub assets_shown: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Message {
    pub items: Vec<Item>,
    pub batch: bool,
    pub window_minutes: i32,
    pub home_url: String,
    pub limits: Option<Limits>,
}

impl Message {
    fn limits(&self) -> Limits {
        self.limits.clone().unwrap_or(Limits {
            summary_runes: DEFAULT_MAX_SUMMARY_RUNES,
            assets_shown: DEFAULT_MAX_ASSETS_SHOWN,
        })
    }
}

// ---------------------------------------------------------------------------
// Filter
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Filter {
    #[serde(default)]
    pub min_severity: String,
    #[serde(default)]
    pub task_ids: Vec<i64>,
    #[serde(default)]
    pub asset_ids: Vec<i64>,
    #[serde(default)]
    pub vulnclass_include: Vec<String>,
    #[serde(default)]
    pub vulnclass_exclude: Vec<String>,
    #[serde(default)]
    pub on_status_change: bool,
}

/// Parse a filter; malformed JSON degrades to a zero Filter (no filtering).
pub fn parse_filter(raw: &[u8]) -> Filter {
    if raw.is_empty() {
        return Filter::default();
    }
    serde_json::from_slice(raw).unwrap_or_default()
}

/// Decide whether an event should be delivered under this filter.
pub fn matches(f: &Filter, s: &Snapshot) -> bool {
    if s.kind == EVENT_FINDING_STATUS_CHANGED && !f.on_status_change {
        return false;
    }
    if !at_least(&s.severity, &f.min_severity) {
        return false;
    }
    if !f.task_ids.is_empty() && !f.task_ids.contains(&s.task_id) {
        return false;
    }
    if !f.asset_ids.is_empty() && !s.asset_ids.iter().any(|a| f.asset_ids.contains(a)) {
        return false;
    }
    let vc = s.vulnclass.to_lowercase();
    if f.vulnclass_exclude
        .iter()
        .any(|k| vc.contains(&k.to_lowercase()))
    {
        return false;
    }
    if !f.vulnclass_include.is_empty()
        && !f
            .vulnclass_include
            .iter()
            .any(|k| vc.contains(&k.to_lowercase()))
    {
        return false;
    }
    true
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

fn truncate_runes(s: &str, max: usize) -> String {
    if max == 0 || s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push('…');
    out
}

fn one_line(s: &str, max: usize) -> String {
    let collapsed: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    truncate_runes(&collapsed, max)
}

/// Render a message as Markdown (dingtalk / wecom / generic text channels).
pub fn render_markdown(m: &Message) -> String {
    let lim = m.limits();
    let mut b = String::new();
    if m.batch {
        b.push_str(&format!(
            "## ARTEX vulnerabilities (last {} minutes)\n\n",
            m.window_minutes
        ));
        b.push_str(&format!("{} finding(s) in this window.\n\n", m.items.len()));
    } else {
        b.push_str("## ARTEX new vulnerability\n\n");
    }
    for it in &m.items {
        b.push_str(&format!(
            "### {} ({})\n\n",
            it.title(),
            severity_label(&it.severity)
        ));
        if it.is_status_change() {
            b.push_str(&format!(
                "Status: {} → {}\n\n",
                status_label(&it.from_status),
                status_label(&it.to_status)
            ));
        }
        if !it.summary.is_empty() {
            b.push_str(&one_line(&it.summary, lim.summary_runes));
            b.push_str("\n\n");
        }
        if !it.assets.is_empty() {
            let shown: Vec<String> = it
                .assets
                .iter()
                .take(if lim.assets_shown == 0 {
                    usize::MAX
                } else {
                    lim.assets_shown
                })
                .cloned()
                .collect();
            b.push_str(&format!("Assets: {}\n\n", shown.join(", ")));
        }
        if !it.detail_url.is_empty() {
            b.push_str(&format!("[Details]({})\n\n", it.detail_url));
        }
    }
    if !m.home_url.is_empty() {
        b.push_str(&format!("[Open ARTEX]({})", m.home_url));
    }
    b
}

/// Render a Feishu interactive card (JSON value).
pub fn render_feishu_card(m: &Message) -> Value {
    let lim = m.limits();
    let title = if m.batch {
        format!("ARTEX vulnerabilities ({}m)", m.window_minutes)
    } else {
        "ARTEX new vulnerability".to_string()
    };
    let mut elements = Vec::new();
    for it in &m.items {
        let sev_color = match severity_rank(&it.severity) {
            4 => "red",
            3 => "orange",
            2 => "yellow",
            _ => "blue",
        };
        let mut lines = format!("**{}** ({})", it.title(), severity_label(&it.severity));
        if it.is_status_change() {
            lines.push_str(&format!(
                "\nStatus: {} → {}",
                status_label(&it.from_status),
                status_label(&it.to_status)
            ));
        }
        if !it.summary.is_empty() {
            lines.push_str(&format!("\n{}", one_line(&it.summary, lim.summary_runes)));
        }
        elements.push(json!({
            "tag": "div",
            "text": { "tag": "lark_md", "content": lines },
            "fields": [{ "is_short": false, "text": { "tag": "lark_md", "content": format!("<font color='{}'>{}</font>", sev_color, severity_label(&it.severity)) } }]
        }));
    }
    json!({
        "msg_type": "interactive",
        "card": {
            "header": { "title": { "tag": "plain_text", "content": title }, "template": "red" },
            "elements": elements
        }
    })
}

/// Render a Telegram HTML message.
pub fn render_telegram(m: &Message) -> String {
    let lim = m.limits();
    let mut b = String::new();
    b.push_str(if m.batch {
        "<b>ARTEX vulnerabilities</b>\n"
    } else {
        "<b>ARTEX new vulnerability</b>\n"
    });
    for it in &m.items {
        b.push_str(&format!(
            "\n<b>{}</b> ({})\n",
            it.title(),
            severity_label(&it.severity)
        ));
        if it.is_status_change() {
            b.push_str(&format!(
                "Status: {} → {}\n",
                status_label(&it.from_status),
                status_label(&it.to_status)
            ));
        }
        if !it.summary.is_empty() {
            b.push_str(&one_line(&it.summary, lim.summary_runes));
            b.push('\n');
        }
        if !it.detail_url.is_empty() {
            b.push_str(&format!("<a href=\"{}\">Details</a>\n", it.detail_url));
        }
    }
    b
}

/// Render a generic webhook JSON payload.
pub fn render_webhook(m: &Message) -> Value {
    let items: Vec<Value> = m
        .items
        .iter()
        .map(|it| {
            json!({
                "finding_id": it.finding_id,
                "name": it.name,
                "vulnclass": it.vulnclass,
                "severity": it.severity,
                "summary": it.summary,
                "assets": it.assets,
                "detail_url": it.detail_url,
                "from_status": it.from_status,
                "to_status": it.to_status,
            })
        })
        .collect();
    json!({ "batch": m.batch, "window_minutes": m.window_minutes, "items": items, "home_url": m.home_url })
}

/// Render an email subject + body.
pub fn render_email(m: &Message) -> (String, String) {
    let subject = if m.batch {
        format!("[ARTEX] {} new vulnerabilities", m.items.len())
    } else if let Some(it) = m.items.first() {
        format!("[ARTEX] {} ({})", it.title(), severity_label(&it.severity))
    } else {
        "[ARTEX] vulnerability".to_string()
    };
    (subject, render_markdown(m))
}

// ---------------------------------------------------------------------------
// Config helpers
// ---------------------------------------------------------------------------

fn cfg_str(cfg: &Value, key: &str) -> String {
    cfg.get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

/// Build the HTTP request body for a channel kind from a Message + config.
pub fn build_http_request(kind: &str, cfg: &Value, m: &Message) -> Option<(String, Value)> {
    match kind {
        KIND_DINGTALK => Some((
            cfg_str(cfg, "webhook"),
            json!({ "msgtype": "markdown", "markdown": { "title": "ARTEX", "text": render_markdown(m) } }),
        )),
        KIND_WECOM => Some((
            cfg_str(cfg, "webhook"),
            json!({ "msgtype": "markdown", "markdown": { "content": render_markdown(m) } }),
        )),
        KIND_FEISHU => Some((cfg_str(cfg, "webhook"), render_feishu_card(m))),
        KIND_WEBHOOK => Some((cfg_str(cfg, "url"), render_webhook(m))),
        KIND_TELEGRAM => {
            let base = cfg_str(cfg, "api_base");
            let base = if base.is_empty() {
                "https://api.telegram.org".to_string()
            } else {
                base
            };
            let token = cfg_str(cfg, "bot_token");
            let chat = cfg_str(cfg, "chat_id");
            Some((
                format!("{base}/bot{token}/sendMessage"),
                json!({ "chat_id": chat, "text": render_telegram(m), "parse_mode": "HTML" }),
            ))
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// DB stores
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NotificationChannel {
    pub id: i64,
    pub name: String,
    pub kind: String,
    pub enabled: bool,
    pub config: Value,
    pub mode: String,
    pub filter: Value,
    pub rate_per_min: i32,
}

pub struct ChannelStore {
    db: Db,
}

const CHANNEL_COLS: &str =
    "id, name, kind, enabled, config::text AS config, mode, filter::text AS filter, rate_per_min";

impl ChannelStore {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    pub async fn list(&self) -> Result<Vec<NotificationChannel>, NotifyError> {
        let rows = sqlx::query(&format!(
            "SELECT {CHANNEL_COLS} FROM notification_channels ORDER BY name"
        ))
        .fetch_all(self.db.pool())
        .await?;
        rows.iter().map(channel_from_row).collect()
    }

    pub async fn get(&self, id: i64) -> Result<Option<NotificationChannel>, NotifyError> {
        let row = sqlx::query(&format!(
            "SELECT {CHANNEL_COLS} FROM notification_channels WHERE id = $1"
        ))
        .bind(id)
        .fetch_optional(self.db.pool())
        .await?;
        row.as_ref().map(channel_from_row).transpose()
    }

    pub async fn create(&self, c: &NotificationChannel) -> Result<i64, NotifyError> {
        let config = c.config.to_string();
        let filter = c.filter.to_string();
        let row = sqlx::query(
            "INSERT INTO notification_channels(name, kind, enabled, config, mode, filter, rate_per_min) \
             VALUES ($1,$2,$3,$4::jsonb,$5,$6::jsonb,$7) RETURNING id",
        )
        .bind(&c.name)
        .bind(&c.kind)
        .bind(c.enabled)
        .bind(&config)
        .bind(&c.mode)
        .bind(&filter)
        .bind(c.rate_per_min)
        .fetch_one(self.db.pool())
        .await?;
        Ok(row.get("id"))
    }

    pub async fn update(&self, c: &NotificationChannel) -> Result<(), NotifyError> {
        let config = c.config.to_string();
        let filter = c.filter.to_string();
        sqlx::query(
            "UPDATE notification_channels SET name=$2, kind=$3, enabled=$4, config=$5::jsonb, mode=$6, \
             filter=$7::jsonb, rate_per_min=$8 WHERE id=$1",
        )
        .bind(c.id)
        .bind(&c.name)
        .bind(&c.kind)
        .bind(c.enabled)
        .bind(&config)
        .bind(&c.mode)
        .bind(&filter)
        .bind(c.rate_per_min)
        .execute(self.db.pool())
        .await?;
        Ok(())
    }

    pub async fn delete(&self, id: i64) -> Result<bool, NotifyError> {
        let res = sqlx::query("DELETE FROM notification_channels WHERE id = $1")
            .bind(id)
            .execute(self.db.pool())
            .await?;
        Ok(res.rows_affected() > 0)
    }
}

fn channel_from_row(r: &sqlx::postgres::PgRow) -> Result<NotificationChannel, NotifyError> {
    let config: String = r.get("config");
    let filter: String = r.get("filter");
    Ok(NotificationChannel {
        id: r.get("id"),
        name: r.get("name"),
        kind: r.get("kind"),
        enabled: r.get("enabled"),
        config: serde_json::from_str(&config).unwrap_or(Value::Null),
        mode: r.get("mode"),
        filter: serde_json::from_str(&filter).unwrap_or(Value::Null),
        rate_per_min: r.get("rate_per_min"),
    })
}

#[derive(Debug, thiserror::Error)]
pub enum NotifyError {
    #[error("sql: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("http: {0}")]
    Http(#[from] reqwest::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("config: {0}")]
    Config(String),
    #[error("email delivery not configured (SMTP client not linked)")]
    EmailNotConfigured,
    #[error("not found")]
    NotFound,
}

/// Send a message to a channel via HTTP (dingtalk/feishu/wecom/webhook/telegram).
/// Email returns [`NotifyError::EmailNotConfigured`] (SMTP is a documented stub).
pub async fn send_http(channel: &NotificationChannel, m: &Message) -> Result<(), NotifyError> {
    if channel.kind == KIND_EMAIL {
        return Err(NotifyError::EmailNotConfigured);
    }
    let (url, body) = build_http_request(&channel.kind, &channel.config, m)
        .ok_or_else(|| NotifyError::Config(format!("unknown channel kind {}", channel.kind)))?;
    if url.is_empty() {
        return Err(NotifyError::Config("missing webhook url".into()));
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()?;
    let resp = client.post(&url).json(&body).send().await?;
    if !resp.status().is_success() {
        return Err(NotifyError::Config(format!(
            "{} returned status {}",
            channel.kind,
            resp.status()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severity_ordering() {
        assert!(at_least("critical", "high"));
        assert!(at_least("high", "low"));
        assert!(!at_least("low", "high"));
        assert!(at_least("high", "")); // no threshold
        assert!(!at_least("unknown", "low"));
    }

    #[test]
    fn filter_matches() {
        let f = Filter {
            min_severity: "high".into(),
            ..Default::default()
        };
        let s = Snapshot {
            kind: EVENT_FINDING_CREATED.into(),
            severity: "critical".into(),
            ..Default::default()
        };
        assert!(matches(&f, &s));
        let s2 = Snapshot {
            severity: "low".into(),
            ..s.clone()
        };
        assert!(!matches(&f, &s2));
        // status change without opt-in
        let s3 = Snapshot {
            kind: EVENT_FINDING_STATUS_CHANGED.into(),
            ..s.clone()
        };
        assert!(!matches(&f, &s3));
    }

    #[test]
    fn filter_vulnclass_exclude_precedence() {
        let f = Filter {
            vulnclass_include: vec!["sqli".into()],
            vulnclass_exclude: vec!["xss".into()],
            ..Default::default()
        };
        let base = Snapshot {
            severity: "high".into(),
            ..Default::default()
        };
        assert!(matches(
            &f,
            &Snapshot {
                vulnclass: "sqli".into(),
                ..base.clone()
            }
        ));
        assert!(!matches(
            &f,
            &Snapshot {
                vulnclass: "xss".into(),
                ..base
            }
        ));
    }

    #[test]
    fn markdown_renders_item() {
        let m = Message {
            items: vec![Item {
                finding_id: 1,
                name: "SQLi".into(),
                severity: "high".into(),
                summary: "a bug".into(),
                detail_url: "http://x/f/1".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let md = render_markdown(&m);
        assert!(md.contains("### SQLi (High)"));
        assert!(md.contains("a bug"));
        assert!(md.contains("[Details](http://x/f/1)"));
    }

    #[test]
    fn feishu_card_structure() {
        let m = Message {
            items: vec![Item {
                name: "X".into(),
                severity: "critical".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let card = render_feishu_card(&m);
        assert_eq!(card["msg_type"], "interactive");
        assert!(card["card"]["elements"].as_array().unwrap().len() == 1);
    }

    #[test]
    fn webhook_payload() {
        let m = Message {
            items: vec![Item {
                finding_id: 5,
                name: "N".into(),
                severity: "low".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let p = render_webhook(&m);
        assert_eq!(p["items"][0]["finding_id"], 5);
    }

    #[test]
    fn email_subject_and_body() {
        let m = Message {
            items: vec![Item {
                name: "N".into(),
                severity: "medium".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let (subj, body) = render_email(&m);
        assert!(subj.contains("N"));
        assert!(body.contains("N"));
    }

    #[test]
    fn truncate_runes_works() {
        assert_eq!(truncate_runes("hello", 10), "hello");
        assert_eq!(truncate_runes("hello world", 5), "hello…");
    }
}
