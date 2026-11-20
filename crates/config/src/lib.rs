//! Runtime configuration loading — Rust port of ARTEX `config/`.
//!
//! Loads config from a JSON file with environment variables taking precedence.
//! Behavior-preserving port: resolution order, error semantics, DSN assembly and
//! redaction match the Go original (`config/config.go`).

use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

/// Error type for config loading. Mirrors the Go error strings where callers
/// might observe them.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("read config file {path}: {source}")]
    ReadError {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("parse config file {path}: {source}")]
    ParseError {
        path: String,
        #[source]
        source: serde_json::Error,
    },
    #[error(
        "no database config found: the environment variable ARTEX_PG_DSN is not set, and config file {path} provides no database (dsn or host/user/dbname). Create the config file or set the environment variable and retry"
    )]
    NoDatabase { path: String },
}

/// PostgreSQL connection config. Either set `dsn` directly, or set the component
/// fields and a DSN is assembled from them.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Database {
    #[serde(default)]
    pub dsn: String,
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub port: i64,
    #[serde(default)]
    pub user: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub dbname: String,
    #[serde(default)]
    pub sslmode: String,
}

/// On-disk config file shape.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Config {
    #[serde(default)]
    pub database: Database,
    #[serde(default)]
    pub skill_dir: String,
}

/// BaseDir is the directory that anchors all runtime artifacts (config.json and
/// the data/ store). It is the directory the running binary lives in.
///
/// Rust binaries are not throwaway (unlike `go run`), so this returns the
/// executable's directory, falling back to "." when the executable path cannot
/// be determined.
pub fn base_dir() -> PathBuf {
    match env::current_exe() {
        Ok(exe) => match exe.parent() {
            Some(dir) => dir.to_path_buf(),
            None => PathBuf::from("."),
        },
        Err(_) => PathBuf::from("."),
    }
}

/// KeyDir holds jwt.key and secret.key: ARTEX_JWT_KEY_DIR or the base dir.
/// Never the browsable data dir. Returned absolute.
pub fn key_dir() -> PathBuf {
    let dir = match env::var("ARTEX_JWT_KEY_DIR") {
        Ok(v) if !v.trim().is_empty() => PathBuf::from(v.trim()),
        _ => base_dir(),
    };
    // Make absolute like the Go filepath.Abs.
    match fs::canonicalize(&dir) {
        Ok(abs) => abs,
        Err(_) => absolutize(&dir),
    }
}

/// Best-effort absolutize (join with cwd) mirroring filepath.Abs when the path
/// does not exist yet.
fn absolutize(p: &Path) -> PathBuf {
    if p.is_absolute() {
        p.to_path_buf()
    } else if let Ok(cwd) = env::current_dir() {
        cwd.join(p)
    } else {
        p.to_path_buf()
    }
}

/// Path returns the config file path. Resolution order:
/// 1. env ARTEX_CONFIG (explicit override)
/// 2. ./config.json in the current working directory
/// 3. config.json next to the executable
///
/// The first existing file wins. If none exist, the CWD path is returned.
pub fn path() -> PathBuf {
    if let Ok(v) = env::var("ARTEX_CONFIG") {
        let v = v.trim();
        if !v.is_empty() {
            return PathBuf::from(v);
        }
    }
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(cwd) = env::current_dir() {
        candidates.push(cwd.join("config.json"));
    }
    candidates.push(base_dir().join("config.json"));
    for c in &candidates {
        if c.exists() {
            return c.clone();
        }
    }
    candidates
        .into_iter()
        .next()
        .unwrap_or_else(|| PathBuf::from("config.json"))
}

/// Load reads and parses the config file. A missing file yields a zero Config
/// (callers fall back to defaults). An existing file that cannot be read or
/// parsed is an error.
pub fn load() -> Result<Config, ConfigError> {
    let path = path();
    let b = match fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Config::default()),
        Err(e) => {
            return Err(ConfigError::ReadError {
                path: path.display().to_string(),
                source: e,
            })
        }
    };
    match serde_json::from_slice(&b) {
        Ok(c) => Ok(c),
        Err(e) => Err(ConfigError::ParseError {
            path: path.display().to_string(),
            source: e,
        }),
    }
}

/// SkillDir returns the skill root directory with precedence:
/// env ARTEX_SKILL_DIR > config file (skill_dir) > BaseDir()/skills.
/// The directory is created if it does not exist.
pub fn skill_dir() -> Result<PathBuf, ConfigError> {
    let d: PathBuf = if let Ok(v) = env::var("ARTEX_SKILL_DIR") {
        let v = v.trim();
        if !v.is_empty() {
            PathBuf::from(v)
        } else {
            skill_dir_fallback()?
        }
    } else {
        skill_dir_fallback()?
    };
    let _ = fs::create_dir_all(&d);
    Ok(d)
}

fn skill_dir_fallback() -> Result<PathBuf, ConfigError> {
    let c = load()?;
    let v = c.skill_dir.trim();
    if !v.is_empty() {
        Ok(PathBuf::from(v))
    } else {
        Ok(base_dir().join("skills"))
    }
}

/// PostgresDSN resolves the connection string with precedence:
/// env ARTEX_PG_DSN > config file (database.dsn, or assembled from fields).
/// There is NO built-in fallback: returns an error when neither source supplies
/// a database config. `source` describes where the DSN came from.
pub fn postgres_dsn() -> Result<(String, String), ConfigError> {
    if let Ok(v) = env::var("ARTEX_PG_DSN") {
        let v = v.trim();
        if !v.is_empty() {
            return Ok((
                v.to_string(),
                "environment variable ARTEX_PG_DSN".to_string(),
            ));
        }
    }
    let c = load()?;
    let db = c.database;
    if !db.dsn.trim().is_empty() {
        return Ok((
            db.dsn.clone(),
            format!("config file {} (database.dsn)", path().display()),
        ));
    }
    if !db.host.is_empty() || !db.dbname.is_empty() || !db.user.is_empty() {
        return Ok((
            build_dsn(&db),
            format!("config file {} (database fields)", path().display()),
        ));
    }
    Err(ConfigError::NoDatabase {
        path: path().display().to_string(),
    })
}

/// Assemble a Postgres DSN from component fields, applying Go's defaults
/// (host 127.0.0.1, port 5432, sslmode disable).
pub fn build_dsn(d: &Database) -> String {
    let host = if d.host.is_empty() {
        "127.0.0.1"
    } else {
        &d.host
    };
    let port = if d.port == 0 { 5432 } else { d.port };
    let ssl = if d.sslmode.is_empty() {
        "disable"
    } else {
        &d.sslmode
    };

    let mut userinfo = String::new();
    if !d.user.is_empty() {
        if !d.password.is_empty() {
            userinfo = format!("{}:{}@", urlencode(&d.user), urlencode(&d.password));
        } else {
            userinfo = format!("{}@", urlencode(&d.user));
        }
    }

    // url.Values{"sslmode": {ssl}}.Encode() produces "sslmode=<value>".
    format!(
        "postgres://{}{}:{}/{}?sslmode={}",
        userinfo,
        host,
        port,
        d.dbname,
        urlencode(ssl)
    )
}

/// Percent-encode a value the way Go's url.UserPassword / url.Values.Encode do
/// for the characters we care about (user, password, sslmode). Uses a minimal
/// but correct encoder for RFC 3986 unreserved + sub-delims handling.
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

/// Redact returns a redacted view of the DSN (password masked) for logging.
/// Falls back to the input string when it cannot be parsed.
pub fn redact(dsn: &str) -> String {
    match url::Url::parse(dsn) {
        Ok(mut u) => {
            if u.password().is_some() {
                let username = u.username().to_string();
                let _ = u.set_password(Some("****"));
                let _ = u.set_username(&username);
            }
            u.to_string()
        }
        Err(_) => dsn.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_dsn_defaults() {
        let d = Database {
            host: "".into(),
            port: 0,
            user: "artex".into(),
            password: "pw".into(),
            dbname: "artex".into(),
            sslmode: "".into(),
            ..Default::default()
        };
        assert_eq!(
            build_dsn(&d),
            "postgres://artex:pw@127.0.0.1:5432/artex?sslmode=disable"
        );
    }

    #[test]
    fn build_dsn_full() {
        let d = Database {
            host: "db.local".into(),
            port: 6543,
            user: "u".into(),
            password: "p@ss".into(),
            dbname: "mydb".into(),
            sslmode: "require".into(),
            ..Default::default()
        };
        assert_eq!(
            build_dsn(&d),
            "postgres://u:p%40ss@db.local:6543/mydb?sslmode=require"
        );
    }

    #[test]
    fn build_dsn_user_only() {
        let d = Database {
            host: "h".into(),
            port: 5432,
            user: "u".into(),
            password: "".into(),
            dbname: "db".into(),
            sslmode: "disable".into(),
            ..Default::default()
        };
        assert_eq!(build_dsn(&d), "postgres://u@h:5432/db?sslmode=disable");
    }

    #[test]
    fn redact_masks_password() {
        assert_eq!(
            redact("postgres://user:secret@host:5432/db?sslmode=disable"),
            "postgres://user:****@host:5432/db?sslmode=disable"
        );
    }

    #[test]
    fn redact_no_password() {
        assert_eq!(redact("postgres://host:5432/db"), "postgres://host:5432/db");
    }

    #[test]
    fn redact_invalid_passthrough() {
        assert_eq!(redact("not a url"), "not a url");
    }
}
