use crate::AppState;
use crate::handlers::admin::{dashboard, get_teams, get_users};
use crate::handlers::auth::{change_password, login, logout, me, register};
use crate::handlers::challenges::{get_challenge, list_challenges, submit_flag, unlock_hint};
use crate::handlers::scoreboard::{
    create_team, get_scoreboard, get_scoreboard_graph, get_team_profile, join_team,
};
use crate::handlers::ws::ws_handler;
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
        .route("/api/scoreboard/graph", get(get_scoreboard_graph))
        .route("/api/teams/{id}", get(get_team_profile))
        .route("/api/teams", post(create_team))
        .route("/api/teams/join", post(join_team))
        // Admin routes
        .route("/api/admin", get(dashboard))
        .route("/api/admin/users", get(get_users))
        .route("/api/admin/teams", get(get_teams))
        // WebSocket
        .route("/ws", get(ws_handler))
}
