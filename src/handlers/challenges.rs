// FeralCTF - Challenge handler module
// Implements FERALCTF_SPEC.md section 7.1

use crate::database::DatabaseState;
use crate::errors::HandlerResult;
use crate::models::challenge::{Challenge, Submission};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Json},
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct ChallengeResponse<T> {
    pub challenge: Option<T>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ChallengeListResponse<T> {
    pub challenges: Vec<T>,
    pub total: u64,
}

impl<T: Serialize> IntoResponse for ChallengeResponse<T> {
    fn into_response(self) -> axum::response::Response {
        let json = Json(self);
        (StatusCode::OK, json).into_response()
    }
}

impl<T: Serialize> IntoResponse for ChallengeListResponse<T> {
    fn into_response(self) -> axum::response::Response {
        let json = Json(self);
        (StatusCode::OK, json).into_response()
    }
}

pub async fn list_challenges(
    State(db_state): State<DatabaseState>,
    Query(params): Query<ChallengeFilters>,
) -> HandlerResult<ChallengeListResponse<Challenge>> {
    // Placeholder - implement actual query
    let challenges = vec![];
    let total = 0u64;
    Ok(ChallengeListResponse { challenges, total })
}

pub async fn get_challenge(
    State(db_state): State<DatabaseState>,
    Path(id): Path<i64>,
) -> HandlerResult<ChallengeResponse<Challenge>> {
    // Placeholder - implement actual query
    Ok(ChallengeResponse { challenge: None })
}

pub async fn solve_challenge(
    State(db_state): State<DatabaseState>,
    Path(id): Path<i64>,
    submission: Json<Submission>,
) -> HandlerResult<String> {
    Ok("Solved!".to_string())
}

pub async fn submit_flag(
    State(db_state): State<DatabaseState>,
    Path(id): Path<i64>,
    flag: String,
) -> HandlerResult<String> {
    Ok("Flag verified".to_string())
}

pub async fn get_challenge_stats(
    State(db_state): State<DatabaseState>,
    Path(id): Path<i64>,
) -> HandlerResult<ChallengeResponse<Challenge>> {
    // Placeholder
    Ok(ChallengeResponse { challenge: None })
}

pub async fn get_challenge_tags(
    State(db_state): State<DatabaseState>,
    Path(id): Path<i64>,
) -> HandlerResult<Vec<String>> {
    // Placeholder
    Ok(vec![])
}

pub async fn get_challenge_category(
    State(db_state): State<DatabaseState>,
    Path(id): Path<i64>,
) -> HandlerResult<String> {
    // Placeholder
    Ok("General".to_string())
}

pub async fn get_challenge_difficulty(
    State(db_state): State<DatabaseState>,
    Path(id): Path<i64>,
) -> HandlerResult<String> {
    // Placeholder
    Ok("Medium".to_string())
}

#[derive(Debug, Deserialize)]
pub struct ChallengeFilters {
    pub category: Option<String>,
    pub difficulty: Option<String>,
    pub tag: Option<String>,
    pub search: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ChallengePreview {
    pub id: i64,
    pub title: String,
    pub category: String,
    pub points: i64,
    pub difficulty: String,
    pub tags: Vec<String>,
    pub solved_count: u64,
    pub is_active: bool,
}
