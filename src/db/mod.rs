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
    conn.execute_batch(include_str!("../../migrations/003_competition_state.sql"))?;
    conn.execute_batch(include_str!("../../migrations/004_branding.sql"))?;
    conn.execute_batch(include_str!("../../migrations/005_flag_cipher.sql"))?;
    add_column_if_missing(conn, "challenges", "flag_ciphertext", "TEXT")?;
    crate::flag_cipher::ensure_key(conn)?;
    Ok(())
}

/// `ALTER TABLE ... ADD COLUMN` fails if the column exists, so check first.
fn add_column_if_missing(
    conn: &rusqlite::Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> Result<(), Error> {
    let exists = conn
        .prepare(&format!("PRAGMA table_info({table})"))?
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?
        .iter()
        .any(|name| name == column);
    if !exists {
        conn.execute_batch(&format!(
            "ALTER TABLE {table} ADD COLUMN {column} {definition}"
        ))?;
    }
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

#[cfg(test)]
mod migration_tests {
    use super::*;

    #[test]
    fn upgrading_a_pre_sprint_16_database_adds_cipher_once() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        // Schema as it was before Sprint 16, with an existing challenge.
        for sql in [
            include_str!("../../migrations/001_initial.sql"),
            include_str!("../../migrations/002_audit_log.sql"),
            include_str!("../../migrations/003_competition_state.sql"),
            include_str!("../../migrations/004_branding.sql"),
        ] {
            conn.execute_batch(sql).unwrap();
        }
        conn.execute(
            "INSERT INTO challenges (slug, title, description, category, flag_hash, flag_salt,
                points, created_at)
             VALUES ('old', 'Old', 'd', 'web', 'hash', 'salt', 100, 1)",
            [],
        )
        .unwrap();

        run_migrations(&conn).unwrap();
        let key: Vec<u8> = conn
            .query_row("SELECT key FROM flag_cipher", [], |row| row.get(0))
            .unwrap();
        let legacy: Option<String> = conn
            .query_row(
                "SELECT flag_ciphertext FROM challenges WHERE slug = 'old'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            legacy, None,
            "existing challenges have no ciphertext to backfill"
        );

        run_migrations(&conn).unwrap();
        let columns: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('challenges') WHERE name = 'flag_ciphertext'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(columns, 1);
        let key_again: Vec<u8> = conn
            .query_row("SELECT key FROM flag_cipher", [], |row| row.get(0))
            .unwrap();
        assert_eq!(key, key_again);
    }
}
