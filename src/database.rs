// FeralCTF - Database module
// Implements FERALCTF_SPEC.md section 5

use rusqlite::Connection;
use std::path::PathBuf;

/// Database connection pool wrapper
pub struct DatabaseState {
    /// Path to the SQLite database file
    pub db_path: PathBuf,
    /// Active database connection
    pub conn: Connection,
}

impl DatabaseState {
    /// Create a new DatabaseState with an active connection
    pub fn new(db_path: PathBuf) -> Result<Self, Box<dyn std::error::Error>> {
        // Create and run migrations
        let conn = crate::db::init_database(db_path.as_path())?;
        
        Ok(Self { db_path, conn })
    }

    /// Get a reference to the database path
    pub fn db_path(&self) -> &PathBuf {
        &self.db_path
    }

    /// Get a reference to the connection
    pub fn get_conn(&self) -> &Connection {
        &self.conn
    }
}

/// Initialize database connection from path
pub fn init_database(db_path: &std::path::Path) -> Result<Connection, Box<dyn std::error::Error>> {
    let conn = Connection::open(db_path)?;
    use crate::db::connection::MigrationRunner;
    let runner = MigrationRunner::new("migrations");
    runner.run(&conn)?;
    Ok(conn)
}

/// Get a connection from DatabaseState
pub fn get_db_connection(db_state: &DatabaseState) -> &Connection {
    &db_state.conn
}

/// Re-export Connection for legacy API compatibility
pub type Database = Connection;

/// Re-export Connection type alias for DbPool usage
pub type DbPool = Connection;

/// Get a connection reference with type alias
pub type DatabaseRef<'a> = &'a Connection;