// FeralCTF - WebSocket handler
// Stub implementation for Sprint 0
// Full implementation in Sprint 8 (WsHub, event broadcast, ping loop)

use crate::AppState;
use axum::{extract::State, http::StatusCode};

pub async fn ws_handler(State(_state): State<AppState>) -> StatusCode {
    StatusCode::NOT_IMPLEMENTED
}
