// FeralCTF - Database connection pool
// Implements FERALCTF_SPEC.md section 5.3

use rusqlite::Connection;
use std::path::Path;

/// Simple database connection wrapper
pub struct ConnectionPool {
    conn: Connection,
}

impl ConnectionPool {
    /// Create a new connection pool
    pub fn new(path: &str) -> Self {
        // Create SQLite connection
        let conn = Connection::open(path)
            .expect("Failed to open database");
        
        ConnectionPool { conn }
    }

    /// Get a connection from the pool (returns reference)
    pub fn get_connection(&self) -> &Connection {
        &self.conn
    }

    /// Run all migrations from migrations/ directory
    pub fn run_migrations(&self) -> Result<(), Box<dyn std::error::Error>> {
        let migrations_dir = Path::new("migrations");
        
        // Find all .sql files in migrations directory
        let mut entries = std::fs::read_dir(migrations_dir)?;
        let mut migrations: Vec<_> = entries
            .filter_map(|e| {
                let entry = e.ok()?;
                let path = entry.path();
                if path.extension().map_or(false, |ext| ext == "sql") {
                    Some(path)
                } else {
                    None
                }
            })
            .collect();
        
        // Sort by file number (001, 002, etc.)
        migrations.sort_by(|a, b| {
            let num_a = a.file_stem()
                .and_then(|s| s.to_str())
                .and_then(|s| s.strip_suffix(".sql"))
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(0);
            let num_b = b.file_stem()
                .and_then(|s| s.to_str())
                .and_then(|s| s.strip_suffix(".sql"))
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(0);
            num_a.cmp(&num_b)
        });
        
        // Run each migration
        for migration_path in migrations {
            let sql = std::fs::read_to_string(&migration_path)?;
            self.conn.execute_batch(&sql)?;
            println!("Applied migration: {}", migration_path.display());
        }
        
        Ok(())
    }
}

impl Drop for ConnectionPool {
    fn drop(&mut self) {
        // Connection is dropped automatically by rusqlite
    }
}

/// Migration runner - runs migrations against a connection
pub struct MigrationRunner {
    migrations_dir: std::path::PathBuf,
}

impl MigrationRunner {
    /// Create a new migration runner
    pub fn new(migrations_dir: &str) -> Self {
        MigrationRunner {
            migrations_dir: std::path::PathBuf::from(migrations_dir),
        }
    }

    /// Run all migrations from migrations/ directory
    pub fn run(&self, conn: &Connection) -> Result<(), Box<dyn std::error::Error>> {
        let migrations_dir = &self.migrations_dir;
        
        // Find all .sql files in migrations directory
        let migrations_result = std::fs::read_dir(migrations_dir);
        let mut entries = match migrations_result {
            Ok(entries) => entries,
            Err(e) => {
                return Err(Box::new(e) as Box<dyn std::error::Error>);
            }
        };
        let mut migrations: Vec<_> = entries
            .filter_map(|e| {
                let entry = e.ok()?;
                let path = entry.path();
                if path.extension().map_or(false, |ext| ext == "sql") {
                    Some(path)
                } else {
                    None
                }
            })
            .collect();
        
        // Sort by file number (001, 002, etc.)
        migrations.sort_by(|a, b| {
            let num_a = a.file_stem()
                .and_then(|s| s.to_str())
                .and_then(|s| s.strip_suffix(".sql"))
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(0);
            let num_b = b.file_stem()
                .and_then(|s| s.to_str())
                .and_then(|s| s.strip_suffix(".sql"))
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(0);
            num_a.cmp(&num_b)
        });
        
        // Run each migration
        for migration_path in migrations {
            let sql = std::fs::read_to_string(&migration_path)?;
            conn.execute_batch(&sql)?;
            println!("Applied migration: {}", migration_path.display());
        }
        
        Ok(())
    }
}
