use axum::{
    routing::{get, post, put},
    Router,
};
use crate::AppState;
use crate::handlers::auth::{register, login, logout, me, change_password};
use crate::handlers::challenges::{list_challenges, get_challenge, solve_challenge};
use crate::handlers::scoreboard::get_scoreboard;
use crate::handlers::admin::{dashboard, get_users, get_teams};

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
        .route("/api/challenges/:id", get(get_challenge))
        .route("/api/challenges/:id/submit", post(solve_challenge))
        // Scoreboard routes
        .route("/api/scoreboard", get(get_scoreboard))
        // Admin routes
        .route("/api/admin", get(dashboard))
        .route("/api/admin/users", get(get_users))
        .route("/api/admin/teams", get(get_teams))
}
