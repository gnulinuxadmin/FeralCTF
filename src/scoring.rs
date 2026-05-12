use crate::{db::DbConn, errors::AppError};

pub fn dynamic_points(max_points: i64, min_points: i64, decay_rate: i64, solves: i64) -> i64 {
    let solve_count = solves.max(1);
    let decayed = max_points - decay_rate * (solve_count - 1).pow(2);
    decayed.max(min_points)
}

pub fn recalculate_challenge_points(conn: &DbConn, challenge_id: i64) -> Result<(), AppError> {
    let (flag_type, max_points, min_points, decay_rate): (String, i64, i64, i64) = conn.query_row(
        "SELECT flag_type, max_points, min_points, decay_rate
         FROM challenges WHERE id = ?1",
        rusqlite::params![challenge_id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    )?;

    if flag_type == "dynamic" {
        let solves: i64 = conn.query_row(
            "SELECT COUNT(*) FROM solves WHERE challenge_id = ?1",
            rusqlite::params![challenge_id],
            |row| row.get(0),
        )?;
        let points = dynamic_points(max_points, min_points, decay_rate, solves);
        conn.execute(
            "UPDATE challenges SET points = ?1 WHERE id = ?2",
            rusqlite::params![points, challenge_id],
        )?;
    }

    recalculate_all_team_scores(conn)?;
    Ok(())
}

pub fn recalculate_all_team_scores(conn: &DbConn) -> Result<(), AppError> {
    conn.execute(
        "UPDATE teams
         SET score =
            COALESCE((
                SELECT SUM(c.points)
                FROM solves s
                JOIN challenges c ON c.id = s.challenge_id
                WHERE s.team_id = teams.id
            ), 0)
            - COALESCE((
                SELECT SUM(points_deducted)
                FROM hint_unlocks hu
                WHERE hu.team_id = teams.id
            ), 0)",
        [],
    )?;
    Ok(())
}

pub struct Scoring;

impl Scoring {
    pub fn new() -> Self {
        Self
    }

    pub fn calculate_score(
        &self,
        challenge_points: i64,
        _time_taken: u64,
        _is_first_solve: bool,
    ) -> i64 {
        challenge_points
    }

    pub fn calculate_dynamic_score(
        &self,
        max_points: i64,
        solves_count: u64,
        _max_solves: u64,
    ) -> i64 {
        dynamic_points(max_points, 50, 12, solves_count as i64)
    }
}

impl Default for Scoring {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use r2d2::Pool;
    use r2d2_sqlite::SqliteConnectionManager;

    fn test_pool() -> crate::db::DbPool {
        let pool = Pool::new(SqliteConnectionManager::memory()).unwrap();
        let conn = pool.get().unwrap();
        crate::db::run_migrations(&conn).unwrap();
        drop(conn);
        pool
    }

    #[test]
    fn dynamic_points_decay_and_clamp() {
        assert_eq!(dynamic_points(500, 50, 12, 1), 500);
        assert_eq!(dynamic_points(500, 50, 12, 2), 488);
        assert_eq!(dynamic_points(500, 50, 12, 3), 452);
        assert_eq!(dynamic_points(500, 50, 12, 99), 50);
    }

    #[test]
    fn recalculate_dynamic_challenge_and_team_scores() {
        let pool = test_pool();
        let conn = pool.get().unwrap();
        conn.execute(
            "INSERT INTO teams (id, name, invite_code, score) VALUES
                (1, 'A', 'AAAA1111', 0),
                (2, 'B', 'BBBB1111', 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO users (id, username, password_hash, role, created_at) VALUES
                (1, 'a', 'h', 'player', 1),
                (2, 'b', 'h', 'player', 1)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO challenges (
                id, slug, title, description, category, flag_hash, flag_salt, flag_type,
                flag_case_sensitive, points, max_points, min_points, decay_rate, created_at
            ) VALUES (1, 'dyn', 'Dyn', 'desc', 'web', 'hash', 'salt', 'dynamic',
                0, 500, 500, 50, 12, 1)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO solves (team_id, user_id, challenge_id, solved_at) VALUES
                (1, 1, 1, 10), (2, 2, 1, 11)",
            [],
        )
        .unwrap();

        recalculate_challenge_points(&conn, 1).unwrap();

        let points: i64 = conn
            .query_row("SELECT points FROM challenges WHERE id = 1", [], |row| {
                row.get(0)
            })
            .unwrap();
        let team_score: i64 = conn
            .query_row("SELECT score FROM teams WHERE id = 1", [], |row| row.get(0))
            .unwrap();
        assert_eq!(points, 488);
        assert_eq!(team_score, 488);
    }
}
