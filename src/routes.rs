use crate::AppState;
use crate::handlers::admin::{dashboard, get_teams, get_users};
use crate::handlers::auth::{change_password, login, logout, me, register};
use crate::handlers::challenges::{get_challenge, list_challenges, submit_flag, unlock_hint};
use crate::handlers::scoreboard::get_scoreboard;
use axum::{
    Router,
    routing::{get, post, put},
};

pub fn create_router() -> Router<AppState> {
    Router::new()
        // Auth routes
        .route("/api/auth/register", post(register))
        .route("/api/auth/login", post(login))
        .route("/api/auth/logout", post(logout))
        .route("/api/auth/me", get(me))
        .route("/api/auth/password", put(change_password))
        // Challenge routes
        .route("/api/challenges", get(list_challenges))
        .route("/api/challenges/{id}", get(get_challenge))
        .route("/api/challenges/{id}/submit", post(submit_flag))
        .route(
            "/api/challenges/{challenge_id}/hints/{hint_id}/unlock",
            post(unlock_hint),
        )
        // Scoreboard routes
        .route("/api/scoreboard", get(get_scoreboard))
        // Admin routes
        .route("/api/admin", get(dashboard))
        .route("/api/admin/users", get(get_users))
        .route("/api/admin/teams", get(get_teams))
}
