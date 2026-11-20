//! Asset & company data layer — port of ARTEX `db/assets.go` + `db/companies.go`.
//!
//! Models the unified `assets` table and the `companies` + `company_scope` tables,
//! with the read/query/count and upsert operations the API layer needs. JSONB
//! columns are carried as text (matching the original's `array_to_json(...)::text`
//! SELECTs) and parsed with serde_json.

use database::Db;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::Row;
use std::collections::BTreeMap;

#[derive(Debug, thiserror::Error)]
pub enum AssetError {
    #[error("sql: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("company not found")]
    CompanyNotFound,
    #[error("company name already exists")]
    CompanyNameConflict,
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

/// One entry in the `auth` array.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AuthItem {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// One entry in the `params` array.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ParamItem {
    pub location: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#type: Option<String>,
}

/// One entry in `open_ports`: `{"port":22,"service":"ssh"}`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PortService {
    pub port: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,
}

/// A row in the `assets` table.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Asset {
    pub id: i64,
    #[serde(rename = "type")]
    pub typ: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub company_id: Option<i64>,
    #[serde(default)]
    pub task_ids: Vec<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root_domain: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ip: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub c_segment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icp: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bound_domains: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub open_ports: Option<Vec<PortService>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub record_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub record_value: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bundle_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_icp: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub favicon_mmh3: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_length: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub technologies: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth: Option<Vec<AuthItem>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Vec<ParamItem>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra: Option<Value>,
    pub last_seen: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_source_summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_source_node_id: Option<i64>,
}

const ASSET_SELECT: &str = "SELECT id, type, company_id, array_to_json(task_ids)::text AS task_ids, \
COALESCE(domain,'') AS domain, COALESCE(root_domain,'') AS root_domain, COALESCE(ip,'') AS ip, \
COALESCE(c_segment::text,'') AS c_segment, port, COALESCE(icp,'') AS icp, \
array_to_json(bound_domains)::text AS bound_domains, array_to_json(open_ports)::text AS open_ports, \
COALESCE(record_type,'') AS record_type, array_to_json(record_value)::text AS record_value, \
COALESCE(bundle_id,'') AS bundle_id, COALESCE(app_name,'') AS app_name, COALESCE(category,'') AS category, \
COALESCE(app_description,'') AS app_description, COALESCE(app_icp,'') AS app_icp, COALESCE(url,'') AS url, \
COALESCE(service_type,'') AS service_type, COALESCE(service_name,'') AS service_name, \
COALESCE(favicon_mmh3,'') AS favicon_mmh3, status_code, content_length, COALESCE(page_title,'') AS page_title, \
array_to_json(technologies)::text AS technologies, array_to_json(auth)::text AS auth, \
COALESCE(method,'') AS method, array_to_json(params)::text AS params, extra::text AS extra, last_seen::text AS last_seen";

/// Parse a nullable JSON text column into an Option<Vec<T>>.
fn parse_vec<T: for<'de> Deserialize<'de>>(raw: &str) -> Option<Vec<T>> {
    if raw.is_empty() {
        return None;
    }
    serde_json::from_str::<Vec<T>>(raw).ok()
}

fn opt_str(s: String) -> Option<String> {
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

fn scan_asset(r: &sqlx::postgres::PgRow) -> Asset {
    // array_to_json(x)::text yields NULL (not '') when x is NULL, so these are
    // read as Option<String>.
    let opt = |name: &str| r.get::<Option<String>, _>(name);
    let s = |name: &str| r.get::<String, _>(name);
    let task_ids = opt("task_ids");
    let bound_domains = opt("bound_domains");
    let open_ports = opt("open_ports");
    let record_value = opt("record_value");
    let techs = opt("technologies");
    let auth = opt("auth");
    let params = opt("params");
    let extra = opt("extra");
    Asset {
        id: r.get("id"),
        typ: r.get("type"),
        company_id: r.get("company_id"),
        task_ids: task_ids
            .as_deref()
            .and_then(parse_vec::<i64>)
            .unwrap_or_default(),
        domain: opt_str(s("domain")),
        root_domain: opt_str(s("root_domain")),
        ip: opt_str(s("ip")),
        c_segment: opt_str(s("c_segment")),
        port: r.get("port"),
        icp: opt_str(s("icp")),
        bound_domains: bound_domains.as_deref().and_then(parse_vec::<String>),
        open_ports: open_ports.as_deref().and_then(parse_vec::<PortService>),
        record_type: opt_str(s("record_type")),
        record_value: record_value.as_deref().and_then(parse_vec::<String>),
        bundle_id: opt_str(s("bundle_id")),
        app_name: opt_str(s("app_name")),
        category: opt_str(s("category")),
        app_description: opt_str(s("app_description")),
        app_icp: opt_str(s("app_icp")),
        url: opt_str(s("url")),
        service_type: opt_str(s("service_type")),
        service_name: opt_str(s("service_name")),
        favicon_mmh3: opt_str(s("favicon_mmh3")),
        status_code: r.get("status_code"),
        content_length: r.get("content_length"),
        page_title: opt_str(s("page_title")),
        technologies: techs.as_deref().and_then(parse_vec::<String>),
        auth: auth.as_deref().and_then(parse_vec::<AuthItem>),
        method: opt_str(s("method")),
        params: params.as_deref().and_then(parse_vec::<ParamItem>),
        extra: extra
            .as_deref()
            .and_then(|v| serde_json::from_str::<Value>(v).ok()),
        last_seen: s("last_seen"),
        task_source: None,
        task_source_summary: None,
        task_source_node_id: None,
    }
}

/// Asset store bound to a DB handle.
pub struct AssetStore {
    db: Db,
}

impl AssetStore {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    /// Total assets grouped by type.
    pub async fn counts_by_type(&self) -> Result<BTreeMap<String, i64>, AssetError> {
        let rows = sqlx::query("SELECT type, COUNT(*) AS n FROM assets GROUP BY type")
            .fetch_all(self.db.pool())
            .await?;
        let mut out = BTreeMap::new();
        for r in rows {
            out.insert(r.get::<String, _>("type"), r.get::<i64, _>("n"));
        }
        Ok(out)
    }

    /// Count assets of a given type.
    pub async fn count_by_type(&self, typ: &str) -> Result<i64, AssetError> {
        let n: i64 = sqlx::query_scalar("SELECT count(*) FROM assets WHERE type = $1")
            .bind(typ)
            .fetch_one(self.db.pool())
            .await?;
        Ok(n)
    }

    /// Query assets of a given type, newest first, with limit/offset.
    pub async fn query_by_type(
        &self,
        typ: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<Asset>, AssetError> {
        let limit = if limit <= 0 { 50 } else { limit };
        let offset = if offset < 0 { 0 } else { offset };
        let sql = format!(
            "{} FROM assets WHERE type = $1 ORDER BY last_seen DESC, id DESC LIMIT $2 OFFSET $3",
            ASSET_SELECT
        );
        let rows = sqlx::query(&sql)
            .bind(typ)
            .bind(limit)
            .bind(offset)
            .fetch_all(self.db.pool())
            .await?;
        Ok(rows.iter().map(scan_asset).collect())
    }

    /// Count assets for a company (optionally filtered by type).
    pub async fn count_by_company(
        &self,
        company_id: i64,
        typ: Option<&str>,
    ) -> Result<i64, AssetError> {
        let n: i64 = match typ {
            Some(t) => {
                sqlx::query_scalar(
                    "SELECT count(*) FROM assets WHERE company_id = $1 AND type = $2",
                )
                .bind(company_id)
                .bind(t)
                .fetch_one(self.db.pool())
                .await?
            }
            None => {
                sqlx::query_scalar("SELECT count(*) FROM assets WHERE company_id = $1")
                    .bind(company_id)
                    .fetch_one(self.db.pool())
                    .await?
            }
        };
        Ok(n)
    }

    /// Count assets for a task (optionally filtered by type).
    pub async fn count_by_task(&self, task_id: i64, typ: Option<&str>) -> Result<i64, AssetError> {
        let n: i64 = match typ {
            Some(t) => {
                sqlx::query_scalar(
                    "SELECT count(*) FROM assets WHERE $1 = ANY(task_ids) AND type = $2",
                )
                .bind(task_id)
                .bind(t)
                .fetch_one(self.db.pool())
                .await?
            }
            None => {
                sqlx::query_scalar("SELECT count(*) FROM assets WHERE $1 = ANY(task_ids)")
                    .bind(task_id)
                    .fetch_one(self.db.pool())
                    .await?
            }
        };
        Ok(n)
    }
}

// ---------------------------------------------------------------------------
// Companies
// ---------------------------------------------------------------------------

/// A row in the `companies` table.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Company {
    pub id: i64,
    pub name: String,
    pub nkey: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logo: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// A row in `company_scope`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ScopeRule {
    pub id: i64,
    pub company_id: i64,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub net: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    pub raw: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Company with its scope rules and asset count (the `ListCompanies` shape).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CompanyWithScope {
    pub id: i64,
    pub name: String,
    pub nkey: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logo: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub asset_count: i64,
    pub scope: Vec<ScopeRule>,
}

/// Normalize a company name to its nkey (lowercase, collapsed whitespace).
pub fn company_nkey(name: &str) -> String {
    name.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Company store bound to a DB handle.
pub struct CompanyStore {
    db: Db,
}

impl CompanyStore {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    /// Get a company by id; None when not found.
    pub async fn get(&self, id: i64) -> Result<Option<Company>, AssetError> {
        let row = sqlx::query(
            "SELECT id, name, nkey, logo, created_at::text AS created_at, updated_at::text AS updated_at \
             FROM companies WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(self.db.pool())
        .await?;
        Ok(row.map(|r| Company {
            id: r.get("id"),
            name: r.get("name"),
            nkey: r.get("nkey"),
            logo: r.get("logo"),
            created_at: r.get("created_at"),
            updated_at: r.get("updated_at"),
        }))
    }

    /// Create or update a company by name. Returns (id, created).
    pub async fn upsert(&self, name: &str, logo: &str) -> Result<(i64, bool), AssetError> {
        let nkey = company_nkey(name);
        let logo_val: Option<&str> = if logo.is_empty() { None } else { Some(logo) };
        let row = sqlx::query(
            "INSERT INTO companies(name, nkey, logo) VALUES ($1, $2, $3) \
             ON CONFLICT (nkey) DO UPDATE SET name = EXCLUDED.name, \
             logo = COALESCE(EXCLUDED.logo, companies.logo), updated_at = now() \
             RETURNING id, (xmax = 0) AS created",
        )
        .bind(name)
        .bind(&nkey)
        .bind(logo_val)
        .fetch_one(self.db.pool())
        .await?;
        let id: i64 = row.get("id");
        let created: bool = row.get("created");
        Ok((id, created))
    }

    /// List all companies with scope and asset count, ordered by name.
    pub async fn list(&self) -> Result<Vec<CompanyWithScope>, AssetError> {
        let rows = sqlx::query(
            "SELECT c.id, c.name, c.nkey, c.logo, c.created_at::text AS created_at, \
             c.updated_at::text AS updated_at, COUNT(DISTINCT a.id) AS asset_count \
             FROM companies c LEFT JOIN assets a ON a.company_id = c.id \
             GROUP BY c.id ORDER BY c.name",
        )
        .fetch_all(self.db.pool())
        .await?;
        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            let id: i64 = r.get("id");
            let scope = self.get_scope(id).await?;
            out.push(CompanyWithScope {
                id,
                name: r.get("name"),
                nkey: r.get("nkey"),
                logo: r.get("logo"),
                created_at: r.get("created_at"),
                updated_at: r.get("updated_at"),
                asset_count: r.get("asset_count"),
                scope,
            });
        }
        Ok(out)
    }

    /// Scope rules for a company, ordered by id.
    pub async fn get_scope(&self, company_id: i64) -> Result<Vec<ScopeRule>, AssetError> {
        let rows = sqlx::query(
            "SELECT id, company_id, kind, COALESCE(domain,'') AS domain, \
             COALESCE(net::text,'') AS net, COALESCE(value,'') AS value, raw, COALESCE(reason,'') AS reason \
             FROM company_scope WHERE company_id = $1 ORDER BY id",
        )
        .bind(company_id)
        .fetch_all(self.db.pool())
        .await?;
        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            let net: String = r.get("net");
            let domain: String = r.get("domain");
            let value: String = r.get("value");
            let reason: String = r.get("reason");
            out.push(ScopeRule {
                id: r.get("id"),
                company_id: r.get("company_id"),
                kind: r.get("kind"),
                domain: opt_str(domain),
                net: opt_str(net),
                value: opt_str(value),
                raw: r.get("raw"),
                reason: opt_str(reason),
            });
        }
        Ok(out)
    }

    /// Delete a company (without its assets).
    pub async fn delete(&self, id: i64) -> Result<(), AssetError> {
        let res = sqlx::query("DELETE FROM companies WHERE id = $1")
            .bind(id)
            .execute(self.db.pool())
            .await?;
        if res.rows_affected() == 0 {
            return Err(AssetError::CompanyNotFound);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nkey_normalization() {
        assert_eq!(company_nkey("  Acme   Corp "), "acme corp");
        assert_eq!(company_nkey("Example.com"), "example.com");
    }
}
