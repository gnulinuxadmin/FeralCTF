// FeralCTF - Team Model
// Implements FERALCTF_SPEC.md section 2

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Team {
    pub id: i64,
    pub name: String,
    pub university: String,
    pub members: String,
    pub captain_name: String,
    pub email: String,
    pub captain_id: i64,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TeamMember {
    pub user_id: i64,
    pub username: String,
    pub role: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TeamStats {
    pub total_points: i64,
    pub challenges_solved: i32,
    pub rank: i32,
    pub streak: i32,
}
