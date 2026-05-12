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

pub struct AppState;

pub use anticheat::AntiCheat;
pub use auth::Auth;
pub use cache::Cache;
pub use config::Config;
pub use database::{Database, DatabaseState, get_db_connection};
pub use db::DbPool;
pub use errors::{AppError, ErrorResponse};
