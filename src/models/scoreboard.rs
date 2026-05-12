// FeralCTF - Scoreboard Model
// Implements FERALCTF_SPEC.md section 6

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct ScoreboardEntry {
    pub team_id: i64,
    pub team_name: String,
    pub rank: i32,
    pub total_points: i64,
    pub challenges_solved: i32,
    pub pending_flags: i32,
    pub progress_percentage: f64,
    pub last_submission_at: DateTime<Utc>,
    pub global_rank: i32,
    pub local_rank: i32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LeaderboardEntry {
    pub rank: i32,
    pub team_name: String,
    pub score: i64,
    pub points_breakdown: Vec<PointBreakdown>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PointBreakdown {
    pub category: String,
    pub points: i64,
    pub solved: i32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TeamComparison {
    pub team_id: i64,
    pub team_name: String,
    pub stats: TeamStats,
    pub rank_change: i32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TeamStats {
    pub total_points: i64,
    pub total_submissions: i64,
    pub avg_solve_time_ms: f64,
    pub best_category: Option<String>,
    pub streak: i32,
}

/// Public scoreboard type exposed to handlers
pub type Scoreboard = Vec<ScoreboardEntry>;
