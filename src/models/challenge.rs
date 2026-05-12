use crate::{db::DbConn, errors::AppError};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Challenge {
    pub id: i64,
    pub slug: String,
    pub title: String,
    pub description: String,
    pub category: String,
    pub flag_hash: String,
    pub flag_salt: String,
    pub flag_type: String,
    pub flag_case_sensitive: bool,
    pub points: i64,
    pub max_points: i64,
    pub min_points: i64,
    pub decay_rate: i64,
    pub author: Option<String>,
    pub tags: Option<String>,
    pub unlock_requires: Option<i64>,
    pub is_hidden: bool,
    pub created_at: i64,
}

#[derive(Debug, Serialize)]
pub struct ChallengePublic {
    pub id: i64,
    pub slug: String,
    pub title: String,
    pub description: String,
    pub category: String,
    pub points: i64,
    pub solve_count: i64,
    pub solved_by_team: bool,
    pub tags: Vec<String>,
    pub file_count: i64,
    pub hint_count: i64,
    pub unlock_requires: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hint {
    pub id: i64,
    pub challenge_id: i64,
    pub content: String,
    pub cost_points: i64,
    pub sort_order: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChallengeFile {
    pub id: i64,
    pub challenge_id: i64,
    pub filename: String,
    pub storage_path: String,
    pub size_bytes: i64,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Submission {
    pub id: i64,
    pub team_id: i64,
    pub user_id: i64,
    pub challenge_id: i64,
    pub flag: String,
    pub is_correct: bool,
    pub ip_address: Option<String>,
    pub submitted_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct HintPublic {
    pub id: i64,
    pub cost_points: i64,
    pub sort_order: i64,
    pub unlocked: bool,
    pub content: Option<String>,
}

const SELECT_BY_ID: &str =
    "SELECT id, slug, title, description, category, flag_hash, flag_salt, flag_type,
     flag_case_sensitive, points, max_points, min_points, decay_rate, author,
     tags, unlock_requires, is_hidden, created_at
     FROM challenges WHERE id = ?1";

const SELECT_BY_SLUG: &str =
    "SELECT id, slug, title, description, category, flag_hash, flag_salt, flag_type,
     flag_case_sensitive, points, max_points, min_points, decay_rate, author,
     tags, unlock_requires, is_hidden, created_at
     FROM challenges WHERE slug = ?1";

const SELECT_VISIBLE: &str =
    "SELECT id, slug, title, description, category, flag_hash, flag_salt, flag_type,
     flag_case_sensitive, points, max_points, min_points, decay_rate, author,
     tags, unlock_requires, is_hidden, created_at
     FROM challenges WHERE is_hidden = 0 ORDER BY category, points";

const SELECT_ALL: &str =
    "SELECT id, slug, title, description, category, flag_hash, flag_salt, flag_type,
     flag_case_sensitive, points, max_points, min_points, decay_rate, author,
     tags, unlock_requires, is_hidden, created_at
     FROM challenges ORDER BY category, points";

impl Challenge {
    pub fn find_by_id(conn: &DbConn, id: i64) -> Result<Option<Self>, AppError> {
        let result = conn.query_row(SELECT_BY_ID, rusqlite::params![id], Self::from_row);
        match result {
            Ok(c) => Ok(Some(c)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(AppError::Database(e)),
        }
    }

    pub fn find_by_slug(conn: &DbConn, slug: &str) -> Result<Option<Self>, AppError> {
        let result = conn.query_row(SELECT_BY_SLUG, rusqlite::params![slug], Self::from_row);
        match result {
            Ok(c) => Ok(Some(c)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(AppError::Database(e)),
        }
    }

    pub fn list_visible(conn: &DbConn) -> Result<Vec<Self>, AppError> {
        let mut stmt = conn.prepare(SELECT_VISIBLE)?;
        let items = stmt
            .query_map([], Self::from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(items)
    }

    pub fn list_all(conn: &DbConn) -> Result<Vec<Self>, AppError> {
        let mut stmt = conn.prepare(SELECT_ALL)?;
        let items = stmt
            .query_map([], Self::from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(items)
    }

    pub fn solve_count(conn: &DbConn, challenge_id: i64) -> Result<i64, AppError> {
        let n = conn.query_row(
            "SELECT COUNT(*) FROM solves WHERE challenge_id = ?1",
            rusqlite::params![challenge_id],
            |row| row.get(0),
        )?;
        Ok(n)
    }

    pub fn is_solved_by_team(
        conn: &DbConn,
        challenge_id: i64,
        team_id: i64,
    ) -> Result<bool, AppError> {
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM solves WHERE challenge_id = ?1 AND team_id = ?2",
            rusqlite::params![challenge_id, team_id],
            |row| row.get(0),
        )?;
        Ok(n > 0)
    }

    pub fn file_count(conn: &DbConn, challenge_id: i64) -> Result<i64, AppError> {
        let n = conn.query_row(
            "SELECT COUNT(*) FROM files WHERE challenge_id = ?1",
            rusqlite::params![challenge_id],
            |row| row.get(0),
        )?;
        Ok(n)
    }

    pub fn hint_count(conn: &DbConn, challenge_id: i64) -> Result<i64, AppError> {
        let n = conn.query_row(
            "SELECT COUNT(*) FROM hints WHERE challenge_id = ?1",
            rusqlite::params![challenge_id],
            |row| row.get(0),
        )?;
        Ok(n)
    }

    pub fn to_public(&self, conn: &DbConn, team_id: i64) -> Result<ChallengePublic, AppError> {
        Ok(ChallengePublic {
            id: self.id,
            slug: self.slug.clone(),
            title: self.title.clone(),
            description: self.description.clone(),
            category: self.category.clone(),
            points: self.points,
            solve_count: Self::solve_count(conn, self.id)?,
            solved_by_team: Self::is_solved_by_team(conn, self.id, team_id)?,
            tags: parse_tags(self.tags.as_deref()),
            file_count: Self::file_count(conn, self.id)?,
            hint_count: Self::hint_count(conn, self.id)?,
            unlock_requires: self.unlock_requires,
        })
    }

    fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Challenge {
            id: row.get(0)?,
            slug: row.get(1)?,
            title: row.get(2)?,
            description: row.get(3)?,
            category: row.get(4)?,
            flag_hash: row.get(5)?,
            flag_salt: row.get(6)?,
            flag_type: row.get(7)?,
            flag_case_sensitive: row.get::<_, i64>(8)? != 0,
            points: row.get(9)?,
            max_points: row.get(10)?,
            min_points: row.get(11)?,
            decay_rate: row.get(12)?,
            author: row.get(13)?,
            tags: row.get(14)?,
            unlock_requires: row.get(15)?,
            is_hidden: row.get::<_, i64>(16)? != 0,
            created_at: row.get(17)?,
        })
    }
}

impl Hint {
    pub fn find_by_id(conn: &DbConn, id: i64) -> Result<Option<Self>, AppError> {
        let result = conn.query_row(
            "SELECT id, challenge_id, content, cost_points, sort_order FROM hints WHERE id = ?1",
            rusqlite::params![id],
            Self::from_row,
        );
        match result {
            Ok(hint) => Ok(Some(hint)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(err) => Err(AppError::Database(err)),
        }
    }

    pub fn list_public_for_team(
        conn: &DbConn,
        challenge_id: i64,
        team_id: i64,
    ) -> Result<Vec<HintPublic>, AppError> {
        let mut stmt = conn.prepare(
            "SELECT h.id, h.content, h.cost_points, h.sort_order,
                    hu.id IS NOT NULL AS unlocked
             FROM hints h
             LEFT JOIN hint_unlocks hu ON hu.hint_id = h.id AND hu.team_id = ?2
             WHERE h.challenge_id = ?1
             ORDER BY h.sort_order, h.id",
        )?;
        let hints = stmt
            .query_map(rusqlite::params![challenge_id, team_id], |row| {
                let unlocked = row.get::<_, i64>(4)? != 0;
                Ok(HintPublic {
                    id: row.get(0)?,
                    cost_points: row.get(2)?,
                    sort_order: row.get(3)?,
                    unlocked,
                    content: if unlocked { Some(row.get(1)?) } else { None },
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(hints)
    }

    pub fn is_unlocked(conn: &DbConn, team_id: i64, hint_id: i64) -> Result<bool, AppError> {
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM hint_unlocks WHERE team_id = ?1 AND hint_id = ?2",
            rusqlite::params![team_id, hint_id],
            |row| row.get(0),
        )?;
        Ok(n > 0)
    }

    pub fn unlock(
        conn: &DbConn,
        team_id: i64,
        hint_id: i64,
        points_deducted: i64,
        unlocked_at: i64,
    ) -> Result<(), AppError> {
        conn.execute(
            "INSERT INTO hint_unlocks (team_id, hint_id, points_deducted, unlocked_at)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![team_id, hint_id, points_deducted, unlocked_at],
        )?;
        Ok(())
    }

    fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Hint {
            id: row.get(0)?,
            challenge_id: row.get(1)?,
            content: row.get(2)?,
            cost_points: row.get(3)?,
            sort_order: row.get(4)?,
        })
    }
}

impl ChallengeFile {
    pub fn list_by_challenge(conn: &DbConn, challenge_id: i64) -> Result<Vec<Self>, AppError> {
        let mut stmt = conn.prepare(
            "SELECT id, challenge_id, filename, storage_path, size_bytes, sha256
             FROM files WHERE challenge_id = ?1 ORDER BY filename",
        )?;
        let files = stmt
            .query_map(rusqlite::params![challenge_id], |row| {
                Ok(ChallengeFile {
                    id: row.get(0)?,
                    challenge_id: row.get(1)?,
                    filename: row.get(2)?,
                    storage_path: row.get(3)?,
                    size_bytes: row.get(4)?,
                    sha256: row.get(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(files)
    }
}

impl Submission {
    pub fn create(
        conn: &DbConn,
        team_id: i64,
        user_id: i64,
        challenge_id: i64,
        flag: &str,
        is_correct: bool,
        ip_address: Option<&str>,
    ) -> Result<Self, AppError> {
        let submitted_at = chrono::Utc::now().timestamp();
        conn.execute(
            "INSERT INTO submissions
                (team_id, user_id, challenge_id, flag, is_correct, ip_address, submitted_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                team_id,
                user_id,
                challenge_id,
                flag,
                if is_correct { 1 } else { 0 },
                ip_address,
                submitted_at
            ],
        )?;
        Ok(Submission {
            id: conn.last_insert_rowid(),
            team_id,
            user_id,
            challenge_id,
            flag: flag.to_string(),
            is_correct,
            ip_address: ip_address.map(ToString::to_string),
            submitted_at,
        })
    }
}

pub fn insert_solve(
    conn: &DbConn,
    team_id: i64,
    user_id: i64,
    challenge_id: i64,
    solved_at: i64,
) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO solves (team_id, user_id, challenge_id, solved_at)
         VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![team_id, user_id, challenge_id, solved_at],
    )?;
    Ok(())
}

pub fn insert_score_history(
    conn: &DbConn,
    team_id: i64,
    score: i64,
    recorded_at: i64,
) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO score_history (team_id, score, recorded_at)
         VALUES (?1, ?2, ?3)",
        rusqlite::params![team_id, score, recorded_at],
    )?;
    Ok(())
}

fn parse_tags(tags: Option<&str>) -> Vec<String> {
    tags.and_then(|raw| serde_json::from_str::<Vec<String>>(raw).ok())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        models::team::Team,
        models::user::{RegisterRequest, User},
    };
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

    fn insert_challenge(conn: &DbConn, slug: &str, title: &str, is_hidden: i64) -> i64 {
        conn.execute(
            "INSERT INTO challenges (
                slug, title, description, category, flag_hash, flag_salt, flag_type,
                flag_case_sensitive, points, max_points, min_points, decay_rate, author,
                tags, unlock_requires, is_hidden, created_at
             )
             VALUES (?1, ?2, 'desc', 'web', 'hash', 'salt', 'static', 0, 100, 500, 50, 12,
                'author', '[\"tag\"]', NULL, ?3, 1234)",
            rusqlite::params![slug, title, is_hidden],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    #[test]
    fn challenge_helpers_find_list_and_solve_status() {
        let pool = test_pool();
        let conn = pool.get().unwrap();
        let visible_id = insert_challenge(&conn, "visible-one", "Visible One", 0);
        let hidden_id = insert_challenge(&conn, "hidden-one", "Hidden One", 1);

        let req = RegisterRequest {
            username: "solver".to_string(),
            email: None,
            password: "password".to_string(),
            team_name: None,
            invite_code: None,
        };
        let user = User::create(&conn, &req, "hash").unwrap();
        let team = Team::create(&conn, "Team One").unwrap();
        Team::add_member(&conn, team.id, user.id).unwrap();
        conn.execute(
            "INSERT INTO solves (team_id, user_id, challenge_id, solved_at) VALUES (?1, ?2, ?3, 2000)",
            rusqlite::params![team.id, user.id, visible_id],
        )
        .unwrap();

        assert_eq!(
            Challenge::find_by_id(&conn, visible_id)
                .unwrap()
                .unwrap()
                .slug,
            "visible-one"
        );
        assert_eq!(
            Challenge::find_by_slug(&conn, "hidden-one")
                .unwrap()
                .unwrap()
                .id,
            hidden_id
        );
        assert_eq!(Challenge::list_visible(&conn).unwrap().len(), 1);
        assert_eq!(Challenge::list_all(&conn).unwrap().len(), 2);
        assert_eq!(Challenge::solve_count(&conn, visible_id).unwrap(), 1);
        assert!(Challenge::is_solved_by_team(&conn, visible_id, team.id).unwrap());
        assert!(!Challenge::is_solved_by_team(&conn, hidden_id, team.id).unwrap());
    }
}
