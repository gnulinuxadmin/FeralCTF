// FeralCTF - Migration runner
// ConnectionPool is replaced by r2d2::Pool in db/mod.rs (Sprint 1).

use rusqlite::Connection;
use std::path::Path;

/// Reads all .sql files from a directory (sorted by filename) and executes them.
pub struct MigrationRunner {
    migrations_dir: std::path::PathBuf,
}

impl MigrationRunner {
    pub fn new(migrations_dir: &str) -> Self {
        MigrationRunner {
            migrations_dir: std::path::PathBuf::from(migrations_dir),
        }
    }

    pub fn run(&self, conn: &Connection) -> Result<(), anyhow::Error> {
        let mut migrations: Vec<_> = std::fs::read_dir(&self.migrations_dir)?
            .filter_map(|e| {
                let entry = e.ok()?;
                let path = entry.path();
                if path.extension().is_some_and(|ext| ext == "sql") {
                    Some(path)
                } else {
                    None
                }
            })
            .collect();

        migrations.sort();

        for path in migrations {
            let sql = std::fs::read_to_string(&path)?;
            conn.execute_batch(&sql)?;
        }

        Ok(())
    }
}
