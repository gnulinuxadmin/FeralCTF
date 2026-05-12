// FeralCTF - Scoreboard handler module
// Implements FERALCTF_SPEC.md section 7.5

use crate::database::DatabaseState;
use crate::errors::HandlerResult;
use crate::models::scoreboard::ScoreboardState;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Json},
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct ScoreboardResponse<T> {
    pub entries: Vec<T>,
}

impl<T: Serialize> IntoResponse for ScoreboardResponse<T> {
    fn into_response(self) -> axum::response::Response {
        let json = Json(self);
        (StatusCode::OK, json).into_response()
    }
}

pub async fn get_scoreboard(
    State(_db_state): State<DatabaseState>,
    Query(_params): Query<ScoreboardFilters>,
) -> HandlerResult<Json<ScoreboardState>> {
    // Placeholder
    Ok(Json(ScoreboardState {
        teams: vec![],
        generated_at: 0,
    }))
}

#[derive(Debug, Deserialize)]
pub struct ScoreboardFilters {
    pub top: Option<u64>,
    pub page: Option<u64>,
    pub per_page: Option<u64>,
    pub category: Option<String>,
    pub tag: Option<String>,
}
