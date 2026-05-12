use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

use dashmap::DashMap;

use crate::{config::RateLimitConfig, db::DbConn, errors::AppError};

const SUBMISSION_WINDOW: Duration = Duration::from_secs(60);
const WRONG_ATTEMPT_TTL: Duration = Duration::from_secs(60 * 60);

pub struct RateLimiter {
    team_windows: DashMap<i64, VecDeque<Instant>>,
    wrong_attempts: DashMap<(i64, i64), (u32, Instant)>,
}

impl RateLimiter {
    pub fn new() -> Self {
        Self {
            team_windows: DashMap::new(),
            wrong_attempts: DashMap::new(),
        }
    }

    pub fn check_submission(
        &self,
        team_id: i64,
        challenge_id: i64,
        config: &RateLimitConfig,
    ) -> Result<(), AppError> {
        let now = Instant::now();

        if let Some(mut window) = self.team_windows.get_mut(&team_id) {
            expire_window(&mut window, now);
            if window.len() >= config.submissions_per_minute as usize {
                let retry_after = window
                    .front()
                    .map(|oldest| {
                        retry_after_seconds(SUBMISSION_WINDOW, now.duration_since(*oldest))
                    })
                    .unwrap_or(1);
                return Err(AppError::RateLimited {
                    retry_after_seconds: retry_after,
                });
            }
        }

        if let Some(attempts) = self.wrong_attempts.get(&(team_id, challenge_id)) {
            let (count, last_attempt) = *attempts;
            if count >= config.wrong_attempts_before_backoff {
                let exponent = count.saturating_sub(config.wrong_attempts_before_backoff);
                let wait = backoff_wait(config.backoff_base_seconds, exponent);
                let elapsed = now.duration_since(last_attempt);
                if elapsed < wait {
                    return Err(AppError::RateLimited {
                        retry_after_seconds: retry_after_seconds(wait, elapsed),
                    });
                }
            }
        }

        Ok(())
    }

    pub fn record_attempt(&self, team_id: i64, challenge_id: i64, correct: bool) {
        let now = Instant::now();
        let mut window = self.team_windows.entry(team_id).or_default();
        expire_window(&mut window, now);
        window.push_back(now);
        drop(window);

        if correct {
            self.wrong_attempts.remove(&(team_id, challenge_id));
            return;
        }

        self.wrong_attempts
            .entry((team_id, challenge_id))
            .and_modify(|(count, last_attempt)| {
                *count = count.saturating_add(1);
                *last_attempt = now;
            })
            .or_insert((1, now));
    }

    pub fn gc(&self) {
        let now = Instant::now();
        self.team_windows.retain(|_, window| {
            expire_window(window, now);
            !window.is_empty()
        });
        self.wrong_attempts
            .retain(|_, (_, last_attempt)| now.duration_since(*last_attempt) <= WRONG_ATTEMPT_TTL);
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

pub fn spawn_rate_limiter_gc_task(
    limiter: std::sync::Arc<RateLimiter>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(SUBMISSION_WINDOW);
        loop {
            interval.tick().await;
            limiter.gc();
        }
    })
}

pub fn check_flag_sharing(
    conn: &DbConn,
    challenge_id: i64,
    team_id: i64,
    flag: &str,
    window_seconds: u64,
) -> Result<bool, AppError> {
    let since = chrono::Utc::now().timestamp() - window_seconds as i64;
    let shared = conn.query_row(
        "SELECT EXISTS(
            SELECT 1
            FROM submissions
            WHERE challenge_id = ?1
              AND team_id != ?2
              AND flag = ?3
              AND is_correct = 1
              AND submitted_at >= ?4
            LIMIT 1
        )",
        rusqlite::params![challenge_id, team_id, flag, since],
        |row| row.get::<_, i64>(0),
    )? != 0;

    if shared {
        tracing::warn!(
            challenge_id,
            team_id,
            "possible flag sharing detected: same correct flag submitted by another team"
        );
    }

    Ok(shared)
}

fn expire_window(window: &mut VecDeque<Instant>, now: Instant) {
    while window
        .front()
        .map(|instant| now.duration_since(*instant) >= SUBMISSION_WINDOW)
        .unwrap_or(false)
    {
        window.pop_front();
    }
}

fn retry_after_seconds(wait: Duration, elapsed: Duration) -> u64 {
    wait.saturating_sub(elapsed).as_secs().max(1)
}

fn backoff_wait(base_seconds: u64, exponent: u32) -> Duration {
    let multiplier = if exponent >= 63 {
        u64::MAX
    } else {
        1u64 << exponent
    };
    Duration::from_secs(base_seconds.saturating_mul(multiplier))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{auth, db};
    use r2d2::Pool;
    use r2d2_sqlite::SqliteConnectionManager;

    fn test_config() -> RateLimitConfig {
        RateLimitConfig {
            submissions_per_minute: 10,
            wrong_attempts_before_backoff: 2,
            backoff_base_seconds: 30,
            flag_sharing_window_seconds: 300,
        }
    }

    #[test]
    fn eleventh_submission_in_window_is_rate_limited() {
        let limiter = RateLimiter::new();
        let config = test_config();

        for challenge_id in 0..10 {
            limiter.check_submission(1, challenge_id, &config).unwrap();
            limiter.record_attempt(1, challenge_id, false);
        }

        let err = limiter.check_submission(1, 11, &config).unwrap_err();
        match err {
            AppError::RateLimited {
                retry_after_seconds,
            } => assert!(retry_after_seconds > 0),
            other => panic!("expected rate limited error, got {other:?}"),
        }
    }

    #[test]
    fn wrong_attempt_backoff_doubles_after_threshold() {
        let limiter = RateLimiter::new();
        let config = test_config();

        limiter.record_attempt(1, 1, false);
        assert!(limiter.check_submission(1, 1, &config).is_ok());

        limiter.record_attempt(1, 1, false);
        let err = limiter.check_submission(1, 1, &config).unwrap_err();
        let first_retry = match err {
            AppError::RateLimited {
                retry_after_seconds,
            } => retry_after_seconds,
            other => panic!("expected rate limited error, got {other:?}"),
        };
        assert!(first_retry <= 30);

        limiter.record_attempt(1, 1, false);
        let err = limiter.check_submission(1, 1, &config).unwrap_err();
        let second_retry = match err {
            AppError::RateLimited {
                retry_after_seconds,
            } => retry_after_seconds,
            other => panic!("expected rate limited error, got {other:?}"),
        };
        assert!(second_retry > first_retry);
        assert!(second_retry <= 60);
    }

    #[test]
    fn correct_attempt_clears_wrong_attempt_backoff() {
        let limiter = RateLimiter::new();
        let config = test_config();

        limiter.record_attempt(1, 1, false);
        limiter.record_attempt(1, 1, false);
        assert!(limiter.check_submission(1, 1, &config).is_err());

        limiter.record_attempt(1, 1, true);
        assert!(limiter.check_submission(1, 1, &config).is_ok());
    }

    #[test]
    fn flag_sharing_detects_same_correct_flag_inside_window() {
        let pool = Pool::builder()
            .max_size(1)
            .build(SqliteConnectionManager::memory())
            .unwrap();
        let conn = pool.get().unwrap();
        db::run_migrations(&conn).unwrap();
        let now = chrono::Utc::now().timestamp();

        conn.execute(
            "INSERT INTO submissions
                (team_id, user_id, challenge_id, flag, is_correct, ip_address, submitted_at)
             VALUES (?1, ?2, ?3, ?4, 1, '127.0.0.1', ?5)",
            rusqlite::params![2, 2, 1, "flag{shared}", now],
        )
        .unwrap();

        assert!(check_flag_sharing(&conn, 1, 1, "flag{shared}", 60).unwrap());
        assert!(!check_flag_sharing(&conn, 1, 1, "flag{other}", 60).unwrap());

        let salt = "salt";
        let hashed = auth::hash_flag("flag{not-raw}", salt);
        assert!(!check_flag_sharing(&conn, 1, 1, &hashed, 60).unwrap());
    }
}
