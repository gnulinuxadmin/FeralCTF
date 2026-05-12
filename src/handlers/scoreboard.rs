use axum::{
    Json,
    extract::{Path, State},
    http::HeaderMap,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::{
    AppState, auth,
    errors::{AppError, HandlerResult},
    models::{scoreboard::ScoreboardState, team::Team, user::User},
};

#[derive(Debug, Serialize)]
pub struct TeamGraphData {
    pub team_id: i64,
    pub team_name: String,
    pub points: Vec<(i64, i64)>,
}

#[derive(Debug, Serialize)]
pub struct TeamSolve {
    pub challenge_id: i64,
    pub challenge_title: String,
    pub category: String,
    pub points: i64,
    pub solved_at: i64,
}

#[derive(Debug, Serialize)]
pub struct TeamProfile {
    pub team: Team,
    pub solve_history: Vec<TeamSolve>,
}

#[derive(Debug, Deserialize)]
pub struct CreateTeamRequest {
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct JoinTeamRequest {
    pub invite_code: String,
}

pub async fn get_scoreboard(State(state): State<AppState>) -> HandlerResult<Json<ScoreboardState>> {
    let conn = state
        .db
        .get()
        .map_err(|err| anyhow::anyhow!("db pool: {err}"))?;
    let scoreboard = state.cache.get_or_build_scoreboard(&conn)?;
    Ok(Json(scoreboard))
}

pub async fn get_scoreboard_graph(
    State(state): State<AppState>,
) -> HandlerResult<Json<Vec<TeamGraphData>>> {
    let conn = state
        .db
        .get()
        .map_err(|err| anyhow::anyhow!("db pool: {err}"))?;
    let mut stmt = conn.prepare(
        "SELECT t.id, t.name, sh.recorded_at, sh.score
         FROM teams t
         JOIN score_history sh ON sh.team_id = t.id
         ORDER BY t.id, sh.recorded_at",
    )?;

    let mut by_team: BTreeMap<i64, TeamGraphData> = BTreeMap::new();
    for row in stmt.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, i64>(3)?,
        ))
    })? {
        let (team_id, team_name, timestamp, score) = row?;
        by_team
            .entry(team_id)
            .or_insert_with(|| TeamGraphData {
                team_id,
                team_name,
                points: Vec::new(),
            })
            .points
            .push((timestamp, score));
    }

    Ok(Json(by_team.into_values().collect()))
}

pub async fn get_team_profile(
    State(state): State<AppState>,
    Path(team_id): Path<i64>,
) -> HandlerResult<Json<TeamProfile>> {
    let conn = state
        .db
        .get()
        .map_err(|err| anyhow::anyhow!("db pool: {err}"))?;
    let team = Team::find_by_id(&conn, team_id)?
        .ok_or_else(|| AppError::NotFound("team not found".to_string()))?;
    let solve_history = team_solve_history(&conn, team_id)?;
    Ok(Json(TeamProfile {
        team,
        solve_history,
    }))
}

pub async fn create_team(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CreateTeamRequest>,
) -> HandlerResult<Json<Team>> {
    let user = current_user(&state, &headers)?;
    if user.team_id.is_some() {
        return Err(AppError::BadRequest(
            "user is already on a team".to_string(),
        ));
    }
    if request.name.trim().is_empty() {
        return Err(AppError::BadRequest("team name is required".to_string()));
    }

    let conn = state
        .db
        .get()
        .map_err(|err| anyhow::anyhow!("db pool: {err}"))?;
    let team = Team::create(&conn, request.name.trim())?;
    Team::add_member(&conn, team.id, user.id)?;
    state.cache.invalidate_scoreboard();
    Ok(Json(team))
}

pub async fn join_team(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<JoinTeamRequest>,
) -> HandlerResult<Json<Team>> {
    let user = current_user(&state, &headers)?;
    if user.team_id.is_some() {
        return Err(AppError::BadRequest(
            "user is already on a team".to_string(),
        ));
    }

    let conn = state
        .db
        .get()
        .map_err(|err| anyhow::anyhow!("db pool: {err}"))?;
    let team = Team::find_by_invite_code(&conn, request.invite_code.trim())?
        .ok_or_else(|| AppError::BadRequest("invalid invite code".to_string()))?;
    Team::add_member(&conn, team.id, user.id)?;
    state.cache.invalidate_scoreboard();
    Ok(Json(team))
}

pub fn snapshot_scores(conn: &crate::db::DbConn, recorded_at: i64) -> Result<usize, AppError> {
    let inserted = conn.execute(
        "INSERT INTO score_history (team_id, score, recorded_at)
         SELECT id, score, ?1 FROM teams",
        rusqlite::params![recorded_at],
    )?;
    Ok(inserted)
}

pub fn spawn_score_snapshot_task(state: AppState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(300));
        loop {
            interval.tick().await;
            match state.db.get() {
                Ok(conn) => {
                    let now = chrono::Utc::now().timestamp();
                    if let Err(err) = snapshot_scores(&conn, now) {
                        tracing::warn!("score snapshot failed: {err}");
                    }
                }
                Err(err) => tracing::warn!("score snapshot could not get db connection: {err}"),
            }
        }
    })
}

fn team_solve_history(conn: &crate::db::DbConn, team_id: i64) -> Result<Vec<TeamSolve>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT c.id, c.title, c.category, c.points, s.solved_at
         FROM solves s
         JOIN challenges c ON c.id = s.challenge_id
         WHERE s.team_id = ?1
         ORDER BY s.solved_at DESC",
    )?;
    let solves = stmt
        .query_map(rusqlite::params![team_id], |row| {
            Ok(TeamSolve {
                challenge_id: row.get(0)?,
                challenge_title: row.get(1)?,
                category: row.get(2)?,
                points: row.get(3)?,
                solved_at: row.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(solves)
}

fn current_user(state: &AppState, headers: &HeaderMap) -> Result<User, AppError> {
    let token = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .ok_or(AppError::Unauthorized)?;
    let claims = auth::verify_jwt(token, &state.config.auth.jwt_secret)?;
    if !auth::is_session_valid(&state.db, &auth::hash_token(token))? {
        return Err(AppError::Unauthorized);
    }

    let conn = state
        .db
        .get()
        .map_err(|err| anyhow::anyhow!("db pool: {err}"))?;
    User::find_by_id(&conn, claims.sub)?.ok_or(AppError::Unauthorized)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        cache::AppCache,
        config::Config,
        db,
        models::user::{RegisterRequest, User},
    };
    use axum::extract::State;
    use r2d2::Pool;
    use r2d2_sqlite::SqliteConnectionManager;
    use std::sync::Arc;

    fn test_state() -> AppState {
        let pool = Pool::builder()
            .max_size(1)
            .build(SqliteConnectionManager::memory())
            .unwrap();
        {
            let conn = pool.get().unwrap();
            db::run_migrations(&conn).unwrap();
        }
        let mut config = Config::default();
        config.auth.jwt_secret = "test-secret".to_string();
        AppState {
            db: pool,
            config: Arc::new(config),
            cache: Arc::new(AppCache::new()),
            ws_hub: Arc::new(crate::WsHub::new()),
        }
    }

    fn authed_headers(state: &AppState, username: &str) -> (HeaderMap, User) {
        let conn = state.db.get().unwrap();
        let req = RegisterRequest {
            username: username.to_string(),
            email: None,
            password: "password123".to_string(),
            team_name: None,
            invite_code: None,
        };
        let user = User::create(&conn, &req, "hash").unwrap();
        drop(conn);

        let now = chrono::Utc::now().timestamp() as u64;
        let token = auth::sign_jwt(
            &auth::Claims {
                sub: user.id,
                role: user.role.clone(),
                team_id: user.team_id,
                iat: now,
                exp: now + 3600,
            },
            &state.config.auth.jwt_secret,
        )
        .unwrap();
        auth::create_session(&state.db, user.id, &token, 1).unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {token}").parse().unwrap(),
        );
        (headers, user)
    }

    #[tokio::test]
    async fn scoreboard_is_served_from_cache() {
        let state = test_state();
        {
            let conn = state.db.get().unwrap();
            conn.execute(
                "INSERT INTO teams (id, name, invite_code, score) VALUES (1, 'A', 'ABCDEFGH', 10)",
                [],
            )
            .unwrap();
        }

        let Json(first) = get_scoreboard(State(state.clone())).await.unwrap();
        assert_eq!(first.teams.len(), 1);
        assert!(state.cache.is_scoreboard_cached());

        {
            let conn = state.db.get().unwrap();
            conn.execute(
                "INSERT INTO teams (id, name, invite_code, score) VALUES (2, 'B', 'BCDEFGHI', 20)",
                [],
            )
            .unwrap();
        }

        let Json(cached) = get_scoreboard(State(state.clone())).await.unwrap();
        assert_eq!(cached.teams.len(), 1);
        state.cache.invalidate_scoreboard();
        let Json(rebuilt) = get_scoreboard(State(state)).await.unwrap();
        assert_eq!(rebuilt.teams.len(), 2);
    }

    #[tokio::test]
    async fn graph_and_snapshot_return_time_series() {
        let state = test_state();
        {
            let conn = state.db.get().unwrap();
            conn.execute(
                "INSERT INTO teams (id, name, invite_code, score) VALUES (1, 'A', 'ABCDEFGH', 10)",
                [],
            )
            .unwrap();
            snapshot_scores(&conn, 100).unwrap();
            conn.execute("UPDATE teams SET score = 20 WHERE id = 1", [])
                .unwrap();
            snapshot_scores(&conn, 200).unwrap();
        }

        let Json(graph) = get_scoreboard_graph(State(state)).await.unwrap();
        assert_eq!(graph.len(), 1);
        assert_eq!(graph[0].points, vec![(100, 10), (200, 20)]);
    }

    #[tokio::test]
    async fn create_and_join_team_update_users() {
        let state = test_state();
        let (headers_a, user_a) = authed_headers(&state, "alice");
        let Json(team) = create_team(
            State(state.clone()),
            headers_a,
            Json(CreateTeamRequest {
                name: "A Team".to_string(),
            }),
        )
        .await
        .unwrap();

        let (headers_b, user_b) = authed_headers(&state, "bob");
        let Json(joined) = join_team(
            State(state.clone()),
            headers_b,
            Json(JoinTeamRequest {
                invite_code: team.invite_code.clone(),
            }),
        )
        .await
        .unwrap();
        assert_eq!(joined.id, team.id);

        let conn = state.db.get().unwrap();
        assert_eq!(
            User::find_by_id(&conn, user_a.id).unwrap().unwrap().team_id,
            Some(team.id)
        );
        assert_eq!(
            User::find_by_id(&conn, user_b.id).unwrap().unwrap().team_id,
            Some(team.id)
        );
    }

    #[tokio::test]
    async fn team_profile_includes_solve_history() {
        let state = test_state();
        {
            let conn = state.db.get().unwrap();
            conn.execute(
                "INSERT INTO teams (id, name, invite_code, score) VALUES (1, 'A', 'ABCDEFGH', 100)",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO users (id, username, password_hash, role, team_id, created_at)
                 VALUES (1, 'u', 'h', 'player', 1, 1)",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO challenges (
                    id, slug, title, description, category, flag_hash, flag_salt, flag_type,
                    flag_case_sensitive, points, max_points, min_points, decay_rate, created_at
                 )
                 VALUES (1, 'c', 'Challenge', 'desc', 'web', 'hash', 'salt', 'static',
                    0, 100, 500, 50, 12, 1)",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO solves (team_id, user_id, challenge_id, solved_at)
                 VALUES (1, 1, 1, 50)",
                [],
            )
            .unwrap();
        }

        let Json(profile) = get_team_profile(State(state), Path(1)).await.unwrap();
        assert_eq!(profile.team.name, "A");
        assert_eq!(profile.solve_history.len(), 1);
        assert_eq!(profile.solve_history[0].challenge_title, "Challenge");
    }
}
