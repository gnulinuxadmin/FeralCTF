// FeralCTF - Challenge Model
// Implements FERALCTF_SPEC.md section 2

use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

#[derive(Debug, Serialize, Deserialize)]
pub struct Challenge {
    pub id: i64,
    pub title: String,
    pub description: String,
    pub category: String,
    pub points: i64,
    pub is_solved: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub is_active: bool,
    pub author_id: i64,
    pub hint: Option<String>,
    pub flag: String,
    pub max_attempts: Option<i32>,
    pub dynamic_scoring: bool,
    pub difficulty: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ChallengeSubmission {
    pub id: i64,
    pub challenge_id: i64,
    pub team_id: i64,
    pub user_id: Option<i64>,
    pub solution: String,
    pub is_valid: bool,
    pub points_earned: i64,
    pub submitted_at: DateTime<Utc>,
    pub review_status: String,
    pub reviewer_notes: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ChallengeStat {
    pub total_submissions: i64,
    pub valid_submissions: i64,
    pub avg_solve_time: f64,
    pub median_solve_time: f64,
    pub solve_rate: f64,
    pub first_solve_time: Option<DateTime<Utc>>,
}