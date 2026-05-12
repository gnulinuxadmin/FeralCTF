// FeralCTF - User Model
// Implements FERALCTF_SPEC.md section 2

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub email: String,
    pub password_hash: String,
    pub role: String,
    pub team_id: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_login: Option<DateTime<Utc>>,
    pub is_active: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UserStats {
    pub total_points: i64,
    pub challenges_solved: i32,
    pub top_category: Option<String>,
    pub streak: i32,
    pub max_streak: i32,
    pub first_solved_at: Option<DateTime<Utc>>,
}
