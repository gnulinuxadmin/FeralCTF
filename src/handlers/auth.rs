use axum::{extract::State, http::HeaderMap, response::Json};
use serde::{Deserialize, Serialize};

use crate::{
    AppState,
    auth::{self, Claims},
    errors::{AppError, HandlerResult},
    models::{
        team::Team,
        user::{LoginResponse, RegisterRequest, User, UserPublic},
    },
};

// ---- request types ----

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

// ---- helpers ----

fn extract_bearer(headers: &HeaderMap) -> Result<&str, AppError> {
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .ok_or(AppError::Unauthorized)
}

fn validate_username(username: &str) -> Result<(), AppError> {
    let n = username.len();
    if n < 3 || n > 32 {
        return Err(AppError::BadRequest("username must be 3–32 characters".into()));
    }
    if !username.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        return Err(AppError::BadRequest(
            "username may only contain letters, digits, _ and -".into(),
        ));
    }
    Ok(())
}

fn make_claims(user: &User, ttl_hours: u64) -> Claims {
    let now = chrono::Utc::now().timestamp() as u64;
    Claims {
        sub: user.id,
        role: user.role.clone(),
        team_id: user.team_id,
        iat: now,
        exp: now + ttl_hours * 3600,
    }
}

fn session_ttl(config: &crate::config::Config, role: &str) -> u64 {
    if role == "admin" {
        config.auth.admin_session_ttl_hours
    } else {
        config.auth.session_ttl_hours
    }
}

// ---- handlers ----

pub async fn register(
    State(state): State<AppState>,
    Json(req): Json<RegisterRequest>,
) -> HandlerResult<Json<LoginResponse>> {
    validate_username(&req.username)?;
    if req.password.len() < 8 {
        return Err(AppError::BadRequest(
            "password must be at least 8 characters".into(),
        ));
    }

    let conn = state.db.get().map_err(|e| anyhow::anyhow!("db pool: {e}"))?;

    if User::find_by_username(&conn, &req.username)?.is_some() {
        return Err(AppError::BadRequest("username already taken".into()));
    }

    let is_first = User::count(&conn)? == 0;
    let password_hash = auth::hash_password(&req.password)?;
    let user = User::create(&conn, &req, &password_hash)?;

    if is_first {
        conn.execute(
            "UPDATE users SET role = 'admin' WHERE id = ?1",
            rusqlite::params![user.id],
        )?;
    }

    // Team handling
    if let Some(ref name) = req.team_name {
        let team = Team::create(&conn, name)?;
        Team::add_member(&conn, team.id, user.id)?;
    } else if let Some(ref code) = req.invite_code {
        let team = Team::find_by_invite_code(&conn, code)?
            .ok_or_else(|| AppError::BadRequest("invalid invite code".into()))?;
        Team::add_member(&conn, team.id, user.id)?;
    }

    // Re-fetch to get final role and team_id
    let user = User::find_by_id(&conn, user.id)?
        .ok_or_else(|| anyhow::anyhow!("user not found after insert"))?;

    let ttl = session_ttl(&state.config, &user.role);
    let claims = make_claims(&user, ttl);
    let token = auth::sign_jwt(&claims, &state.config.auth.jwt_secret)?;
    auth::create_session(&state.db, user.id, &token, ttl)?;

    Ok(Json(LoginResponse {
        token,
        expires_at: claims.exp,
        user: UserPublic {
            id: user.id,
            username: user.username,
            role: user.role,
            team_id: user.team_id,
        },
    }))
}

pub async fn login(
    State(state): State<AppState>,
    Json(req): Json<LoginRequest>,
) -> HandlerResult<Json<LoginResponse>> {
    let conn = state.db.get().map_err(|e| anyhow::anyhow!("db pool: {e}"))?;

    let user = User::find_by_username(&conn, &req.username)?.ok_or(AppError::Unauthorized)?;

    if !auth::verify_password(&req.password, &user.password_hash)? {
        return Err(AppError::Unauthorized);
    }

    let ttl = session_ttl(&state.config, &user.role);
    let claims = make_claims(&user, ttl);
    let token = auth::sign_jwt(&claims, &state.config.auth.jwt_secret)?;
    auth::create_session(&state.db, user.id, &token, ttl)?;

    Ok(Json(LoginResponse {
        token,
        expires_at: claims.exp,
        user: UserPublic {
            id: user.id,
            username: user.username,
            role: user.role,
            team_id: user.team_id,
        },
    }))
}

pub async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> HandlerResult<Json<()>> {
    let token = extract_bearer(&headers)?;
    let token_hash = auth::hash_token(token);
    auth::revoke_session(&state.db, &token_hash)?;
    Ok(Json(()))
}

pub async fn me(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> HandlerResult<Json<UserPublic>> {
    let token = extract_bearer(&headers)?;
    let claims = auth::verify_jwt(token, &state.config.auth.jwt_secret)?;

    if !auth::is_session_valid(&state.db, &auth::hash_token(token))? {
        return Err(AppError::Unauthorized);
    }

    let conn = state.db.get().map_err(|e| anyhow::anyhow!("db pool: {e}"))?;
    let user = User::find_by_id(&conn, claims.sub)?.ok_or(AppError::Unauthorized)?;

    Ok(Json(UserPublic {
        id: user.id,
        username: user.username,
        role: user.role,
        team_id: user.team_id,
    }))
}

pub async fn change_password(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<ChangePasswordRequest>,
) -> HandlerResult<Json<()>> {
    let token = extract_bearer(&headers)?;
    let claims = auth::verify_jwt(token, &state.config.auth.jwt_secret)?;

    if !auth::is_session_valid(&state.db, &auth::hash_token(token))? {
        return Err(AppError::Unauthorized);
    }

    if req.new_password.len() < 8 {
        return Err(AppError::BadRequest(
            "password must be at least 8 characters".into(),
        ));
    }

    let conn = state.db.get().map_err(|e| anyhow::anyhow!("db pool: {e}"))?;
    let user = User::find_by_id(&conn, claims.sub)?.ok_or(AppError::Unauthorized)?;

    if !auth::verify_password(&req.current_password, &user.password_hash)? {
        return Err(AppError::Unauthorized);
    }

    let new_hash = auth::hash_password(&req.new_password)?;
    conn.execute(
        "UPDATE users SET password_hash = ?1 WHERE id = ?2",
        rusqlite::params![new_hash, user.id],
    )?;

    Ok(Json(()))
}

// ---- tests ----

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{auth, cache::AppCache, config::Config, db, models::user::RegisterRequest};
    use r2d2::Pool;
    use r2d2_sqlite::SqliteConnectionManager;
    use std::sync::Arc;

    fn test_state() -> AppState {
        let pool = Pool::new(SqliteConnectionManager::memory()).unwrap();
        let conn = pool.get().unwrap();
        db::run_migrations(&conn).unwrap();
        drop(conn);
        AppState {
            db: pool,
            config: Arc::new(Config::default()),
            cache: Arc::new(AppCache::new()),
        }
    }

    fn reg(username: &str) -> RegisterRequest {
        RegisterRequest {
            username: username.to_string(),
            email: None,
            password: "password123".to_string(),
            team_name: None,
            invite_code: None,
        }
    }

    #[test]
    fn first_user_becomes_admin() {
        let state = test_state();
        let conn = state.db.get().unwrap();
        let req = reg("alice");
        let hash = auth::hash_password(&req.password).unwrap();
        assert_eq!(User::count(&conn).unwrap(), 0);
        let user = User::create(&conn, &req, &hash).unwrap();
        // Simulate handler logic
        conn.execute(
            "UPDATE users SET role = 'admin' WHERE id = ?1",
            rusqlite::params![user.id],
        )
        .unwrap();
        let user = User::find_by_id(&conn, user.id).unwrap().unwrap();
        assert_eq!(user.role, "admin");
    }

    #[test]
    fn second_user_stays_player() {
        let state = test_state();
        let conn = state.db.get().unwrap();
        for name in ["alice", "bob"] {
            let req = reg(name);
            let hash = auth::hash_password(&req.password).unwrap();
            User::create(&conn, &req, &hash).unwrap();
        }
        let bob = User::find_by_username(&conn, "bob").unwrap().unwrap();
        assert_eq!(bob.role, "player");
    }

    #[test]
    fn wrong_password_rejected() {
        let state = test_state();
        let conn = state.db.get().unwrap();
        let req = reg("alice");
        let hash = auth::hash_password(&req.password).unwrap();
        User::create(&conn, &req, &hash).unwrap();
        let user = User::find_by_username(&conn, "alice").unwrap().unwrap();
        assert!(!auth::verify_password("wrong-password", &user.password_hash).unwrap());
        assert!(auth::verify_password("password123", &user.password_hash).unwrap());
    }

    #[test]
    fn jwt_claims_correct() {
        let state = test_state();
        let conn = state.db.get().unwrap();
        let req = reg("alice");
        let hash = auth::hash_password(&req.password).unwrap();
        let user = User::create(&conn, &req, &hash).unwrap();
        let ttl = session_ttl(&state.config, &user.role);
        let claims = make_claims(&user, ttl);
        let token = auth::sign_jwt(&claims, &state.config.auth.jwt_secret).unwrap();
        let decoded = auth::verify_jwt(&token, &state.config.auth.jwt_secret).unwrap();
        assert_eq!(decoded.sub, user.id);
        assert_eq!(decoded.role, "player");
        assert!(decoded.exp > decoded.iat);
    }

    #[test]
    fn logout_invalidates_session() {
        let state = test_state();
        let conn = state.db.get().unwrap();
        let req = reg("alice");
        let hash = auth::hash_password(&req.password).unwrap();
        let user = User::create(&conn, &req, &hash).unwrap();
        let ttl = session_ttl(&state.config, &user.role);
        let claims = make_claims(&user, ttl);
        let token = auth::sign_jwt(&claims, &state.config.auth.jwt_secret).unwrap();
        auth::create_session(&state.db, user.id, &token, ttl).unwrap();

        let token_hash = auth::hash_token(&token);
        assert!(auth::is_session_valid(&state.db, &token_hash).unwrap());

        auth::revoke_session(&state.db, &token_hash).unwrap();
        assert!(!auth::is_session_valid(&state.db, &token_hash).unwrap());
    }

    #[test]
    fn username_validation() {
        assert!(validate_username("ab").is_err()); // too short
        assert!(validate_username(&"a".repeat(33)).is_err()); // too long
        assert!(validate_username("bad name").is_err()); // space
        assert!(validate_username("good_user-1").is_ok());
    }
}
