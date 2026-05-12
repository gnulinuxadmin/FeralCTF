// FeralCTF - Handlers module
// Implements FERALCTF_SPEC.md section 6

/// Handler response type alias
pub type HandlerResponse = crate::errors::HandlerResult<String>;

// Admin handlers
pub mod admin;

// Auth handlers
pub mod auth;

// Challenge handlers
pub mod challenges;

// Scoreboard handlers
pub mod scoreboard;

// WebSocket hub
pub mod ws;