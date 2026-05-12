use crate::{db::DbConn, errors::AppError};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub email: Option<String>,
    pub password_hash: String,
    pub role: String,
    pub team_id: Option<i64>,
    pub created_at: i64,
}

#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    pub username: String,
    pub email: Option<String>,
    pub password: String,
    pub team_name: Option<String>,
    pub invite_code: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub token: String,
    pub expires_at: u64,
    pub user: UserPublic,
}

#[derive(Debug, Serialize)]
pub struct UserPublic {
    pub id: i64,
    pub username: String,
    pub role: String,
    pub team_id: Option<i64>,
}

impl User {
    pub fn find_by_id(conn: &DbConn, id: i64) -> Result<Option<Self>, AppError> {
        let result = conn.query_row(
            "SELECT id, username, email, password_hash, role, team_id, created_at
             FROM users WHERE id = ?1",
            rusqlite::params![id],
            Self::from_row,
        );
        match result {
            Ok(u) => Ok(Some(u)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(AppError::Database(e)),
        }
    }

    pub fn find_by_username(conn: &DbConn, username: &str) -> Result<Option<Self>, AppError> {
        let result = conn.query_row(
            "SELECT id, username, email, password_hash, role, team_id, created_at
             FROM users WHERE username = ?1",
            rusqlite::params![username],
            Self::from_row,
        );
        match result {
            Ok(u) => Ok(Some(u)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(AppError::Database(e)),
        }
    }

    pub fn create(
        conn: &DbConn,
        req: &RegisterRequest,
        password_hash: &str,
    ) -> Result<Self, AppError> {
        let now = chrono::Utc::now().timestamp();
        conn.execute(
            "INSERT INTO users (username, email, password_hash, role, team_id, created_at)
             VALUES (?1, ?2, ?3, 'player', NULL, ?4)",
            rusqlite::params![req.username, req.email, password_hash, now],
        )?;
        let id = conn.last_insert_rowid();
        Self::find_by_id(conn, id)?
            .ok_or_else(|| anyhow::anyhow!("user not found after insert").into())
    }

    pub fn count(conn: &DbConn) -> Result<i64, AppError> {
        let n = conn.query_row("SELECT COUNT(*) FROM users", [], |row| row.get(0))?;
        Ok(n)
    }

    fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(User {
            id: row.get(0)?,
            username: row.get(1)?,
            email: row.get(2)?,
            password_hash: row.get(3)?,
            role: row.get(4)?,
            team_id: row.get(5)?,
            created_at: row.get(6)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use r2d2::Pool;
    use r2d2_sqlite::SqliteConnectionManager;

    fn test_pool() -> crate::db::DbPool {
        let pool = Pool::new(SqliteConnectionManager::memory()).unwrap();
        {
            let conn = pool.get().unwrap();
            crate::db::run_migrations(&conn).unwrap();
        }
        pool
    }

    #[test]
    fn user_helpers_create_find_and_count() {
        let pool = test_pool();
        let conn = pool.get().unwrap();
        let req = RegisterRequest {
            username: "alice".to_string(),
            email: Some("alice@example.test".to_string()),
            password: "not-used-here".to_string(),
            team_name: None,
            invite_code: None,
        };

        let user = User::create(&conn, &req, "hashed-password").unwrap();

        assert_eq!(user.username, "alice");
        assert_eq!(user.email.as_deref(), Some("alice@example.test"));
        assert_eq!(user.password_hash, "hashed-password");
        assert_eq!(user.role, "player");
        assert_eq!(User::count(&conn).unwrap(), 1);
        assert_eq!(
            User::find_by_id(&conn, user.id).unwrap().unwrap().username,
            "alice"
        );
        assert_eq!(
            User::find_by_username(&conn, "alice").unwrap().unwrap().id,
            user.id
        );
        assert!(User::find_by_username(&conn, "missing").unwrap().is_none());
    }
}
