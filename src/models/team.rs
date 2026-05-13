use crate::{db::DbConn, errors::AppError};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Team {
    pub id: i64,
    pub name: String,
    pub invite_code: String,
    pub score: i64,
    pub last_solve_at: Option<i64>,
}

impl Team {
    pub fn find_by_id(conn: &DbConn, id: i64) -> Result<Option<Self>, AppError> {
        let result = conn.query_row(
            "SELECT id, name, invite_code, score, last_solve_at FROM teams WHERE id = ?1",
            rusqlite::params![id],
            Self::from_row,
        );
        match result {
            Ok(t) => Ok(Some(t)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(AppError::Database(e)),
        }
    }

    pub fn find_by_invite_code(conn: &DbConn, code: &str) -> Result<Option<Self>, AppError> {
        let result = conn.query_row(
            "SELECT id, name, invite_code, score, last_solve_at FROM teams WHERE invite_code = ?1",
            rusqlite::params![code],
            Self::from_row,
        );
        match result {
            Ok(t) => Ok(Some(t)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(AppError::Database(e)),
        }
    }

    pub fn create(conn: &DbConn, name: &str) -> Result<Self, AppError> {
        let invite_code = generate_invite_code();
        conn.execute(
            "INSERT INTO teams (name, invite_code, score) VALUES (?1, ?2, 0)",
            rusqlite::params![name, invite_code],
        )?;
        let id = conn.last_insert_rowid();
        Self::find_by_id(conn, id)?
            .ok_or_else(|| anyhow::anyhow!("team not found after insert").into())
    }

    pub fn add_member(conn: &DbConn, team_id: i64, user_id: i64) -> Result<(), AppError> {
        conn.execute(
            "UPDATE users SET team_id = ?1 WHERE id = ?2",
            rusqlite::params![team_id, user_id],
        )?;
        Ok(())
    }

    pub fn update_score(
        conn: &DbConn,
        team_id: i64,
        delta: i64,
        solved_at: i64,
    ) -> Result<(), AppError> {
        conn.execute(
            "UPDATE teams SET score = score + ?1, last_solve_at = ?2 WHERE id = ?3",
            rusqlite::params![delta, solved_at, team_id],
        )?;
        Ok(())
    }

    fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Team {
            id: row.get(0)?,
            name: row.get(1)?,
            invite_code: row.get(2)?,
            score: row.get(3)?,
            last_solve_at: row.get(4)?,
        })
    }
}

fn generate_invite_code() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::user::{RegisterRequest, User};
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
    fn team_helpers_create_find_join_and_score() {
        let pool = test_pool();
        let conn = pool.get().unwrap();
        let team = Team::create(&conn, "Blue Team").unwrap();

        assert_eq!(team.name, "Blue Team");
        assert_eq!(team.invite_code.len(), 36);
        assert_eq!(
            Team::find_by_invite_code(&conn, &team.invite_code)
                .unwrap()
                .unwrap()
                .id,
            team.id
        );

        let req = RegisterRequest {
            username: "member".to_string(),
            email: None,
            password: "not-used-here".to_string(),
            team_name: None,
            invite_code: None,
        };
        let user = User::create(&conn, &req, "hash").unwrap();
        Team::add_member(&conn, team.id, user.id).unwrap();
        Team::update_score(&conn, team.id, 150, 3000).unwrap();

        let updated_team = Team::find_by_id(&conn, team.id).unwrap().unwrap();
        let updated_user = User::find_by_id(&conn, user.id).unwrap().unwrap();
        assert_eq!(updated_team.score, 150);
        assert_eq!(updated_team.last_solve_at, Some(3000));
        assert_eq!(updated_user.team_id, Some(team.id));
    }
}
