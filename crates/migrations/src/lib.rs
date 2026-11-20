//! Embedded ARTEX schema + versioned migrations — port of `db/migrate.go`.
//!
//! Pure logic (loading, naming validation, SHA-256 checksums, fail-closed
//! validation). The actual application against a live Postgres connection lives
//! in the `database` crate, which consumes this crate's data.
//!
//! The SQL files are copied verbatim from `ARTEX/db/` so the resulting database
//! schema is byte-identical to the original (compatibility requirement).

use sha2::{Digest, Sha256};
use std::fmt;

/// The idempotent base schema, executed on every startup.
pub const SCHEMA_SQL: &str = include_str!("../sql/schema.sql");

/// The advisory-lock key used to serialize schema migration across instances.
pub const SCHEMA_MIGRATION_LOCK_KEY: i64 = 7_337_741_001;

/// DDL for the migrations bookkeeping table (identical to the original).
pub const SCHEMA_MIGRATIONS_TABLE: &str = "
CREATE TABLE IF NOT EXISTS schema_migrations (
    version    INTEGER PRIMARY KEY,
    name       TEXT,
    checksum   TEXT,
    applied_at TIMESTAMPTZ DEFAULT now()
);
";

/// Embedded (name, content) pairs of versioned migration files, sorted by version.
/// Add new migrations here as `NNNN_slug.sql` files under `sql/` plus an
/// `include_str!` entry.
const MIGRATION_FILES: &[(&str, &str)] = &[
    (
        "0001_llm_records.sql",
        include_str!("../sql/0001_llm_records.sql"),
    ),
    (
        "0002_llm_usage.sql",
        include_str!("../sql/0002_llm_usage.sql"),
    ),
];

/// A loaded versioned migration.
#[derive(Debug, Clone, PartialEq)]
pub struct Migration {
    pub version: u32,
    /// File name, e.g. `0001_llm_records.sql`.
    pub name: String,
    pub sql: String,
    /// Hex SHA-256 of the file content.
    pub checksum: String,
}

/// Errors from loading/validating migrations.
#[derive(Debug, thiserror::Error)]
pub enum MigrationError {
    #[error("migration file {0}: name must be NNNN_slug.sql")]
    BadName(String),
    #[error("migration file {0}: version must be > 0")]
    ZeroVersion(String),
    #[error("migration version {version:04} is used by both {a} and {b}")]
    DuplicateVersion { version: u32, a: String, b: String },
}

impl fmt::Display for Migration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}_{}", self.version, self.name)
    }
}

/// Parse a `NNNN_slug.sql` filename into (version, is_valid).
fn parse_name(name: &str) -> Result<u32, MigrationError> {
    let stem = match name.strip_suffix(".sql") {
        Some(s) => s,
        None => return Err(MigrationError::BadName(name.to_string())),
    };
    let (ver_str, slug) = match stem.split_once('_') {
        Some((v, s)) => (v, s),
        None => return Err(MigrationError::BadName(name.to_string())),
    };
    if ver_str.len() != 4 || !ver_str.chars().all(|c| c.is_ascii_digit()) {
        return Err(MigrationError::BadName(name.to_string()));
    }
    if slug.is_empty()
        || !slug
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    {
        return Err(MigrationError::BadName(name.to_string()));
    }
    let version: u32 = ver_str
        .parse()
        .map_err(|_| MigrationError::BadName(name.to_string()))?;
    if version == 0 {
        return Err(MigrationError::ZeroVersion(name.to_string()));
    }
    Ok(version)
}

/// SHA-256 hex of a byte slice.
pub fn checksum_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut s = String::with_capacity(digest.len() * 2);
    for b in digest {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

/// Load all embedded migrations, sorted by version. Fails on bad names,
/// non-positive versions, or duplicate versions.
pub fn load_migrations() -> Result<Vec<Migration>, MigrationError> {
    let mut out: Vec<Migration> = Vec::new();
    let mut seen: std::collections::HashMap<u32, String> = std::collections::HashMap::new();
    for (name, content) in MIGRATION_FILES {
        let version = parse_name(name)?;
        if let Some(prev) = seen.get(&version) {
            return Err(MigrationError::DuplicateVersion {
                version,
                a: prev.clone(),
                b: name.to_string(),
            });
        }
        seen.insert(version, name.to_string());
        out.push(Migration {
            version,
            name: name.to_string(),
            sql: content.to_string(),
            checksum: checksum_hex(content.as_bytes()),
        });
    }
    out.sort_by_key(|m| m.version);
    Ok(out)
}

/// A row read back from `schema_migrations`.
#[derive(Debug, Clone, PartialEq)]
pub struct AppliedMigration {
    pub version: u32,
    pub name: String,
    pub checksum: String,
}

/// Fail-closed validation of already-applied migrations against the files this
/// binary ships. Returns an error when:
/// - the DB records a version not shipped by this binary;
/// - an applied migration's file name differs;
/// - an applied migration's checksum changed (published migrations are immutable).
pub fn validate_applied(
    migrations: &[Migration],
    applied: &[AppliedMigration],
) -> Result<(), String> {
    let by_version: std::collections::HashMap<u32, &Migration> =
        migrations.iter().map(|m| (m.version, m)).collect();

    for row in applied {
        let m = by_version.get(&row.version).ok_or_else(|| {
            format!(
                "migration {:04} ({}) is applied in the database but missing from this binary",
                row.version, row.name
            )
        })?;
        if row.name != m.name {
            return Err(format!(
                "migration {:04}: database records file {} but this binary has {}",
                row.version, row.name, m.name
            ));
        }
        if row.checksum != m.checksum {
            return Err(format!(
                "migration {:04} ({}): checksum changed since it was applied (database {}, file {}); published migrations must not be edited",
                row.version, m.name, row.checksum, m.checksum
            ));
        }
    }

    // A pending version below the newest applied one would run out of order.
    if let Some(max_applied) = applied.iter().map(|a| a.version).max() {
        for m in migrations {
            let done = applied.iter().any(|a| a.version == m.version);
            if !done && m.version < max_applied {
                let max_row = applied.iter().find(|a| a.version == max_applied).unwrap();
                return Err(format!(
                    "migration {} ({:04}) is older than the applied migration {} ({:04}); migrations must be appended, never inserted",
                    m.name, m.version, max_row.name, max_applied
                ));
            }
        }
    }

    Ok(())
}

/// The subset of migrations that still need to be applied, in version order.
pub fn pending<'a>(
    migrations: &'a [Migration],
    applied: &[AppliedMigration],
) -> Vec<&'a Migration> {
    migrations
        .iter()
        .filter(|m| !applied.iter().any(|a| a.version == m.version))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_embeds() {
        assert!(SCHEMA_SQL.contains("CREATE TABLE IF NOT EXISTS"));
        // schema.sql is the full original schema.
        assert!(SCHEMA_SQL.len() > 80_000);
    }

    #[test]
    fn load_sorted() {
        let ms = load_migrations().unwrap();
        assert_eq!(ms.len(), 2);
        assert_eq!(ms[0].version, 1);
        assert_eq!(ms[1].version, 2);
        assert_eq!(ms[0].name, "0001_llm_records.sql");
    }

    #[test]
    fn checksum_is_sha256() {
        // echo -n "abc" | sha256sum
        assert_eq!(
            checksum_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn name_parsing() {
        assert_eq!(parse_name("0001_llm_records.sql").unwrap(), 1);
        assert!(parse_name("llm_records.sql").is_err());
        assert!(parse_name("0001_LLM.sql").is_err());
        assert!(parse_name("0000_x.sql").is_err());
        assert!(parse_name("0001_x.txt").is_err());
    }

    #[test]
    fn validate_detects_missing() {
        let ms = load_migrations().unwrap();
        let applied = vec![AppliedMigration {
            version: 9,
            name: "9999_x.sql".into(),
            checksum: "x".into(),
        }];
        assert!(validate_applied(&ms, &applied).is_err());
    }

    #[test]
    fn validate_detects_checksum_change() {
        let ms = load_migrations().unwrap();
        let applied = vec![AppliedMigration {
            version: 1,
            name: ms[0].name.clone(),
            checksum: "deadbeef".into(),
        }];
        assert!(validate_applied(&ms, &applied).is_err());
    }

    #[test]
    fn validate_ok_when_matching() {
        let ms = load_migrations().unwrap();
        let applied = vec![AppliedMigration {
            version: 1,
            name: ms[0].name.clone(),
            checksum: ms[0].checksum.clone(),
        }];
        assert!(validate_applied(&ms, &applied).is_ok());
        assert_eq!(pending(&ms, &applied).len(), 1);
    }
}
