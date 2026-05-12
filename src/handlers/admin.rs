use axum::{
    extract::{Path, Query, State},
    http::HeaderMap,
    middleware::Next,
    response::{IntoResponse, Json, Response},
};
use serde::{Deserialize, Serialize};

use crate::{
    AppState, auth,
    errors::{AppError, HandlerResult},
    models::{
        challenge::Challenge,
        team::Team,
        user::UserPublic,
    },
    scoring, WsEvent,
};

// ---- request / response types ----

#[derive(Debug, Deserialize)]
pub struct CreateChallengeRequest {
    pub title: String,
    pub category: String,
    pub description: String,
    pub flag: String,
    pub flag_type: String,
    pub flag_case_sensitive: bool,
    pub points: i64,
    pub max_points: i64,
    pub min_points: i64,
    pub decay_rate: i64,
    pub author: Option<String>,
    pub tags: Vec<String>,
    pub unlock_requires: Option<i64>,
    pub is_hidden: bool,
}

#[derive(Debug, Deserialize)]
pub struct UpdateChallengeRequest {
    pub title: Option<String>,
    pub category: Option<String>,
    pub description: Option<String>,
    pub flag: Option<String>,
    pub flag_type: Option<String>,
    pub flag_case_sensitive: Option<bool>,
    pub points: Option<i64>,
    pub max_points: Option<i64>,
    pub min_points: Option<i64>,
    pub decay_rate: Option<i64>,
    pub author: Option<Option<String>>,
    pub tags: Option<Vec<String>>,
    pub unlock_requires: Option<Option<i64>>,
    pub is_hidden: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct SubmissionsQuery {
    pub team_id: Option<i64>,
    pub challenge_id: Option<i64>,
    pub correct: Option<bool>,
    pub page: Option<i64>,
    pub per_page: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct SubmissionRecord {
    pub id: i64,
    pub team_id: i64,
    pub user_id: i64,
    pub challenge_id: i64,
    pub flag: String,
    pub is_correct: bool,
    pub ip_address: Option<String>,
    pub submitted_at: i64,
}

#[derive(Debug, Serialize)]
pub struct PaginatedSubmissions {
    pub submissions: Vec<SubmissionRecord>,
    pub total: i64,
    pub page: i64,
    pub per_page: i64,
}

#[derive(Debug, Deserialize)]
pub struct AnnounceRequest {
    pub title: String,
    pub body: String,
    pub challenge_id: Option<i64>,
}

// ---- admin auth middleware ----

pub async fn require_admin(
    State(state): State<AppState>,
    headers: HeaderMap,
    request: axum::extract::Request,
    next: Next,
) -> Result<Response, AppError> {
    let token = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .ok_or(AppError::Unauthorized)?;
    let claims = auth::verify_jwt(token, &state.config.auth.jwt_secret)?;
    if claims.role != "admin" {
        return Err(AppError::Forbidden);
    }
    if !auth::is_session_valid(&state.db, &auth::hash_token(token))? {
        return Err(AppError::Unauthorized);
    }
    Ok(next.run(request).await)
}

// ---- dashboard ----

pub async fn dashboard(State(state): State<AppState>) -> HandlerResult<Json<serde_json::Value>> {
    let conn = state
        .db
        .get()
        .map_err(|e| anyhow::anyhow!("db pool: {e}"))?;
    let challenge_count: i64 =
        conn.query_row("SELECT COUNT(*) FROM challenges", [], |r| r.get(0))?;
    let team_count: i64 = conn.query_row("SELECT COUNT(*) FROM teams", [], |r| r.get(0))?;
    let user_count: i64 = conn.query_row("SELECT COUNT(*) FROM users", [], |r| r.get(0))?;
    let solve_count: i64 = conn.query_row("SELECT COUNT(*) FROM solves", [], |r| r.get(0))?;
    Ok(Json(serde_json::json!({
        "challenges": challenge_count,
        "teams": team_count,
        "users": user_count,
        "solves": solve_count,
    })))
}

// ---- challenge CRUD ----

pub async fn create_challenge(
    State(state): State<AppState>,
    Json(req): Json<CreateChallengeRequest>,
) -> HandlerResult<Json<Challenge>> {
    if req.title.trim().is_empty() {
        return Err(AppError::BadRequest("title is required".into()));
    }
    let slug = slugify(&req.title);
    let salt = generate_salt();
    let flag_hash = if req.flag_type == "regex" {
        req.flag.clone()
    } else {
        auth::hash_flag(&req.flag, &salt)
    };
    let tags_json = serde_json::to_string(&req.tags).unwrap_or_else(|_| "[]".into());
    let now = chrono::Utc::now().timestamp();

    let conn = state
        .db
        .get()
        .map_err(|e| anyhow::anyhow!("db pool: {e}"))?;
    conn.execute(
        "INSERT INTO challenges (
            slug, title, description, category, flag_hash, flag_salt, flag_type,
            flag_case_sensitive, points, max_points, min_points, decay_rate,
            author, tags, unlock_requires, is_hidden, created_at
         ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)",
        rusqlite::params![
            slug,
            req.title,
            req.description,
            req.category,
            flag_hash,
            salt,
            req.flag_type,
            req.flag_case_sensitive as i64,
            req.points,
            req.max_points,
            req.min_points,
            req.decay_rate,
            req.author,
            tags_json,
            req.unlock_requires,
            req.is_hidden as i64,
            now,
        ],
    )?;
    let id = conn.last_insert_rowid();
    let challenge = Challenge::find_by_id(&conn, id)?
        .ok_or_else(|| anyhow::anyhow!("challenge not found after insert"))?;
    state.cache.invalidate_challenges();
    Ok(Json(challenge))
}

pub async fn update_challenge(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(req): Json<UpdateChallengeRequest>,
) -> HandlerResult<Json<Challenge>> {
    let conn = state
        .db
        .get()
        .map_err(|e| anyhow::anyhow!("db pool: {e}"))?;
    let existing = Challenge::find_by_id(&conn, id)?
        .ok_or_else(|| AppError::NotFound("challenge not found".into()))?;

    let title = req.title.as_deref().unwrap_or(&existing.title);
    let slug = if req.title.is_some() {
        slugify(title)
    } else {
        existing.slug.clone()
    };
    let (flag_hash, flag_salt) = if let Some(ref new_flag) = req.flag {
        let salt = generate_salt();
        let flag_type = req.flag_type.as_deref().unwrap_or(&existing.flag_type);
        let hash = if flag_type == "regex" {
            new_flag.clone()
        } else {
            auth::hash_flag(new_flag, &salt)
        };
        (hash, salt)
    } else {
        (existing.flag_hash.clone(), existing.flag_salt.clone())
    };
    let tags_json = req
        .tags
        .as_ref()
        .map(|t| serde_json::to_string(t).unwrap_or_else(|_| "[]".into()))
        .unwrap_or_else(|| existing.tags.clone().unwrap_or_else(|| "[]".into()));
    let author: Option<String> = req
        .author
        .unwrap_or_else(|| existing.author.clone().map(Some).unwrap_or(None));
    let unlock_requires: Option<i64> = req
        .unlock_requires
        .unwrap_or_else(|| existing.unlock_requires.map(Some).unwrap_or(None));

    conn.execute(
        "UPDATE challenges SET
            slug=?1, title=?2, description=?3, category=?4,
            flag_hash=?5, flag_salt=?6, flag_type=?7, flag_case_sensitive=?8,
            points=?9, max_points=?10, min_points=?11, decay_rate=?12,
            author=?13, tags=?14, unlock_requires=?15, is_hidden=?16
         WHERE id=?17",
        rusqlite::params![
            slug,
            title,
            req.description.as_deref().unwrap_or(&existing.description),
            req.category.as_deref().unwrap_or(&existing.category),
            flag_hash,
            flag_salt,
            req.flag_type.as_deref().unwrap_or(&existing.flag_type),
            req.flag_case_sensitive
                .map(|b| b as i64)
                .unwrap_or(existing.flag_case_sensitive as i64),
            req.points.unwrap_or(existing.points),
            req.max_points.unwrap_or(existing.max_points),
            req.min_points.unwrap_or(existing.min_points),
            req.decay_rate.unwrap_or(existing.decay_rate),
            author,
            tags_json,
            unlock_requires,
            req.is_hidden
                .map(|b| b as i64)
                .unwrap_or(existing.is_hidden as i64),
            id,
        ],
    )?;
    state.cache.invalidate_challenges();
    state.cache.invalidate_scoreboard();
    let updated = Challenge::find_by_id(&conn, id)?
        .ok_or_else(|| anyhow::anyhow!("challenge not found after update"))?;
    Ok(Json(updated))
}

pub async fn delete_challenge(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> HandlerResult<Json<serde_json::Value>> {
    let conn = state
        .db
        .get()
        .map_err(|e| anyhow::anyhow!("db pool: {e}"))?;
    let rows = conn.execute(
        "DELETE FROM challenges WHERE id = ?1",
        rusqlite::params![id],
    )?;
    if rows == 0 {
        return Err(AppError::NotFound("challenge not found".into()));
    }
    state.cache.invalidate_challenges();
    state.cache.invalidate_scoreboard();
    Ok(Json(serde_json::json!({ "deleted": true })))
}

// ---- submission log ----

pub async fn list_submissions(
    State(state): State<AppState>,
    Query(q): Query<SubmissionsQuery>,
) -> HandlerResult<Json<PaginatedSubmissions>> {
    let page = q.page.unwrap_or(1).max(1);
    let per_page = q.per_page.unwrap_or(50).clamp(1, 200);
    let offset = (page - 1) * per_page;
    let correct_filter: Option<i64> = q.correct.map(|b| b as i64);

    let conn = state
        .db
        .get()
        .map_err(|e| anyhow::anyhow!("db pool: {e}"))?;

    let total: i64 = conn.query_row(
        "SELECT COUNT(*) FROM submissions
         WHERE (?1 IS NULL OR team_id = ?1)
           AND (?2 IS NULL OR challenge_id = ?2)
           AND (?3 IS NULL OR is_correct = ?3)",
        rusqlite::params![q.team_id, q.challenge_id, correct_filter],
        |r| r.get(0),
    )?;

    let mut stmt = conn.prepare(
        "SELECT id, team_id, user_id, challenge_id, flag, is_correct, ip_address, submitted_at
         FROM submissions
         WHERE (?1 IS NULL OR team_id = ?1)
           AND (?2 IS NULL OR challenge_id = ?2)
           AND (?3 IS NULL OR is_correct = ?3)
         ORDER BY submitted_at DESC
         LIMIT ?4 OFFSET ?5",
    )?;
    let submissions = stmt
        .query_map(
            rusqlite::params![q.team_id, q.challenge_id, correct_filter, per_page, offset],
            |row| {
                Ok(SubmissionRecord {
                    id: row.get(0)?,
                    team_id: row.get(1)?,
                    user_id: row.get(2)?,
                    challenge_id: row.get(3)?,
                    flag: row.get(4)?,
                    is_correct: row.get::<_, i64>(5)? != 0,
                    ip_address: row.get(6)?,
                    submitted_at: row.get(7)?,
                })
            },
        )?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Json(PaginatedSubmissions {
        submissions,
        total,
        page,
        per_page,
    }))
}

// ---- user management ----

pub async fn get_users(State(state): State<AppState>) -> HandlerResult<Json<Vec<UserPublic>>> {
    let conn = state
        .db
        .get()
        .map_err(|e| anyhow::anyhow!("db pool: {e}"))?;
    let mut stmt =
        conn.prepare("SELECT id, username, role, team_id FROM users ORDER BY id")?;
    let users = stmt
        .query_map([], |row| {
            Ok(UserPublic {
                id: row.get(0)?,
                username: row.get(1)?,
                role: row.get(2)?,
                team_id: row.get(3)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(users))
}

pub async fn ban_user(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> HandlerResult<Json<serde_json::Value>> {
    let conn = state
        .db
        .get()
        .map_err(|e| anyhow::anyhow!("db pool: {e}"))?;
    let rows = conn.execute(
        "UPDATE users SET role = 'banned' WHERE id = ?1 AND role != 'admin'",
        rusqlite::params![id],
    )?;
    if rows == 0 {
        return Err(AppError::NotFound("user not found or is admin".into()));
    }
    Ok(Json(serde_json::json!({ "banned": true })))
}

// ---- team management ----

pub async fn get_teams(State(state): State<AppState>) -> HandlerResult<Json<Vec<Team>>> {
    let conn = state
        .db
        .get()
        .map_err(|e| anyhow::anyhow!("db pool: {e}"))?;
    let mut stmt = conn.prepare(
        "SELECT id, name, invite_code, score, last_solve_at FROM teams ORDER BY score DESC",
    )?;
    let teams = stmt
        .query_map([], |row| {
            Ok(Team {
                id: row.get(0)?,
                name: row.get(1)?,
                invite_code: row.get(2)?,
                score: row.get(3)?,
                last_solve_at: row.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(teams))
}

pub async fn disqualify_team(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> HandlerResult<Json<serde_json::Value>> {
    let conn = state
        .db
        .get()
        .map_err(|e| anyhow::anyhow!("db pool: {e}"))?;
    let rows = conn.execute(
        "UPDATE teams SET score = 0, last_solve_at = NULL, is_disqualified = 1 WHERE id = ?1",
        rusqlite::params![id],
    )?;
    if rows == 0 {
        return Err(AppError::NotFound("team not found".into()));
    }
    scoring::recalculate_all_team_scores(&conn)?;
    state.cache.invalidate_scoreboard();
    Ok(Json(serde_json::json!({ "disqualified": true })))
}

// ---- competition controls ----

pub async fn competition_start(
    State(state): State<AppState>,
) -> HandlerResult<Json<serde_json::Value>> {
    state.ws_hub.broadcast(WsEvent::StateChange {
        started: true,
        ended: false,
        frozen: false,
    });
    Ok(Json(serde_json::json!({ "started": true })))
}

pub async fn competition_end(
    State(state): State<AppState>,
) -> HandlerResult<Json<serde_json::Value>> {
    state.ws_hub.broadcast(WsEvent::StateChange {
        started: false,
        ended: true,
        frozen: false,
    });
    Ok(Json(serde_json::json!({ "ended": true })))
}

pub async fn competition_freeze(
    State(state): State<AppState>,
) -> HandlerResult<Json<serde_json::Value>> {
    state.ws_hub.broadcast(WsEvent::StateChange {
        started: true,
        ended: false,
        frozen: true,
    });
    Ok(Json(serde_json::json!({ "frozen": true })))
}

pub async fn announce(
    State(state): State<AppState>,
    Json(req): Json<AnnounceRequest>,
) -> HandlerResult<Json<serde_json::Value>> {
    if req.title.trim().is_empty() {
        return Err(AppError::BadRequest("title is required".into()));
    }
    let conn = state
        .db
        .get()
        .map_err(|e| anyhow::anyhow!("db pool: {e}"))?;
    let now = chrono::Utc::now().timestamp();
    conn.execute(
        "INSERT INTO announcements (title, body, challenge_id, is_visible, created_at)
         VALUES (?1, ?2, ?3, 1, ?4)",
        rusqlite::params![req.title, req.body, req.challenge_id, now],
    )?;
    state.ws_hub.broadcast(WsEvent::Announcement {
        title: req.title,
        body: req.body,
    });
    Ok(Json(serde_json::json!({ "sent": true })))
}

// ---- backup ----

pub async fn backup(State(state): State<AppState>) -> Response {
    let result = do_backup(&state);
    match result {
        Ok((bytes, filename)) => axum::response::Response::builder()
            .status(axum::http::StatusCode::OK)
            .header(axum::http::header::CONTENT_TYPE, "application/octet-stream")
            .header(
                axum::http::header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{filename}\""),
            )
            .body(axum::body::Body::from(bytes))
            .unwrap(),
        Err(e) => e.into_response(),
    }
}

fn do_backup(state: &AppState) -> Result<(Vec<u8>, String), AppError> {
    let timestamp = chrono::Utc::now().timestamp();
    let filename = format!("feralctf-backup-{timestamp}.db");
    let tmp_path =
        std::env::temp_dir().join(format!("feralctf-backup-{}.db", uuid::Uuid::new_v4()));
    let src = state
        .db
        .get()
        .map_err(|e| anyhow::anyhow!("db pool: {e}"))?;
    let mut dst =
        rusqlite::Connection::open(&tmp_path).map_err(|e| anyhow::anyhow!("backup open: {e}"))?;
    {
        let backup = rusqlite::backup::Backup::new(&src, &mut dst)
            .map_err(|e| anyhow::anyhow!("backup init: {e}"))?;
        backup
            .run_to_completion(100, std::time::Duration::ZERO, None)
            .map_err(|e| anyhow::anyhow!("backup run: {e}"))?;
    }
    drop(dst);
    let bytes =
        std::fs::read(&tmp_path).map_err(|e| anyhow::anyhow!("backup read: {e}"))?;
    let _ = std::fs::remove_file(&tmp_path);
    Ok((bytes, filename))
}

// ---- helpers ----

fn slugify(title: &str) -> String {
    title
        .to_lowercase()
        .chars()
        .map(|c| if c == ' ' { '-' } else { c })
        .filter(|c| c.is_alphanumeric() || *c == '-')
        .collect()
}

fn generate_salt() -> String {
    use rand::RngCore;
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut bytes = [0u8; 16];
    rand::rng().fill_bytes(&mut bytes);
    let mut out = String::with_capacity(32);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

// ---- tests ----

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        auth,
        cache::AppCache,
        config::Config,
        db,
        models::user::{RegisterRequest, User},
        WsHub,
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
            ws_hub: Arc::new(WsHub::new()),
        }
    }

    fn make_token(state: &AppState, role: &str) -> String {
        let (user_id, token) = {
            let conn = state.db.get().unwrap();
            let req = RegisterRequest {
                username: format!("user-{role}"),
                email: None,
                password: "password123".to_string(),
                team_name: None,
                invite_code: None,
            };
            let user = User::create(&conn, &req, "hash").unwrap();
            if role == "admin" {
                conn.execute(
                    "UPDATE users SET role = 'admin' WHERE id = ?1",
                    rusqlite::params![user.id],
                )
                .unwrap();
            }
            let now = chrono::Utc::now().timestamp() as u64;
            let claims = auth::Claims {
                sub: user.id,
                role: role.to_string(),
                team_id: None,
                iat: now,
                exp: now + 3600,
            };
            let token = auth::sign_jwt(&claims, &state.config.auth.jwt_secret).unwrap();
            (user.id, token)
        }; // conn dropped here — pool slot free for create_session
        auth::create_session(&state.db, user_id, &token, 1).unwrap();
        token
    }

    fn admin_headers(state: &AppState) -> HeaderMap {
        let token = make_token(state, "admin");
        let mut h = HeaderMap::new();
        h.insert(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {token}").parse().unwrap(),
        );
        h
    }

    fn player_headers(state: &AppState) -> HeaderMap {
        let token = make_token(state, "player");
        let mut h = HeaderMap::new();
        h.insert(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {token}").parse().unwrap(),
        );
        h
    }

    fn verify_admin_check(state: &AppState, headers: &HeaderMap) -> Result<(), AppError> {
        let token = headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.strip_prefix("Bearer "))
            .ok_or(AppError::Unauthorized)?;
        let claims = auth::verify_jwt(token, &state.config.auth.jwt_secret)?;
        if claims.role != "admin" {
            return Err(AppError::Forbidden);
        }
        if !auth::is_session_valid(&state.db, &auth::hash_token(token))? {
            return Err(AppError::Unauthorized);
        }
        Ok(())
    }

    #[test]
    fn non_admin_jwt_returns_forbidden() {
        let state = test_state();
        let headers = player_headers(&state);
        let result = verify_admin_check(&state, &headers);
        assert!(matches!(result, Err(AppError::Forbidden)));
    }

    #[test]
    fn admin_jwt_passes_check() {
        let state = test_state();
        let headers = admin_headers(&state);
        assert!(verify_admin_check(&state, &headers).is_ok());
    }

    #[tokio::test]
    async fn create_challenge_hashes_flag() {
        let state = test_state();
        let req = CreateChallengeRequest {
            title: "Test Chal".to_string(),
            category: "web".to_string(),
            description: "desc".to_string(),
            flag: "flag{secret}".to_string(),
            flag_type: "static".to_string(),
            flag_case_sensitive: false,
            points: 100,
            max_points: 500,
            min_points: 50,
            decay_rate: 12,
            author: None,
            tags: vec!["web".to_string()],
            unlock_requires: None,
            is_hidden: true,
        };
        let Json(challenge) = create_challenge(State(state), Json(req)).await.unwrap();
        assert_ne!(challenge.flag_hash, "flag{secret}");
        assert_eq!(challenge.slug, "test-chal");
        assert!(!challenge.flag_hash.is_empty());
    }

    #[tokio::test]
    async fn disqualify_sets_score_to_zero_and_invalidates_cache() {
        let state = test_state();
        {
            let conn = state.db.get().unwrap();
            conn.execute(
                "INSERT INTO teams (id, name, invite_code, score) VALUES (1, 'Cheaters', 'CHEAT123', 500)",
                [],
            )
            .unwrap();
        }
        // Populate cache so we can verify invalidation
        {
            let conn = state.db.get().unwrap();
            state.cache.get_or_build_scoreboard(&conn).unwrap();
        }
        assert!(state.cache.is_scoreboard_cached());

        let Json(result) = disqualify_team(State(state.clone()), Path(1))
            .await
            .unwrap();
        assert_eq!(result["disqualified"], true);
        assert!(!state.cache.is_scoreboard_cached());

        let conn = state.db.get().unwrap();
        let score: i64 = conn
            .query_row("SELECT score FROM teams WHERE id = 1", [], |r| r.get(0))
            .unwrap();
        assert_eq!(score, 0);
    }

    #[tokio::test]
    async fn ban_user_sets_role_to_banned() {
        let state = test_state();
        let conn = state.db.get().unwrap();
        let req = RegisterRequest {
            username: "target".to_string(),
            email: None,
            password: "password123".to_string(),
            team_name: None,
            invite_code: None,
        };
        let user = User::create(&conn, &req, "hash").unwrap();
        drop(conn);

        let Json(result) = ban_user(State(state.clone()), Path(user.id))
            .await
            .unwrap();
        assert_eq!(result["banned"], true);

        let conn = state.db.get().unwrap();
        let role: String = conn
            .query_row(
                "SELECT role FROM users WHERE id = ?1",
                rusqlite::params![user.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(role, "banned");
    }

    #[test]
    fn slugify_converts_title_correctly() {
        assert_eq!(slugify("Hello World"), "hello-world");
        assert_eq!(slugify("SQL Injection!"), "sql-injection");
        assert_eq!(slugify("XSS <script>"), "xss-script");
    }

    #[tokio::test]
    async fn backup_produces_valid_sqlite_file() {
        let state = test_state();
        let (bytes, filename) = do_backup(&state).unwrap();
        assert!(filename.starts_with("feralctf-backup-"));
        assert!(filename.ends_with(".db"));
        // SQLite files start with the SQLite magic header
        assert_eq!(&bytes[..16], b"SQLite format 3\0");
    }
}
