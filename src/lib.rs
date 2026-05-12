// FeralCTF - Library module
// Implements FERALCTF_SPEC.md section 4
#![allow(dead_code, unused_variables, unused_imports, unused_mut)]

pub mod anticheat;
pub mod auth;
pub mod cache;
pub mod config;
pub mod database;
pub mod db;
pub mod errors;
pub mod handlers;
pub mod import_export;
pub mod models;
pub mod routes;
pub mod scoring;
pub mod storage;

use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub db: db::DbPool,
    pub config: Arc<config::Config>,
    pub cache: Arc<cache::AppCache>,
    pub ws_hub: Arc<handlers::ws::WsHub>,
    pub rate_limiter: Arc<anticheat::RateLimiter>,
}

pub use cache::AppCache;
pub use config::Config;
pub use database::{Database, DatabaseState, get_db_connection};
pub use errors::{AppError, ErrorResponse};
pub use handlers::ws::{WsEvent, WsHub};
pub use models::scoreboard::ScoreboardState;
