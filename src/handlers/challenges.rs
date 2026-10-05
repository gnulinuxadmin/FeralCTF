use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};

use crate::{
    AppState, anticheat, auth,
    errors::{AppError, HandlerResult},
    models::{
        challenge::{
            Challenge, ChallengeFile, ChallengePublic, Hint, HintPublic, Submission,
            insert_score_history, insert_solve,
        },
        user::User,
    },
    scoring,
};

#[derive(Debug, Serialize)]
pub struct ChallengeListResponse {
    pub challenges: Vec<ChallengePublic>,
    pub total: u64,
}

#[derive(Debug, Serialize)]
pub struct ChallengeDetailResponse {
    pub challenge: ChallengePublic,
    pub hints: Vec<HintPublic>,
    pub files: Vec<ChallengeFile>,
}

#[derive(Debug, Deserialize)]
pub struct SubmitFlagRequest {
    pub flag: String,
}

#[derive(Debug, Serialize)]
pub struct SubmitResponse {
    pub correct: bool,
    pub points_earned: i64,
    pub first_blood: bool,
    pub new_score: i64,
    pub message: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct HintUnlockResponse {
    pub unlocked: bool,
    pub points_deducted: i64,
    pub new_score: i64,
    pub content: Option<String>,
}

pub async fn list_challenges(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> HandlerResult<Json<ChallengeListResponse>> {
    let user = current_user(&state, &headers)?;
    let team_id = user.team_id.unwrap_or(0);
    let conn = state
        .db
        .get()
        .map_err(|err| anyhow::anyhow!("db pool: {err}"))?;

    let challenges = state
        .cache
        .get_or_build_challenges(&conn)?
        .into_iter()
        .map(|challenge| challenge.to_public(&conn, team_id))
        .collect::<Result<Vec<_>, _>>()?;
    let total = challenges.len() as u64;

    Ok(Json(ChallengeListResponse { challenges, total }))
}

pub async fn get_challenge(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> HandlerResult<Json<ChallengeDetailResponse>> {
    let user = current_user(&state, &headers)?;
    let team_id = user.team_id.unwrap_or(0);
    let conn = state
        .db
        .get()
        .map_err(|err| anyhow::anyhow!("db pool: {err}"))?;
    let challenge = visible_challenge(&conn, id)?;
    ensure_unlocked(&conn, &challenge, team_id)?;
    let public = challenge.to_public(&conn, team_id)?;
    let hints = Hint::list_public_for_team(&conn, id, team_id)?;
    let files = ChallengeFile::list_by_challenge(&conn, id)?;

    Ok(Json(ChallengeDetailResponse {
        challenge: public,
        hints,
        files,
    }))
}

pub async fn submit_flag(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(request): Json<SubmitFlagRequest>,
) -> HandlerResult<Json<SubmitResponse>> {
    let user = current_user(&state, &headers)?;
    let team_id = require_team_id(&user)?;
    let ip = request_ip(&headers);
    state
        .rate_limiter
        .check_submission(team_id, id, &state.config.rate_limit)?;

    let submitted = request.flag.trim().to_string();
    if submitted.len() > 256 {
        return Err(AppError::BadRequest(
            "flag must be at most 256 characters".into(),
        ));
    }

    let conn = state
        .db
        .get()
        .map_err(|err| anyhow::anyhow!("db pool: {err}"))?;
    ensure_competition_running(&state, &conn, &user)?;
    let challenge = visible_challenge(&conn, id)?;
    ensure_unlocked(&conn, &challenge, team_id)?;

    if Challenge::is_solved_by_team(&conn, id, team_id)? {
        Submission::create(&conn, team_id, user.id, id, &submitted, false, Some(&ip))?;
        state.rate_limiter.record_attempt(team_id, id, false);
        let new_score = team_score(&conn, team_id)?;
        return Ok(Json(SubmitResponse {
            correct: false,
            points_earned: 0,
            first_blood: false,
            new_score,
            message: Some("already solved".to_string()),
        }));
    }

    let correct = verify_submission(&challenge, &submitted)?;
    Submission::create(&conn, team_id, user.id, id, &submitted, correct, Some(&ip))?;
    state.rate_limiter.record_attempt(team_id, id, correct);

    if !correct {
        return Ok(Json(SubmitResponse {
            correct: false,
            points_earned: 0,
            first_blood: false,
            new_score: team_score(&conn, team_id)?,
            message: Some("Incorrect flag.".to_string()),
        }));
    }

    let first_blood = Challenge::solve_count(&conn, id)? == 0;
    let solved_at = chrono::Utc::now().timestamp();
    insert_solve(&conn, team_id, user.id, id, solved_at)?;
    scoring::recalculate_challenge_points(&conn, id)?;
    state.cache.invalidate_scoreboard();

    let points_earned = current_challenge_points(&conn, id)?;
    let new_score = team_score(&conn, team_id)?;
    insert_score_history(&conn, team_id, new_score, solved_at)?;
    anticheat::check_flag_sharing(
        &conn,
        id,
        team_id,
        &submitted,
        state.config.rate_limit.flag_sharing_window_seconds,
    )?;

    let team_name: String = conn
        .query_row(
            "SELECT name FROM teams WHERE id = ?1",
            rusqlite::params![team_id],
            |row| row.get(0),
        )
        .unwrap_or_default();

    state
        .ws_hub
        .broadcast(crate::handlers::ws::WsEvent::NewSolve {
            team: team_name,
            challenge: challenge.title.clone(),
            points: points_earned,
            first_blood,
        });

    crate::handlers::scoreboard::broadcast_score_update(&state, &conn);

    Ok(Json(SubmitResponse {
        correct: true,
        points_earned,
        first_blood,
        new_score,
        message: None,
    }))
}

pub async fn unlock_hint(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((challenge_id, hint_id)): Path<(i64, i64)>,
) -> HandlerResult<Json<HintUnlockResponse>> {
    let user = current_user(&state, &headers)?;
    let team_id = require_team_id(&user)?;
    let conn = state
        .db
        .get()
        .map_err(|err| anyhow::anyhow!("db pool: {err}"))?;
    ensure_competition_running(&state, &conn, &user)?;
    let challenge = visible_challenge(&conn, challenge_id)?;
    ensure_unlocked(&conn, &challenge, team_id)?;
    let hint = Hint::find_by_id(&conn, hint_id)?
        .ok_or_else(|| AppError::NotFound("hint not found".to_string()))?;
    if hint.challenge_id != challenge_id {
        return Err(AppError::NotFound("hint not found".to_string()));
    }

    if Hint::is_unlocked(&conn, team_id, hint_id)? {
        return Ok(Json(HintUnlockResponse {
            unlocked: true,
            points_deducted: 0,
            new_score: team_score(&conn, team_id)?,
            content: Some(hint.content),
        }));
    }

    if Challenge::is_solved_by_team(&conn, challenge_id, team_id)? {
        return Err(AppError::BadRequest(
            "challenge already solved; hints are no longer needed".to_string(),
        ));
    }
    // Hints may not push a team below zero (free hints are always allowed).
    let current_score = team_score(&conn, team_id)?;
    if hint.cost_points > 0 && hint.cost_points > current_score {
        return Err(AppError::BadRequest(format!(
            "not enough points: hint costs {}, team has {current_score}",
            hint.cost_points
        )));
    }

    let now = chrono::Utc::now().timestamp();
    if !Hint::unlock(&conn, team_id, hint_id, hint.cost_points, now)? {
        // A concurrent request unlocked it first; don't charge twice.
        return Ok(Json(HintUnlockResponse {
            unlocked: true,
            points_deducted: 0,
            new_score: current_score,
            content: Some(hint.content),
        }));
    }
    scoring::recalculate_all_team_scores(&conn)?;
    let new_score = team_score(&conn, team_id)?;
    insert_score_history(&conn, team_id, new_score, now)?;
    state.cache.invalidate_scoreboard();
    crate::handlers::scoreboard::broadcast_score_update(&state, &conn);

    Ok(Json(HintUnlockResponse {
        unlocked: true,
        points_deducted: hint.cost_points,
        new_score,
        content: Some(hint.content),
    }))
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

fn require_team_id(user: &User) -> Result<i64, AppError> {
    user.team_id
        .ok_or_else(|| AppError::BadRequest("user must join a team first".to_string()))
}

fn visible_challenge(conn: &crate::db::DbConn, id: i64) -> Result<Challenge, AppError> {
    let challenge = Challenge::find_by_id(conn, id)?
        .ok_or_else(|| AppError::NotFound("challenge not found".to_string()))?;
    if challenge.is_hidden {
        return Err(AppError::NotFound("challenge not found".to_string()));
    }
    Ok(challenge)
}

/// Admins may test challenges at any time; players only while it runs.
fn ensure_competition_running(
    state: &AppState,
    conn: &crate::db::DbConn,
    user: &User,
) -> Result<(), AppError> {
    if user.role == "admin" {
        return Ok(());
    }
    crate::competition::status(conn, &state.config.competition)?.ensure_running()
}

fn ensure_unlocked(
    conn: &crate::db::DbConn,
    challenge: &Challenge,
    team_id: i64,
) -> Result<(), AppError> {
    if challenge.is_locked_for_team(conn, team_id)? {
        return Err(AppError::Forbidden);
    }
    Ok(())
}

fn verify_submission(challenge: &Challenge, submitted: &str) -> Result<bool, AppError> {
    match challenge.flag_type.as_str() {
        "regex" => regex::RegexBuilder::new(&challenge.flag_hash)
            .case_insensitive(!challenge.flag_case_sensitive)
            .build()
            .map(|pattern| pattern.is_match(submitted))
            .map_err(|err| AppError::BadRequest(format!("invalid regex flag pattern: {err}"))),
        _ => Ok(auth::verify_flag(
            submitted,
            &challenge.flag_hash,
            &challenge.flag_salt,
            challenge.flag_case_sensitive,
        )),
    }
}

fn request_ip(headers: &HeaderMap) -> String {
    headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("unknown")
        .to_string()
}

fn team_score(conn: &crate::db::DbConn, team_id: i64) -> Result<i64, AppError> {
    let score = conn.query_row(
        "SELECT score FROM teams WHERE id = ?1",
        rusqlite::params![team_id],
        |row| row.get(0),
    )?;
    Ok(score)
}

fn current_challenge_points(conn: &crate::db::DbConn, challenge_id: i64) -> Result<i64, AppError> {
    let points = conn.query_row(
        "SELECT points FROM challenges WHERE id = ?1",
        rusqlite::params![challenge_id],
        |row| row.get(0),
    )?;
    Ok(points)
}

impl IntoResponse for SubmitResponse {
    fn into_response(self) -> Response {
        (StatusCode::OK, Json(self)).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        auth,
        cache::AppCache,
        config::Config,
        db,
        models::{
            team::Team,
            user::{RegisterRequest, User},
        },
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
            rate_limiter: Arc::new(anticheat::RateLimiter::new()),
        }
    }

    fn authed_user(state: &AppState) -> (HeaderMap, User, Team) {
        authed_user_named(state, "solver", "Solvers")
    }

    fn authed_user_named(
        state: &AppState,
        username: &str,
        team_name: &str,
    ) -> (HeaderMap, User, Team) {
        let conn = state.db.get().unwrap();
        let req = RegisterRequest {
            username: username.to_string(),
            email: None,
            password: "password123".to_string(),
            team_name: None,
            invite_code: None,
        };
        let user = User::create(&conn, &req, "hash").unwrap();
        let team = Team::create(&conn, team_name).unwrap();
        Team::add_member(&conn, team.id, user.id).unwrap();
        let user = User::find_by_id(&conn, user.id).unwrap().unwrap();
        drop(conn);

        let now = chrono::Utc::now().timestamp() as u64;
        let claims = auth::Claims {
            sub: user.id,
            role: user.role.clone(),
            team_id: user.team_id,
            iat: now,
            exp: now + 3600,
        };
        let token = auth::sign_jwt(&claims, &state.config.auth.jwt_secret).unwrap();
        auth::create_session(&state.db, user.id, &token, 1).unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {token}").parse().unwrap(),
        );
        headers.insert("x-forwarded-for", "127.0.0.1".parse().unwrap());
        (headers, user, team)
    }

    fn insert_challenge(
        state: &AppState,
        slug: &str,
        flag: &str,
        flag_type: &str,
        hidden: i64,
    ) -> i64 {
        let conn = state.db.get().unwrap();
        let salt = "salt";
        let stored_flag = if flag_type == "regex" {
            flag.to_string()
        } else {
            auth::hash_flag(flag, salt, false)
        };
        conn.execute(
            "INSERT INTO challenges (
                slug, title, description, category, flag_hash, flag_salt, flag_type,
                flag_case_sensitive, points, max_points, min_points, decay_rate, tags,
                is_hidden, created_at
             )
             VALUES (?1, ?2, 'desc', 'web', ?3, ?4, ?5, 0, 100, 500, 50, 12,
                '[\"web\"]', ?6, 1)",
            rusqlite::params![slug, slug, stored_flag, salt, flag_type, hidden],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    #[tokio::test]
    async fn list_and_detail_hide_flags_and_locked_hints() {
        let state = test_state();
        let (headers, _user, _team) = authed_user(&state);
        let visible_id = insert_challenge(&state, "visible", "flag{ok}", "static", 0);
        insert_challenge(&state, "hidden", "flag{hide}", "static", 1);
        {
            let conn = state.db.get().unwrap();
            conn.execute(
                "INSERT INTO hints (challenge_id, content, cost_points, sort_order)
                 VALUES (?1, 'secret hint', 25, 1)",
                rusqlite::params![visible_id],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO files (challenge_id, filename, storage_path, size_bytes, sha256)
                 VALUES (?1, 'file.txt', 'attachments/file.txt', 4, 'abcd')",
                rusqlite::params![visible_id],
            )
            .unwrap();
        }

        let Json(list) = list_challenges(State(state.clone()), headers.clone())
            .await
            .unwrap();
        assert_eq!(list.total, 1);
        assert_eq!(list.challenges[0].slug, "visible");
        assert_eq!(list.challenges[0].hint_count, 1);
        assert_eq!(list.challenges[0].file_count, 1);

        let Json(detail) = get_challenge(State(state), headers, Path(visible_id))
            .await
            .unwrap();
        assert_eq!(detail.hints.len(), 1);
        assert_eq!(detail.hints[0].content, None);
        assert_eq!(detail.files.len(), 1);
    }

    #[tokio::test]
    async fn submit_correct_static_flag_scores_and_records_history() {
        let state = test_state();
        let (headers, _user, team) = authed_user(&state);
        let challenge_id = insert_challenge(&state, "static", "flag{ok}", "static", 0);

        let Json(response) = submit_flag(
            State(state.clone()),
            headers,
            Path(challenge_id),
            Json(SubmitFlagRequest {
                flag: "  FLAG{OK}  ".to_string(),
            }),
        )
        .await
        .unwrap();

        assert!(response.correct);
        assert_eq!(response.points_earned, 100);
        assert!(response.first_blood);
        assert_eq!(response.new_score, 100);

        let conn = state.db.get().unwrap();
        let submissions: i64 = conn
            .query_row("SELECT COUNT(*) FROM submissions", [], |row| row.get(0))
            .unwrap();
        let solves: i64 = conn
            .query_row("SELECT COUNT(*) FROM solves", [], |row| row.get(0))
            .unwrap();
        let history: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM score_history WHERE team_id = ?1",
                rusqlite::params![team.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(submissions, 1);
        assert_eq!(solves, 1);
        assert_eq!(history, 1);
    }

    #[tokio::test]
    async fn submit_wrong_and_already_solved_are_graceful() {
        let state = test_state();
        let (headers, _user, _team) = authed_user(&state);
        let challenge_id = insert_challenge(&state, "static", "flag{ok}", "static", 0);

        let Json(wrong) = submit_flag(
            State(state.clone()),
            headers.clone(),
            Path(challenge_id),
            Json(SubmitFlagRequest {
                flag: "flag{nope}".to_string(),
            }),
        )
        .await
        .unwrap();
        assert!(!wrong.correct);
        assert_eq!(wrong.message.as_deref(), Some("Incorrect flag."));

        let Json(correct) = submit_flag(
            State(state.clone()),
            headers.clone(),
            Path(challenge_id),
            Json(SubmitFlagRequest {
                flag: "flag{ok}".to_string(),
            }),
        )
        .await
        .unwrap();
        assert!(correct.correct);

        let Json(again) = submit_flag(
            State(state.clone()),
            headers,
            Path(challenge_id),
            Json(SubmitFlagRequest {
                flag: "flag{ok}".to_string(),
            }),
        )
        .await
        .unwrap();
        assert!(!again.correct);
        assert_eq!(again.message.as_deref(), Some("already solved"));

        let conn = state.db.get().unwrap();
        let submissions: i64 = conn
            .query_row("SELECT COUNT(*) FROM submissions", [], |row| row.get(0))
            .unwrap();
        let solves: i64 = conn
            .query_row("SELECT COUNT(*) FROM solves", [], |row| row.get(0))
            .unwrap();
        assert_eq!(submissions, 3);
        assert_eq!(solves, 1);
    }

    #[tokio::test]
    async fn submit_rate_limited_after_ten_attempts_in_window() {
        let state = test_state();
        let (headers, _user, _team) = authed_user(&state);

        for index in 0..10 {
            let challenge_id =
                insert_challenge(&state, &format!("static-{index}"), "flag{ok}", "static", 0);
            let Json(response) = submit_flag(
                State(state.clone()),
                headers.clone(),
                Path(challenge_id),
                Json(SubmitFlagRequest {
                    flag: "flag{nope}".to_string(),
                }),
            )
            .await
            .unwrap();
            assert!(!response.correct);
        }

        let challenge_id = insert_challenge(&state, "static-limited", "flag{ok}", "static", 0);
        let err = submit_flag(
            State(state),
            headers,
            Path(challenge_id),
            Json(SubmitFlagRequest {
                flag: "flag{nope}".to_string(),
            }),
        )
        .await
        .unwrap_err();

        match err {
            AppError::RateLimited {
                retry_after_seconds,
            } => assert!(retry_after_seconds > 0),
            other => panic!("expected rate limited error, got {other:?}"),
        }
    }

    fn insert_hint(state: &AppState, challenge_id: i64, content: &str, cost: i64) -> i64 {
        let conn = state.db.get().unwrap();
        conn.execute(
            "INSERT INTO hints (challenge_id, content, cost_points, sort_order)
             VALUES (?1, ?2, ?3, 1)",
            rusqlite::params![challenge_id, content, cost],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    #[tokio::test]
    async fn regex_flags_and_hint_unlock_work() {
        let state = test_state();
        let (headers, _user, team) = authed_user(&state);
        let regex_id = insert_challenge(&state, "regex", r"^flag\{[0-9]+\}$", "regex", 0);
        let other_id = insert_challenge(&state, "other", "flag{other}", "static", 0);
        let hint_id = insert_hint(&state, other_id, "count it", 25);

        let Json(response) = submit_flag(
            State(state.clone()),
            headers.clone(),
            Path(regex_id),
            Json(SubmitFlagRequest {
                flag: "flag{123}".to_string(),
            }),
        )
        .await
        .unwrap();
        assert!(response.correct);

        let Json(unlocked) = unlock_hint(
            State(state.clone()),
            headers.clone(),
            Path((other_id, hint_id)),
        )
        .await
        .unwrap();
        assert_eq!(unlocked.points_deducted, 25);
        assert_eq!(unlocked.content.as_deref(), Some("count it"));
        assert_eq!(unlocked.new_score, 75);

        let Json(again) = unlock_hint(State(state), headers, Path((other_id, hint_id)))
            .await
            .unwrap();
        assert_eq!(again.points_deducted, 0);
        assert_eq!(again.new_score, 75);

        assert_eq!(team.name, "Solvers");
    }

    #[tokio::test]
    async fn hint_unlock_rejected_after_solve() {
        let state = test_state();
        let (headers, _user, _team) = authed_user(&state);
        let challenge_id = insert_challenge(&state, "solved", "flag{ok}", "static", 0);
        let hint_id = insert_hint(&state, challenge_id, "late", 10);
        let Json(solved) = submit_flag(
            State(state.clone()),
            headers.clone(),
            Path(challenge_id),
            Json(SubmitFlagRequest {
                flag: "flag{ok}".to_string(),
            }),
        )
        .await
        .unwrap();
        assert!(solved.correct);

        let err = unlock_hint(State(state.clone()), headers, Path((challenge_id, hint_id)))
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::BadRequest(_)));
        let conn = state.db.get().unwrap();
        let unlocks: i64 = conn
            .query_row("SELECT COUNT(*) FROM hint_unlocks", [], |row| row.get(0))
            .unwrap();
        assert_eq!(unlocks, 0);
    }

    #[tokio::test]
    async fn hint_unlock_cannot_go_negative_but_free_hints_work() {
        let state = test_state();
        let (headers, _user, team) = authed_user(&state);
        let challenge_id = insert_challenge(&state, "pricey", "flag{ok}", "static", 0);
        let paid = insert_hint(&state, challenge_id, "paid", 50);
        let free = insert_hint(&state, challenge_id, "free", 0);

        let err = unlock_hint(
            State(state.clone()),
            headers.clone(),
            Path((challenge_id, paid)),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, AppError::BadRequest(ref msg) if msg.contains("not enough points")));

        let Json(free_unlock) =
            unlock_hint(State(state.clone()), headers, Path((challenge_id, free)))
                .await
                .unwrap();
        assert_eq!(free_unlock.content.as_deref(), Some("free"));
        assert_eq!(free_unlock.new_score, 0);

        let conn = state.db.get().unwrap();
        assert_eq!(team_score(&conn, team.id).unwrap(), 0);
    }

    #[tokio::test]
    async fn unlocked_hint_content_only_after_unlock() {
        let state = test_state();
        let (headers, _user, _team) = authed_user(&state);
        let challenge_id = insert_challenge(&state, "detail", "flag{ok}", "static", 0);
        let hint_id = insert_hint(&state, challenge_id, "the secret", 0);

        let Json(before) = get_challenge(State(state.clone()), headers.clone(), Path(challenge_id))
            .await
            .unwrap();
        assert!(!before.hints[0].unlocked);
        assert!(
            serde_json::to_string(&before)
                .unwrap()
                .find("the secret")
                .is_none()
        );

        let _ = unlock_hint(
            State(state.clone()),
            headers.clone(),
            Path((challenge_id, hint_id)),
        )
        .await
        .unwrap();
        let Json(after) = get_challenge(State(state), headers, Path(challenge_id))
            .await
            .unwrap();
        assert!(after.hints[0].unlocked);
        assert_eq!(after.hints[0].content.as_deref(), Some("the secret"));
    }

    #[test]
    fn concurrent_unlock_insert_is_charged_once() {
        let state = test_state();
        let conn = state.db.get().unwrap();
        assert!(Hint::unlock(&conn, 1, 1, 10, 1).unwrap());
        assert!(!Hint::unlock(&conn, 1, 1, 10, 2).unwrap());
        let total: i64 = conn
            .query_row("SELECT SUM(points_deducted) FROM hint_unlocks", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(total, 10);
    }

    #[tokio::test]
    async fn locked_challenge_is_forbidden_until_prerequisite_solved() {
        let state = test_state();
        let (headers, _user, _team) = authed_user(&state);
        let first = insert_challenge(&state, "first", "flag{one}", "static", 0);
        let second = insert_challenge(&state, "second", "flag{two}", "static", 0);
        let hint_id = insert_hint(&state, second, "later", 0);
        {
            let conn = state.db.get().unwrap();
            conn.execute(
                "UPDATE challenges SET unlock_requires = ?1 WHERE id = ?2",
                rusqlite::params![first, second],
            )
            .unwrap();
        }

        let Json(list) = list_challenges(State(state.clone()), headers.clone())
            .await
            .unwrap();
        let locked = list.challenges.iter().find(|c| c.id == second).unwrap();
        assert!(locked.locked);
        assert!(locked.description.is_empty());

        assert!(matches!(
            get_challenge(State(state.clone()), headers.clone(), Path(second)).await,
            Err(AppError::Forbidden)
        ));
        assert!(matches!(
            submit_flag(
                State(state.clone()),
                headers.clone(),
                Path(second),
                Json(SubmitFlagRequest {
                    flag: "flag{two}".to_string()
                }),
            )
            .await,
            Err(AppError::Forbidden)
        ));
        assert!(matches!(
            unlock_hint(
                State(state.clone()),
                headers.clone(),
                Path((second, hint_id))
            )
            .await,
            Err(AppError::Forbidden)
        ));

        let Json(solved) = submit_flag(
            State(state.clone()),
            headers.clone(),
            Path(first),
            Json(SubmitFlagRequest {
                flag: "flag{one}".to_string(),
            }),
        )
        .await
        .unwrap();
        assert!(solved.correct);
        let Json(detail) = get_challenge(State(state), headers, Path(second))
            .await
            .unwrap();
        assert!(!detail.challenge.locked);
        assert_eq!(detail.challenge.description, "desc");
    }

    #[tokio::test]
    async fn case_sensitive_regex_and_static_flags() {
        let state = test_state();
        let (headers, _user, _team) = authed_user(&state);
        let regex_id = insert_challenge(&state, "cs-regex", r"^flag\{Abc\}$", "regex", 0);
        let static_id = insert_challenge(&state, "cs-static", "unused", "static", 0);
        {
            let conn = state.db.get().unwrap();
            conn.execute(
                "UPDATE challenges SET flag_case_sensitive = 1, flag_hash = ?1 WHERE id = ?2",
                rusqlite::params![auth::hash_flag("flag{Xyz}", "salt", true), static_id],
            )
            .unwrap();
            conn.execute(
                "UPDATE challenges SET flag_case_sensitive = 1 WHERE id = ?1",
                rusqlite::params![regex_id],
            )
            .unwrap();
        }
        for (id, wrong, right) in [
            (regex_id, "flag{abc}", "flag{Abc}"),
            (static_id, "flag{xyz}", "flag{Xyz}"),
        ] {
            let Json(miss) = submit_flag(
                State(state.clone()),
                headers.clone(),
                Path(id),
                Json(SubmitFlagRequest {
                    flag: wrong.to_string(),
                }),
            )
            .await
            .unwrap();
            assert!(!miss.correct, "{wrong} should not match");
            let Json(hit) = submit_flag(
                State(state.clone()),
                headers.clone(),
                Path(id),
                Json(SubmitFlagRequest {
                    flag: right.to_string(),
                }),
            )
            .await
            .unwrap();
            assert!(hit.correct, "{right} should match");
        }
    }

    #[tokio::test]
    async fn submissions_and_unlocks_blocked_after_end() {
        let state = test_state();
        let (headers, _user, _team) = authed_user(&state);
        let challenge_id = insert_challenge(&state, "ended", "flag{ok}", "static", 0);
        let hint_id = insert_hint(&state, challenge_id, "free", 0);
        {
            let conn = state.db.get().unwrap();
            crate::competition::end(&conn, 1).unwrap();
        }
        let err = submit_flag(
            State(state.clone()),
            headers.clone(),
            Path(challenge_id),
            Json(SubmitFlagRequest {
                flag: "flag{ok}".to_string(),
            }),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, AppError::BadRequest(ref m) if m.contains("ended")));
        let err = unlock_hint(State(state.clone()), headers, Path((challenge_id, hint_id)))
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::BadRequest(ref m) if m.contains("ended")));

        {
            let conn = state.db.get().unwrap();
            crate::competition::start(&conn, 2).unwrap();
        }
        let (headers, _user, _team) = authed_user_named(&state, "solver2", "Solvers2");
        let Json(ok) = submit_flag(
            State(state),
            headers,
            Path(challenge_id),
            Json(SubmitFlagRequest {
                flag: "flag{ok}".to_string(),
            }),
        )
        .await
        .unwrap();
        assert!(ok.correct);
    }
}
