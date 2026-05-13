// FeralCTF - Database module
// Implements the SQLite/WAL storage model described in FERALCTF_SPEC.md §1.2.

pub mod connection;
pub mod queries;

use anyhow::Error;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;

pub type DbPool = Pool<SqliteConnectionManager>;
pub type DbConn = r2d2::PooledConnection<SqliteConnectionManager>;

/// Build a connection pool. WAL mode and synchronous=NORMAL are applied to
/// every connection as it is opened. Migrations are run once before returning.
pub fn init_pool(db_path: &str) -> Result<DbPool, Error> {
    let manager = SqliteConnectionManager::file(db_path).with_init(|conn| {
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;
        Ok(())
    });
    let pool = Pool::new(manager)?;
    let conn = pool.get()?;
    run_migrations(&conn)?;
    Ok(pool)
}

/// Execute embedded SQL migrations idempotently against an open connection.
/// Safe to call on an existing database — all DDL uses IF NOT EXISTS.
pub fn run_migrations(conn: &rusqlite::Connection) -> Result<(), Error> {
    conn.execute_batch(include_str!("../../migrations/001_initial.sql"))?;
    conn.execute_batch(include_str!("../../migrations/002_audit_log.sql"))?;
    Ok(())
}

pub fn audit(
    conn: &rusqlite::Connection,
    user_id: i64,
    action: &str,
    target: Option<&str>,
    detail: Option<&str>,
    ip: Option<&str>,
) -> Result<(), Error> {
    // FERALCTF_SPEC.md §6.3: admin actions are recorded with actor, action,
    // target, timestamp, and IP when available.
    conn.execute(
        "INSERT INTO audit_log (user_id, action, target, detail, ip_address, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![
            user_id,
            action,
            target,
            detail,
            ip,
            chrono::Utc::now().timestamp(),
        ],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_migration_idempotent() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        run_migrations(&conn).expect("first run failed");
        run_migrations(&conn).expect("second run failed — not idempotent");
    }
}
