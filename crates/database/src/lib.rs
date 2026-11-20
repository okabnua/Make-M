//! PostgreSQL data layer — port of ARTEX `db/` (db.go + migrate.go).
//!
//! Behaviour-preserving port:
//! - `open(dsn)` ensures the target database exists, opens a pool capped at
//!   `MAX_OPEN_CONNS`, applies the idempotent `schema.sql`, applies the
//!   embedded versioned migrations (fail-closed on checksum/name mismatch),
//!   and seeds builtin rows — all under a session-level advisory lock.
//! - `schema_migrations` uses the same table shape, so the resulting database
//!   is compatible with the original.

use migrations::{
    load_migrations, pending, validate_applied, AppliedMigration, Migration, MigrationError,
    SCHEMA_MIGRATIONS_TABLE, SCHEMA_MIGRATION_LOCK_KEY, SCHEMA_SQL,
};
use sqlx::postgres::{PgConnectOptions, PgPool, PgPoolOptions};
use sqlx::{Connection, Executor, PgConnection, Row};
use std::time::Duration;

pub mod seed;

/// Connection pool cap (mirrors Go `maxOpenConns = 32`).
pub const MAX_OPEN_CONNS: u32 = 32;

/// Errors from the data layer.
#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("sql: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("migration: {0}")]
    Migration(#[from] MigrationError),
    #[error("apply schema: {0}")]
    Schema(String),
    #[error("apply migrations: {0}")]
    ApplyMigrations(String),
    #[error("advisory lock: {0}")]
    AdvisoryLock(String),
    #[error("advisory unlock: {0}")]
    AdvisoryUnlock(String),
    #[error("ping postgres ({dsn}): {source}")]
    Ping {
        dsn: String,
        #[source]
        source: sqlx::Error,
    },
}

/// The shared database handle wrapping a Postgres pool.
#[derive(Clone)]
pub struct Db {
    pool: PgPool,
}

impl Db {
    /// Access the underlying pool.
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Get a setting value; `None` when unset.
    pub async fn get_setting(&self, key: &str) -> Result<Option<String>, sqlx::Error> {
        let mut conn = self.pool.acquire().await?;
        seed::get_setting(&mut conn, key).await
    }

    /// Upsert a setting value.
    pub async fn set_setting(&self, key: &str, value: &str) -> Result<(), sqlx::Error> {
        let mut conn = self.pool.acquire().await?;
        seed::set_setting(&mut conn, key, value).await
    }

    /// Insert a setting only when absent; returns whether it was inserted.
    pub async fn insert_setting_if_absent(
        &self,
        key: &str,
        value: &str,
    ) -> Result<bool, sqlx::Error> {
        let mut conn = self.pool.acquire().await?;
        seed::insert_setting_if_absent(&mut conn, key, value).await
    }
}

/// Returns true when the error is a Postgres deadlock (SQLSTATE 40P01).
fn is_deadlock(err: &sqlx::Error) -> bool {
    if let sqlx::Error::Database(db) = err {
        return db.code().as_deref() == Some("40P01");
    }
    false
}

/// Deadlock retry delays (100ms, 250ms, 500ms, 1s).
const DEADLOCK_RETRY_DELAYS: [Duration; 4] = [
    Duration::from_millis(100),
    Duration::from_millis(250),
    Duration::from_millis(500),
    Duration::from_secs(1),
];

/// Apply the idempotent schema, retrying on Postgres deadlocks.
async fn apply_schema_with_retry(conn: &mut PgConnection) -> Result<(), DbError> {
    let mut attempt = 0usize;
    loop {
        // Multi-statement DDL: raw_sql allows several statements in one call.
        match conn.execute(sqlx::raw_sql(SCHEMA_SQL)).await {
            Ok(_) => return Ok(()),
            Err(e) => {
                if !is_deadlock(&e) || attempt >= DEADLOCK_RETRY_DELAYS.len() {
                    return Err(DbError::Schema(e.to_string()));
                }
                tokio::time::sleep(DEADLOCK_RETRY_DELAYS[attempt]).await;
                attempt += 1;
                continue;
            }
        }
    }
}

/// Read the applied migrations from `schema_migrations` on `conn`.
async fn read_applied(conn: &mut PgConnection) -> Result<Vec<AppliedMigration>, DbError> {
    // Ensure the bookkeeping table exists.
    conn.execute(SCHEMA_MIGRATIONS_TABLE).await?;
    let rows = sqlx::query(
        "SELECT version, COALESCE(name,'') AS name, COALESCE(checksum,'') AS checksum FROM schema_migrations ORDER BY version",
    )
    .fetch_all(&mut *conn)
    .await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let version: i32 = r.get("version");
        let name: String = r.get("name");
        let checksum: String = r.get("checksum");
        out.push(AppliedMigration {
            version: version as u32,
            name,
            checksum,
        });
    }
    Ok(out)
}

/// Apply a single migration in its own transaction, recording it in
/// `schema_migrations`.
async fn apply_one(conn: &mut PgConnection, m: &Migration) -> Result<(), DbError> {
    let mut tx = conn.begin().await?;
    tx.execute(sqlx::raw_sql(&m.sql)).await.map_err(|e| {
        DbError::ApplyMigrations(format!("migration {:04} ({}): {e}", m.version, m.name))
    })?;
    sqlx::query("INSERT INTO schema_migrations(version, name, checksum) VALUES ($1, $2, $3)")
        .bind(m.version as i32)
        .bind(&m.name)
        .bind(&m.checksum)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            DbError::ApplyMigrations(format!("record {:04} ({}): {e}", m.version, m.name))
        })?;
    tx.commit().await?;
    Ok(())
}

/// Bring the database up to the shipped migrations, fail-closed on any
/// mismatch between the DB's recorded migrations and the shipped files.
pub async fn apply_migrations(conn: &mut PgConnection) -> Result<(), DbError> {
    let migrations = load_migrations()?;
    let applied = read_applied(conn).await?;
    validate_applied(&migrations, &applied).map_err(DbError::ApplyMigrations)?;
    for m in pending(&migrations, &applied) {
        apply_one(conn, m).await?;
    }
    Ok(())
}

/// Parse a Postgres DSN into connect options (best-effort; unparseable DSNs fall
/// back to letting sqlx surface the error).
fn connect_options(dsn: &str) -> PgConnectOptions {
    dsn.parse::<PgConnectOptions>()
        .unwrap_or_else(|_| PgConnectOptions::new())
}

/// Ensure the target database exists by connecting to the `postgres`
/// maintenance database and creating it if absent. Best-effort: unparseable
/// DSNs and connection failures return `Ok` and let `open` surface the real
/// error.
async fn ensure_database(dsn: &str) -> Result<(), DbError> {
    let opts = match dsn.parse::<PgConnectOptions>() {
        Ok(o) => o,
        Err(_) => return Ok(()),
    };
    let db_name = opts
        .get_database()
        .map(|s| s.to_string())
        .unwrap_or_default();
    if db_name.is_empty() || db_name == "postgres" {
        return Ok(());
    }
    // Connect to the maintenance database instead.
    let mut admin_opts = opts.clone();
    admin_opts = admin_opts.database("postgres");
    let mut admin = match PgConnection::connect_with(&admin_opts).await {
        Ok(c) => c,
        Err(_) => return Ok(()),
    };
    let exists: Option<bool> =
        sqlx::query_scalar("SELECT true FROM pg_database WHERE datname = $1")
            .bind(&db_name)
            .fetch_optional(&mut admin)
            .await
            .ok()
            .flatten();
    if exists != Some(true) {
        // CREATE DATABASE cannot be parameterized; the name is quoted.
        let create = format!("CREATE DATABASE \"{}\"", db_name.replace('"', "\"\""));
        if let Err(e) = admin.execute(sqlx::raw_sql(&create)).await {
            return Err(DbError::Schema(format!(
                "create database {:?}: {e}",
                db_name
            )));
        }
    }
    let _ = admin.close().await;
    Ok(())
}

/// Open (and if needed create) the database, apply schema + migrations, and
/// seed builtin rows. All DDL runs under a session-level advisory lock held on
/// one checked-out connection, so concurrent instances initialize in order.
pub async fn open(dsn: &str) -> Result<Db, DbError> {
    ensure_database(dsn).await?;

    let opts = connect_options(dsn);
    let pool = PgPoolOptions::new()
        .max_connections(MAX_OPEN_CONNS)
        .acquire_timeout(Duration::from_secs(30))
        .connect_with(opts)
        .await
        .map_err(|e| DbError::Ping {
            dsn: config::redact(dsn),
            source: e,
        })?;

    // Hold one connection for the whole init (advisory lock + DDL + seed).
    let mut conn = pool.acquire().await?;
    acquire_schema_lock(&mut conn).await?;
    let result = async {
        apply_schema_with_retry(&mut conn).await?;
        apply_migrations(&mut conn).await?;
        seed::seed_builtins(&mut conn).await?;
        Ok::<(), DbError>(())
    }
    .await;
    // Always attempt to release the lock.
    let unlock = release_schema_lock(&mut conn).await;
    result?;
    unlock?;
    Ok(Db { pool })
}

async fn acquire_schema_lock(conn: &mut PgConnection) -> Result<(), DbError> {
    sqlx::query("SELECT pg_advisory_lock($1)")
        .bind(SCHEMA_MIGRATION_LOCK_KEY)
        .execute(&mut *conn)
        .await
        .map_err(|e| DbError::AdvisoryLock(e.to_string()))?;
    Ok(())
}

async fn release_schema_lock(conn: &mut PgConnection) -> Result<(), DbError> {
    sqlx::query("SELECT pg_advisory_unlock($1)")
        .bind(SCHEMA_MIGRATION_LOCK_KEY)
        .execute(&mut *conn)
        .await
        .map_err(|e| DbError::AdvisoryUnlock(e.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deadlock_detection() {
        // Non-db error is not a deadlock.
        let e = sqlx::Error::RowNotFound;
        assert!(!is_deadlock(&e));
    }

    #[test]
    fn connect_options_parses() {
        let o = connect_options("postgres://u:p@h:5432/db?sslmode=disable");
        assert_eq!(o.get_host(), "h");
        assert_eq!(o.get_port(), 5432);
        assert_eq!(o.get_database(), Some("db"));
    }
}
