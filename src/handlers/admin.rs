// FeralCTF - Admin handler module
// Implements FERALCTF_SPEC.md section 7.3

use crate::AppState;
use crate::errors::HandlerResult;
use axum::{
    extract::State,
    response::{IntoResponse, Json},
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct AdminResponse<T> {
    pub status: String,
    pub data: Option<T>,
}

impl<T: Serialize> IntoResponse for AdminResponse<T> {
    fn into_response(self) -> axum::response::Response {
        let json = Json(self);
        (axum::http::StatusCode::OK, json).into_response()
    }
}

pub async fn dashboard(State(_db_state): State<AppState>) -> HandlerResult<String> {
    Ok("Admin dashboard".to_string())
}

pub async fn get_users(State(_db_state): State<AppState>) -> HandlerResult<String> {
    Ok("Get all users".to_string())
}

pub async fn get_teams(State(_db_state): State<AppState>) -> HandlerResult<String> {
    Ok("Get all teams".to_string())
}

pub async fn reset_scores(State(_db_state): State<AppState>) -> HandlerResult<String> {
    Ok("Reset all scores".to_string())
}

pub async fn purge_old_solves(State(_db_state): State<AppState>) -> HandlerResult<String> {
    Ok("Purge old solves".to_string())
}

pub async fn update_challenge(
    State(_db_state): State<AppState>,
    challenge_id: i64,
    data: Json<ChallengeUpdate>,
) -> HandlerResult<String> {
    Ok("Challenge updated".to_string())
}

pub async fn ban_user(State(_db_state): State<AppState>, username: &str) -> HandlerResult<String> {
    Ok("User banned".to_string())
}

#[derive(Debug, Deserialize)]
pub struct ChallengeUpdate {
    pub description: Option<String>,
    pub category: Option<String>,
    pub points: Option<i64>,
    pub tags: Option<Vec<String>>,
}
