use std::path::PathBuf;

/// Compatibility wrapper around the canonical r2d2 SQLite pool.
#[derive(Clone)]
pub struct DatabaseState {
    pub db_path: PathBuf,
    pub pool: crate::db::DbPool,
}

impl DatabaseState {
    pub fn new(db_path: PathBuf) -> Result<Self, Box<dyn std::error::Error>> {
        let db_path_str = db_path
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("database path is not valid UTF-8"))?;
        let pool = crate::db::init_pool(db_path_str)?;
        Ok(Self { db_path, pool })
    }

    pub fn db_path(&self) -> &PathBuf {
        &self.db_path
    }

    pub fn get_conn(&self) -> Result<crate::db::DbConn, r2d2::Error> {
        self.pool.get()
    }
}

pub fn get_db_connection(db_state: &DatabaseState) -> Result<crate::db::DbConn, r2d2::Error> {
    db_state.get_conn()
}

pub type Database = crate::db::DbConn;
pub type DbPool = crate::db::DbPool;
