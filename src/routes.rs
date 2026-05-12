use crate::AppState;
use crate::handlers::admin::{
    announce, backup, ban_user, competition_end, competition_freeze, competition_start,
    create_challenge, dashboard, delete_challenge, disqualify_team, export_bundle, get_teams,
    get_users, import_bundle, list_submissions, require_admin, update_challenge,
};
use crate::handlers::auth::{change_password, login, logout, me, register};
use crate::handlers::challenges::{get_challenge, list_challenges, submit_flag, unlock_hint};
use crate::handlers::scoreboard::{
    create_team, get_scoreboard, get_scoreboard_graph, get_team_profile, join_team,
};
use crate::handlers::ws::ws_handler;
use axum::{
    Router,
    body::Body,
    http::{StatusCode, Uri, header},
    middleware,
    response::{IntoResponse, Response},
    routing::{get, post, put},
};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "frontend/"]
struct FrontendAssets;

/// Build the application router. Takes ownership of AppState so the admin
/// middleware can be baked in via `from_fn_with_state`.
pub fn create_router(state: AppState) -> Router {
    let admin_router = Router::new()
        .route("/api/admin", get(dashboard))
        .route("/api/admin/challenges", post(create_challenge))
        .route(
            "/api/admin/challenges/{id}",
            put(update_challenge).delete(delete_challenge),
        )
        .route("/api/admin/submissions", get(list_submissions))
        .route("/api/admin/users", get(get_users))
        .route("/api/admin/users/{id}/ban", post(ban_user))
        .route("/api/admin/teams", get(get_teams))
        .route("/api/admin/teams/{id}/disqualify", post(disqualify_team))
        .route("/api/admin/competition/start", post(competition_start))
        .route("/api/admin/competition/end", post(competition_end))
        .route("/api/admin/competition/freeze", post(competition_freeze))
        .route("/api/admin/announce", post(announce))
        .route("/api/admin/export", get(export_bundle))
        .route("/api/admin/import", post(import_bundle))
        .route("/api/admin/backup", get(backup))
        .route_layer(middleware::from_fn_with_state(state.clone(), require_admin));

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
        // WebSocket
        .route("/ws", get(ws_handler))
        // Admin (require_admin middleware applied inside admin_router)
        .merge(admin_router)
        .fallback(frontend)
        .with_state(state)
}

async fn frontend(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    if path.starts_with("api/") || path == "ws" {
        return StatusCode::NOT_FOUND.into_response();
    }

    let asset_path = if path.is_empty() { "index.html" } else { path };
    asset_response(asset_path).unwrap_or_else(|| {
        asset_response("index.html").unwrap_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "embedded frontend index.html is missing",
            )
                .into_response()
        })
    })
}

fn asset_response(path: &str) -> Option<Response> {
    let asset = FrontendAssets::get(path)?;
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    match Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime.as_ref())
        .body(Body::from(asset.data.into_owned()))
    {
        Ok(response) => Some(response),
        Err(_) => Some(StatusCode::INTERNAL_SERVER_ERROR.into_response()),
    }
}
