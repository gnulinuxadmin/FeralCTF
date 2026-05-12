// FeralCTF - Database module
// Sprint 1: r2d2 connection pool, WAL mode init, migration runner.

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

/// Execute all .sql migration files idempotently against an open connection.
/// Safe to call on an existing database — all DDL uses IF NOT EXISTS.
pub fn run_migrations(conn: &rusqlite::Connection) -> Result<(), Error> {
    connection::MigrationRunner::new("migrations").run(conn)
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
