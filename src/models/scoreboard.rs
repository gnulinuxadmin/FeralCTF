use crate::{db::DbConn, errors::AppError};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct TeamScore {
    pub rank: i64,
    pub team_id: i64,
    pub team_name: String,
    pub score: i64,
    pub solve_count: i64,
    pub last_solve_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScoreboardState {
    pub teams: Vec<TeamScore>,
    pub generated_at: i64,
}

impl ScoreboardState {
    pub fn build(conn: &DbConn) -> Result<Self, AppError> {
        let generated_at = chrono::Utc::now().timestamp();
        let mut stmt = conn.prepare(
            "SELECT t.id, t.name, t.score, COUNT(s.id) AS solve_count, t.last_solve_at
             FROM teams t
             LEFT JOIN solves s ON t.id = s.team_id
             GROUP BY t.id
             ORDER BY t.score DESC, t.last_solve_at ASC NULLS LAST",
        )?;
        let rows: Vec<(i64, String, i64, i64, Option<i64>)> = stmt
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            })?
            .collect::<Result<_, _>>()?;

        let mut teams = Vec::with_capacity(rows.len());
        let mut rank: i64 = 1;
        for (i, (team_id, team_name, score, solve_count, last_solve_at)) in rows.iter().enumerate()
        {
            if i > 0 {
                let prev = &rows[i - 1];
                if *score != prev.2 || *last_solve_at != prev.4 {
                    rank = (i + 1) as i64;
                }
            }
            teams.push(TeamScore {
                rank,
                team_id: *team_id,
                team_name: team_name.clone(),
                score: *score,
                solve_count: *solve_count,
                last_solve_at: *last_solve_at,
            });
        }

        Ok(ScoreboardState {
            teams,
            generated_at,
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
    fn scoreboard_build_orders_and_ranks_teams() {
        let pool = test_pool();
        let conn = pool.get().unwrap();
        conn.execute(
            "INSERT INTO teams (id, name, invite_code, score, last_solve_at)
             VALUES
                (1, 'Alpha', 'ALPHA001', 200, 20),
                (2, 'Bravo', 'BRAVO001', 300, 30),
                (3, 'Charlie', 'CHARLIE1', 300, 30),
                (4, 'Delta', 'DELTA001', 300, 10)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO challenges (
                id, slug, title, description, category, flag_hash, flag_salt, flag_type,
                flag_case_sensitive, points, max_points, min_points, decay_rate, created_at
             )
             VALUES (1, 'challenge', 'Challenge', 'desc', 'web', 'hash', 'salt', 'static',
                0, 100, 500, 50, 12, 1000)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO users (id, username, password_hash, role, created_at)
             VALUES (1, 'u1', 'h', 'player', 1), (2, 'u2', 'h', 'player', 1)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO solves (team_id, user_id, challenge_id, solved_at)
             VALUES (2, 1, 1, 100), (3, 2, 1, 100)",
            [],
        )
        .unwrap();

        let scoreboard = ScoreboardState::build(&conn).unwrap();

        assert_eq!(scoreboard.teams.len(), 4);
        assert_eq!(scoreboard.teams[0].team_name, "Delta");
        assert_eq!(scoreboard.teams[0].rank, 1);
        assert_eq!(scoreboard.teams[1].team_name, "Bravo");
        assert_eq!(scoreboard.teams[1].rank, 2);
        assert_eq!(scoreboard.teams[2].team_name, "Charlie");
        assert_eq!(scoreboard.teams[2].rank, 2);
        assert_eq!(scoreboard.teams[3].team_name, "Alpha");
        assert_eq!(scoreboard.teams[3].rank, 4);
        assert_eq!(scoreboard.teams[1].solve_count, 1);
    }
}
