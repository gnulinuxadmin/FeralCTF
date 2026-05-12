use dashmap::DashMap;
use std::sync::{Arc, RwLock};

use crate::{
    db::DbConn,
    errors::AppError,
    models::{challenge::Challenge, scoreboard::ScoreboardState},
};

#[derive(Clone)]
pub struct AppCache {
    pub scoreboard: Arc<RwLock<Option<ScoreboardState>>>,
    pub challenges: Arc<RwLock<Option<Vec<Challenge>>>>,
    pub sessions: Arc<DashMap<String, String>>,
}

impl Default for AppCache {
    fn default() -> Self {
        Self::new()
    }
}

impl AppCache {
    pub fn new() -> Self {
        Self {
            scoreboard: Arc::new(RwLock::new(None)),
            challenges: Arc::new(RwLock::new(None)),
            sessions: Arc::new(DashMap::new()),
        }
    }

    pub fn invalidate_scoreboard(&self) {
        if let Ok(mut scoreboard) = self.scoreboard.write() {
            *scoreboard = None;
        }
    }

    pub fn invalidate_challenges(&self) {
        if let Ok(mut challenges) = self.challenges.write() {
            *challenges = None;
        }
    }

    pub fn get_or_build_scoreboard(&self, conn: &DbConn) -> Result<ScoreboardState, AppError> {
        if let Some(scoreboard) = self
            .scoreboard
            .read()
            .map_err(|_| anyhow::anyhow!("scoreboard cache lock poisoned"))?
            .clone()
        {
            return Ok(scoreboard);
        }

        let scoreboard = ScoreboardState::build(conn)?;
        *self
            .scoreboard
            .write()
            .map_err(|_| anyhow::anyhow!("scoreboard cache lock poisoned"))? =
            Some(scoreboard.clone());
        Ok(scoreboard)
    }

    pub fn get_or_build_challenges(&self, conn: &DbConn) -> Result<Vec<Challenge>, AppError> {
        if let Some(challenges) = self
            .challenges
            .read()
            .map_err(|_| anyhow::anyhow!("challenge cache lock poisoned"))?
            .clone()
        {
            return Ok(challenges);
        }

        let challenges = Challenge::list_visible(conn)?;
        *self
            .challenges
            .write()
            .map_err(|_| anyhow::anyhow!("challenge cache lock poisoned"))? =
            Some(challenges.clone());
        Ok(challenges)
    }

    pub fn is_scoreboard_cached(&self) -> bool {
        self.scoreboard
            .read()
            .map(|scoreboard| scoreboard.is_some())
            .unwrap_or(false)
    }

    pub fn is_challenges_cached(&self) -> bool {
        self.challenges
            .read()
            .map(|challenges| challenges.is_some())
            .unwrap_or(false)
    }

    pub fn is_session_active(&self, token_hash: &str) -> bool {
        self.sessions.contains_key(token_hash)
    }

    pub fn add_session(&self, token_hash: &str, user_id: String) {
        self.sessions.insert(token_hash.to_string(), user_id);
    }

    pub fn revoke_session(&self, token_hash: &str) {
        self.sessions.remove(token_hash);
    }

    pub fn get_session(&self, token_hash: &str) -> Option<String> {
        self.sessions
            .get(token_hash)
            .map(|entry| entry.value().clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use r2d2::Pool;
    use r2d2_sqlite::SqliteConnectionManager;

    fn test_pool() -> crate::db::DbPool {
        let pool = Pool::new(SqliteConnectionManager::memory()).unwrap();
        {
            let conn = pool.get().unwrap();
            crate::db::run_migrations(&conn).unwrap();
        }
        pool
    }

    #[test]
    fn cache_sessions_still_work() {
        let cache = AppCache::new();
        cache.add_session("token123", "1".to_string());
        assert!(cache.is_session_active("token123"));
        assert_eq!(cache.get_session("token123"), Some("1".to_string()));
        cache.revoke_session("token123");
        assert!(!cache.is_session_active("token123"));
    }

    #[test]
    fn get_or_build_scoreboard_caches_until_invalidated() {
        let pool = test_pool();
        let conn = pool.get().unwrap();
        conn.execute(
            "INSERT INTO teams (id, name, invite_code, score) VALUES (1, 'A', 'ABCDEFGH', 10)",
            [],
        )
        .unwrap();

        let cache = AppCache::new();
        assert!(!cache.is_scoreboard_cached());
        let scoreboard = cache.get_or_build_scoreboard(&conn).unwrap();
        assert_eq!(scoreboard.teams.len(), 1);
        assert!(cache.is_scoreboard_cached());
        cache.invalidate_scoreboard();
        assert!(!cache.is_scoreboard_cached());
    }

    #[test]
    fn get_or_build_challenges_caches_visible_challenges() {
        let pool = test_pool();
        let conn = pool.get().unwrap();
        conn.execute(
            "INSERT INTO challenges (
                slug, title, description, category, flag_hash, flag_salt, flag_type,
                flag_case_sensitive, points, max_points, min_points, decay_rate, is_hidden, created_at
             )
             VALUES ('visible', 'Visible', 'desc', 'web', 'hash', 'salt', 'static',
                0, 100, 500, 50, 12, 0, 1)",
            [],
        )
        .unwrap();

        let cache = AppCache::new();
        let challenges = cache.get_or_build_challenges(&conn).unwrap();
        assert_eq!(challenges.len(), 1);
        assert!(cache.is_challenges_cached());
        cache.invalidate_challenges();
        assert!(!cache.is_challenges_cached());
    }
}
