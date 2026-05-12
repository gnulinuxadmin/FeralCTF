// FeralCTF - Authentication handler module
// Implements FERALCTF_SPEC.md section 7.2

use axum::{
    extract::State,
    response::{IntoResponse, Json},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use crate::database::DatabaseState;
use crate::errors::HandlerResult;

#[derive(Debug, Serialize, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LoginResponse {
    pub token: String,
    pub user: UserResponse,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UserResponse {
    pub username: String,
    pub rank: String,
}

impl IntoResponse for LoginResponse {
    fn into_response(self) -> axum::response::Response {
        let json = Json(self);
        (StatusCode::OK, json).into_response()
    }
}

pub async fn login(State(db_state): State<DatabaseState>, credentials: Json<LoginRequest>) -> HandlerResult<LoginResponse> {
    // Placeholder - implement actual auth logic
    Ok(LoginResponse {
        token: "placeholder_token".to_string(),
        user: UserResponse {
            username: "admin".to_string(),
            rank: "Admin".to_string(),
        },
    })
}

pub async fn register(State(db_state): State<DatabaseState>, data: Json<RegisterRequest>) -> HandlerResult<String> {
    Ok("Registration successful".to_string())
}

pub async fn logout() -> HandlerResult<String> {
    Ok("Logged out".to_string())
}

pub async fn get_profile(State(db_state): State<DatabaseState>) -> HandlerResult<String> {
    Ok("Get profile".to_string())
}

#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    pub username: String,
    pub password: String,
    pub team_name: String,
}