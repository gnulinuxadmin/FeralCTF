//! Cache module for in-memory state management
//!
//! Provides session management, token revocation, and cached state operations.

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};

use crate::errors::AppError;

/// In-memory scoreboard cache (for rate limiting, etc.)
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ScoreboardState {
    pub challenges: Vec<ChallengeInfo>,
    pub submissions: Vec<SubmissionInfo>,
    pub flags: Vec<FlagInfo>,
    pub solved_count: u32,
    pub total_score: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChallengeInfo {
    pub id: i32,
    pub title: String,
    pub category: String,
    pub points: u32,
    pub solved: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmissionInfo {
    pub team_name: String,
    pub challenge_id: i32,
    pub solved_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlagInfo {
    pub challenge_id: i32,
    submitted_by: String,
    pub score: u32,
}

/// Websocket hub for real-time notifications (placeholder)
pub struct WsHub {
    pub sender: tokio::sync::broadcast::Sender<Vec<String>>,
    pub subscribers: usize,
}

impl WsHub {
    pub fn new() -> Self {
        let (sender, _receiver) = tokio::sync::broadcast::channel::<Vec<String>>(1000);
        Self {
            sender,
            subscribers: 0,
        }
    }

    pub async fn publish(&self, messages: Vec<String>) -> std::result::Result<(), AppError> {
        let _ = self.sender.send(messages);
        Ok(())
    }

    pub fn subscriber_count(&self) -> usize {
        self.subscribers
    }
}

impl Default for WsHub {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for WsHub {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WsHub")
            .field("sender", &"...")
            .field("subscribers", &self.subscribers)
            .finish()
    }
}

/// Application-level cache for state management
#[derive(Clone)]
pub struct AppCache {
    pub scoreboard: Arc<RwLock<ScoreboardState>>,
    pub challenges: Arc<RwLock<Vec<ChallengeInfo>>>,
    pub sessions: Arc<DashMap<String, String>>,
    pub ws_hub: Arc<WsHub>,
}

impl Default for AppCache {
    fn default() -> Self {
        Self::new()
    }
}

impl AppCache {
    /// Create a new cache with default channels
    pub fn new() -> Self {
        let (ws_sender, _) = tokio::sync::broadcast::channel::<Vec<String>>(1000);
        Self {
            scoreboard: Arc::new(RwLock::new(ScoreboardState {
                challenges: Vec::new(),
                submissions: Vec::new(),
                flags: Vec::new(),
                solved_count: 0,
                total_score: 0,
            })),
            challenges: Arc::new(RwLock::new(Vec::new())),
            sessions: Arc::new(DashMap::new()),
            ws_hub: Arc::new(WsHub {
                sender: ws_sender,
                subscribers: 0,
            }),
        }
    }

    /// Check if a session exists (simplified check)
    pub fn is_session_active(&self, token_hash: &str) -> bool {
        self.sessions.contains_key(token_hash)
    }

    /// Add a session (simplified)
    pub fn add_session(&self, token_hash: &str, user_id: String) {
        self.sessions.insert(token_hash.to_string(), user_id);
    }

    /// Revoke a session
    pub fn revoke_session(&self, token_hash: &str) {
        self.sessions.remove(token_hash);
    }

    /// Get user_id by token hash
    pub fn get_session(&self, token_hash: &str) -> Option<String> {
        self.sessions
            .get(token_hash)
            .map(|entry| entry.value().clone())
    }

    /// Increment scoreboard solved count atomically
    pub async fn increment_solved(&self) -> u32 {
        let mut state = self.scoreboard.write().expect("scoreboard lock poisoned");
        state.solved_count += 1;
        state.total_score =
            state.solved_count * state.challenges.first().map(|c| c.points).unwrap_or(0);
        state.solved_count
    }

    /// Add a challenge to the cache
    pub fn add_challenge(&self, challenge: ChallengeInfo) {
        let mut challenges = self.challenges.write().unwrap();
        challenges.push(challenge);
    }

    /// Get all challenges
    pub fn get_all_challenges(&self) -> Vec<ChallengeInfo> {
        let challenges = self.challenges.read().unwrap();
        challenges.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_app_cache() {
        let cache = AppCache::new();
        cache.add_session("token123", "1".to_string());
        assert!(cache.is_session_active("token123"));
        assert_eq!(cache.get_session("token123"), Some("1".to_string()));
        cache.revoke_session("token123");
        assert!(!cache.is_session_active("token123"));
    }

    #[tokio::test]
    async fn test_increment_solved() {
        let cache = AppCache::new();
        let challenges = vec![ChallengeInfo {
            id: 1,
            title: "Test".to_string(),
            category: "Crypto".to_string(),
            points: 100,
            solved: 0,
        }];
        cache.scoreboard.write().unwrap().challenges = challenges;
        let count = cache.increment_solved().await;
        assert_eq!(count, 1);
    }
}
