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
pub struct TeamHintUnlock {
    pub challenge_id: i64,
    pub challenge_title: String,
    pub hint_id: i64,
    pub points_deducted: i64,
    pub unlocked_at: i64,
}

/// Team as shown on a profile. `invite_code` is only present for members of
/// the team and for admins.
#[derive(Debug, Serialize)]
pub struct TeamProfileInfo {
    pub id: i64,
    pub name: String,
    pub score: i64,
    pub last_solve_at: Option<i64>,
    pub is_disqualified: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invite_code: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct TeamProfile {
    pub team: TeamProfileInfo,
    pub solve_history: Vec<TeamSolve>,
    pub hint_history: Vec<TeamHintUnlock>,
    pub hints_used: i64,
    pub first_bloods: i64,
}

#[derive(Debug, Deserialize)]
pub struct CreateTeamRequest {
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct JoinTeamRequest {
    pub invite_code: String,
}

#[derive(Debug, Serialize)]
pub struct Announcement {
    pub id: i64,
    pub title: String,
    pub body: String,
    pub challenge_id: Option<i64>,
    pub created_at: i64,
}

/// The moment public scores are frozen at, if a freeze is in effect.
fn active_freeze(state: &AppState, conn: &crate::db::DbConn) -> Result<Option<i64>, AppError> {
    let status = crate::competition::status(conn, &state.config.competition)?;
    let now = chrono::Utc::now().timestamp();
    Ok(status.frozen_at.filter(|_| status.is_frozen(now)))
}

fn is_admin(user: Option<&User>) -> bool {
    user.is_some_and(|user| user.role == "admin")
}

/// Scoreboard everyone but admins sees: frozen while a freeze is active.
pub(crate) fn public_scoreboard(
    state: &AppState,
    conn: &crate::db::DbConn,
) -> Result<ScoreboardState, AppError> {
    match active_freeze(state, conn)? {
        Some(at) => ScoreboardState::build_as_of(conn, at),
        None => state.cache.get_or_build_scoreboard(conn),
    }
}

pub async fn get_scoreboard(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> HandlerResult<Json<ScoreboardState>> {
    let viewer = current_user(&state, &headers).ok();
    let conn = state
        .db
        .get()
        .map_err(|err| anyhow::anyhow!("db pool: {err}"))?;
    let scoreboard = if is_admin(viewer.as_ref()) {
        state.cache.get_or_build_scoreboard(&conn)?
    } else {
        public_scoreboard(&state, &conn)?
    };
    Ok(Json(scoreboard))
}

pub async fn get_competition(
    State(state): State<AppState>,
) -> HandlerResult<Json<crate::competition::CompetitionStatus>> {
    let conn = state
        .db
        .get()
        .map_err(|err| anyhow::anyhow!("db pool: {err}"))?;
    Ok(Json(crate::competition::status(
        &conn,
        &state.config.competition,
    )?))
}

pub async fn list_announcements(
    State(state): State<AppState>,
) -> HandlerResult<Json<Vec<Announcement>>> {
    let conn = state
        .db
        .get()
        .map_err(|err| anyhow::anyhow!("db pool: {err}"))?;
    let mut stmt = conn.prepare(
        "SELECT id, title, body, challenge_id, created_at
         FROM announcements WHERE is_visible = 1
         ORDER BY created_at DESC, id DESC
         LIMIT 50",
    )?;
    let announcements = stmt
        .query_map([], |row| {
            Ok(Announcement {
                id: row.get(0)?,
                title: row.get(1)?,
                body: row.get(2)?,
                challenge_id: row.get(3)?,
                created_at: row.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(announcements))
}

pub async fn get_scoreboard_graph(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> HandlerResult<Json<Vec<TeamGraphData>>> {
    let viewer = current_user(&state, &headers).ok();
    let conn = state
        .db
        .get()
        .map_err(|err| anyhow::anyhow!("db pool: {err}"))?;
    let cutoff = if is_admin(viewer.as_ref()) {
        None
    } else {
        active_freeze(&state, &conn)?
    };
    let mut stmt = conn.prepare(
        "SELECT t.id, t.name, sh.recorded_at, sh.score
         FROM teams t
         JOIN score_history sh ON sh.team_id = t.id
         WHERE ?1 IS NULL OR sh.recorded_at <= ?1
         ORDER BY t.id, sh.recorded_at",
    )?;

    let mut by_team: BTreeMap<i64, TeamGraphData> = BTreeMap::new();
    for row in stmt.query_map(rusqlite::params![cutoff], |row| {
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
    headers: HeaderMap,
    Path(team_id): Path<i64>,
) -> HandlerResult<Json<TeamProfile>> {
    // Anonymous callers are allowed; only members and admins see the invite code.
    let viewer = current_user(&state, &headers).ok();
    let can_see_invite = viewer
        .as_ref()
        .is_some_and(|user| user.role == "admin" || user.team_id == Some(team_id));
    let conn = state
        .db
        .get()
        .map_err(|err| anyhow::anyhow!("db pool: {err}"))?;
    let mut team = Team::find_by_id(&conn, team_id)?
        .ok_or_else(|| AppError::NotFound("team not found".to_string()))?;
    let mut solve_history = team_solve_history(&conn, team_id)?;
    let mut hint_history = team_hint_history(&conn, team_id)?;
    // During a freeze other teams only see this team as it stood at the freeze.
    if !can_see_invite && let Some(at) = active_freeze(&state, &conn)? {
        solve_history.retain(|solve| solve.solved_at <= at);
        hint_history.retain(|unlock| unlock.unlocked_at <= at);
        if let Some(frozen) = ScoreboardState::build_as_of(&conn, at)?
            .teams
            .into_iter()
            .find(|entry| entry.team_id == team_id)
        {
            team.score = frozen.score;
            team.last_solve_at = frozen.last_solve_at;
        }
    }
    let first_bloods = conn.query_row(
        "SELECT COUNT(*) FROM solves s
         WHERE s.team_id = ?1
           AND NOT EXISTS (
               SELECT 1 FROM solves o
               WHERE o.challenge_id = s.challenge_id
                 AND (o.solved_at < s.solved_at OR (o.solved_at = s.solved_at AND o.id < s.id))
           )",
        rusqlite::params![team_id],
        |row| row.get(0),
    )?;
    Ok(Json(TeamProfile {
        team: TeamProfileInfo {
            id: team.id,
            name: team.name,
            score: team.score,
            last_solve_at: team.last_solve_at,
            is_disqualified: team.is_disqualified,
            invite_code: can_see_invite.then_some(team.invite_code),
        },
        solve_history,
        hints_used: hint_history.len() as i64,
        hint_history,
        first_bloods,
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
    if Team::find_by_name(&conn, request.name.trim())?.is_some() {
        return Err(AppError::BadRequest("team name already taken".to_string()));
    }
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
    Team::ensure_has_room(&conn, team.id, state.config.competition.max_team_size)?;
    Team::add_member(&conn, team.id, user.id)?;
    state.cache.invalidate_scoreboard();
    Ok(Json(team))
}

/// Push the current scoreboard to every WebSocket client.
pub(crate) fn broadcast_score_update(state: &AppState, conn: &crate::db::DbConn) {
    state.cache.invalidate_scoreboard();
    if let Ok(sb) = public_scoreboard(state, conn) {
        state
            .ws_hub
            .broadcast(crate::handlers::ws::WsEvent::ScoreUpdate {
                scoreboard: sb.teams,
                total_visible_points: sb.total_visible_points,
            });
    }
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

fn team_hint_history(
    conn: &crate::db::DbConn,
    team_id: i64,
) -> Result<Vec<TeamHintUnlock>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT h.challenge_id, COALESCE(c.title, ''), hu.hint_id, hu.points_deducted, hu.unlocked_at
         FROM hint_unlocks hu
         JOIN hints h ON h.id = hu.hint_id
         LEFT JOIN challenges c ON c.id = h.challenge_id
         WHERE hu.team_id = ?1
         ORDER BY hu.unlocked_at DESC",
    )?;
    let unlocks = stmt
        .query_map(rusqlite::params![team_id], |row| {
            Ok(TeamHintUnlock {
                challenge_id: row.get(0)?,
                challenge_title: row.get(1)?,
                hint_id: row.get(2)?,
                points_deducted: row.get(3)?,
                unlocked_at: row.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(unlocks)
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
            rate_limiter: Arc::new(crate::anticheat::RateLimiter::new()),
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

        let Json(first) = get_scoreboard(State(state.clone()), HeaderMap::new())
            .await
            .unwrap();
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

        let Json(cached) = get_scoreboard(State(state.clone()), HeaderMap::new())
            .await
            .unwrap();
        assert_eq!(cached.teams.len(), 1);
        state.cache.invalidate_scoreboard();
        let Json(rebuilt) = get_scoreboard(State(state), HeaderMap::new())
            .await
            .unwrap();
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

        let Json(graph) = get_scoreboard_graph(State(state), HeaderMap::new())
            .await
            .unwrap();
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

        let Json(profile) = get_team_profile(State(state), HeaderMap::new(), Path(1))
            .await
            .unwrap();
        assert_eq!(profile.team.name, "A");
        assert_eq!(profile.team.invite_code, None);
        assert_eq!(profile.first_bloods, 1);
        assert_eq!(profile.hints_used, 0);
        assert_eq!(profile.solve_history.len(), 1);
        assert_eq!(profile.solve_history[0].challenge_title, "Challenge");
    }

    #[tokio::test]
    async fn team_profile_invite_code_only_visible_to_members() {
        let state = test_state();
        let (member_headers, _member) = authed_headers(&state, "member");
        let Json(team) = create_team(
            State(state.clone()),
            member_headers.clone(),
            Json(CreateTeamRequest {
                name: "Secret Team".to_string(),
            }),
        )
        .await
        .unwrap();
        let (outsider_headers, _outsider) = authed_headers(&state, "outsider");

        let Json(as_member) = get_team_profile(State(state.clone()), member_headers, Path(team.id))
            .await
            .unwrap();
        assert_eq!(
            as_member.team.invite_code.as_deref(),
            Some(team.invite_code.as_str())
        );

        let Json(as_outsider) =
            get_team_profile(State(state.clone()), outsider_headers, Path(team.id))
                .await
                .unwrap();
        assert_eq!(as_outsider.team.invite_code, None);

        let Json(anonymous) = get_team_profile(State(state), HeaderMap::new(), Path(team.id))
            .await
            .unwrap();
        assert_eq!(anonymous.team.invite_code, None);
        let json = serde_json::to_value(&anonymous).unwrap();
        assert!(json["team"].get("invite_code").is_none());
    }

    #[tokio::test]
    async fn freeze_hides_later_solves_from_everyone_but_admins() {
        let state = test_state();
        let (admin_headers, admin) = authed_headers(&state, "boss");
        let (player_headers, _player) = authed_headers(&state, "watcher");
        {
            let conn = state.db.get().unwrap();
            conn.execute(
                "UPDATE users SET role = 'admin' WHERE id = ?1",
                rusqlite::params![admin.id],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO teams (id, name, invite_code, score) VALUES (7, 'Racers', 'RACE0001', 0)",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO users (id, username, password_hash, role, team_id, created_at)
                 VALUES (70, 'racer', 'h', 'player', 7, 1)",
                [],
            )
            .unwrap();
            for (id, slug) in [(1, 'a'), (2, 'b')] {
                conn.execute(
                    "INSERT INTO challenges (
                        id, slug, title, description, category, flag_hash, flag_salt, flag_type,
                        flag_case_sensitive, points, max_points, min_points, decay_rate, is_hidden, created_at
                     ) VALUES (?1, ?2, ?2, 'd', 'web', 'h', 's', 'static', 0, 100, 100, 10, 1, 0, 1)",
                    rusqlite::params![id, slug.to_string()],
                )
                .unwrap();
            }
            conn.execute(
                "INSERT INTO solves (team_id, user_id, challenge_id, solved_at) VALUES (7, 70, 1, 10), (7, 70, 2, 30)",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO score_history (team_id, score, recorded_at) VALUES (7, 100, 10), (7, 200, 30)",
                [],
            )
            .unwrap();
            crate::scoring::recalculate_all_team_scores(&conn).unwrap();
            crate::competition::freeze(&conn, 20).unwrap();
        }

        let Json(public) = get_scoreboard(State(state.clone()), player_headers.clone())
            .await
            .unwrap();
        assert_eq!(
            (public.teams[0].score, public.teams[0].solve_count),
            (100, 1)
        );
        let Json(anonymous) = get_scoreboard(State(state.clone()), HeaderMap::new())
            .await
            .unwrap();
        assert_eq!(anonymous.teams[0].score, 100);
        let Json(live) = get_scoreboard(State(state.clone()), admin_headers.clone())
            .await
            .unwrap();
        assert_eq!(live.teams[0].score, 200);

        let Json(graph) = get_scoreboard_graph(State(state.clone()), player_headers.clone())
            .await
            .unwrap();
        assert_eq!(graph[0].points, vec![(10, 100)]);
        let Json(admin_graph) = get_scoreboard_graph(State(state.clone()), admin_headers)
            .await
            .unwrap();
        assert_eq!(admin_graph[0].points.len(), 2);

        let Json(profile) = get_team_profile(State(state), player_headers, Path(7))
            .await
            .unwrap();
        assert_eq!(profile.team.score, 100);
        assert_eq!(profile.solve_history.len(), 1);
    }

    #[tokio::test]
    async fn websocket_score_updates_respect_the_freeze() {
        let state = test_state();
        let mut rx = state.ws_hub.subscribe();
        {
            let conn = state.db.get().unwrap();
            conn.execute(
                "INSERT INTO teams (id, name, invite_code, score) VALUES (3, 'Live', 'LIVE0001', 0)",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO users (id, username, password_hash, role, team_id, created_at)
                 VALUES (30, 'liver', 'h', 'player', 3, 1)",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO challenges (id, slug, title, description, category, flag_hash,
                    flag_salt, points, is_hidden, created_at)
                 VALUES (9, 'late', 'Late', 'd', 'web', 'h', 's', 100, 0, 1)",
                [],
            )
            .unwrap();
            crate::competition::freeze(&conn, 20).unwrap();
            conn.execute(
                "INSERT INTO solves (team_id, user_id, challenge_id, solved_at) VALUES (3, 30, 9, 30)",
                [],
            )
            .unwrap();
            crate::scoring::recalculate_all_team_scores(&conn).unwrap();
            broadcast_score_update(&state, &conn);
        }
        match rx.try_recv().unwrap() {
            crate::handlers::ws::WsEvent::ScoreUpdate { scoreboard, .. } => {
                assert_eq!(
                    scoreboard[0].score, 0,
                    "solve after the freeze must not leak"
                );
            }
            other => panic!("expected score update, got {other:?}"),
        }
    }
}
