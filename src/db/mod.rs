// FeralCTF - Database Module
// Implements FERALCTF_SPEC.md section 5

pub mod connection;
pub mod schema;
pub mod queries;

/// Initialize database connection
pub fn init_database(database_path: &std::path::Path) -> Result<rusqlite::Connection, Box<dyn std::error::Error>> {
    let conn = rusqlite::Connection::open(database_path)?;
    let runner = connection::MigrationRunner::new("migrations");
    runner.run(&conn)?;
    Ok(conn)
}

/// Get a new database connection
pub fn get_connection(conn: &rusqlite::Connection) -> Option<&rusqlite::Connection> {
    // This will be implemented with a proper pool later
    Some(conn)
}