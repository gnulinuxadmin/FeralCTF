// Competition lifecycle: combines the admin start/end/freeze controls stored
// in `competition_state` with `start_time`, `end_time` and
// `score_freeze_minutes_before_end` from config.toml.

use serde::Serialize;

use crate::{config::CompetitionConfig, db::DbConn, errors::AppError};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct StoredState {
    started_at: Option<i64>,
    ended_at: Option<i64>,
    frozen_at: Option<i64>,
}

/// Competition name and logo shown in the player UI.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Branding {
    pub name: String,
    pub logo_url: Option<String>,
}

impl Branding {
    /// `scheme://host[:port]` of the logo, for the CSP `img-src` directive.
    pub fn logo_origin(&self) -> Option<String> {
        let uri = self.logo_url.as_deref()?.parse::<axum::http::Uri>().ok()?;
        let scheme = uri.scheme_str()?;
        let host = uri.host()?;
        Some(match uri.port_u16() {
            Some(port) => format!("{scheme}://{host}:{port}"),
            None => format!("{scheme}://{host}"),
        })
    }
}

pub const MAX_NAME_LEN: usize = 64;

/// Admin overrides from the `branding` table, falling back to config.toml.
pub fn branding(conn: &DbConn, config: &CompetitionConfig) -> Result<Branding, AppError> {
    let stored = conn.query_row(
        "SELECT name, logo_url FROM branding WHERE id = 1",
        [],
        |row| {
            Ok((
                row.get::<_, Option<String>>(0)?,
                row.get::<_, Option<String>>(1)?,
            ))
        },
    );
    let (name, logo_url) = match stored {
        Ok(row) => row,
        Err(rusqlite::Error::QueryReturnedNoRows) => (None, None),
        Err(err) => return Err(AppError::Database(err)),
    };
    Ok(Branding {
        name: name
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| config.name.clone()),
        logo_url: logo_url.filter(|u| !u.trim().is_empty()),
    })
}

/// Store branding. `name: None` reverts to the config.toml name and
/// `logo_url: None` to the built-in logo. Callers validate first.
pub fn set_branding(
    conn: &DbConn,
    name: Option<&str>,
    logo_url: Option<&str>,
    now: i64,
) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO branding (id, name, logo_url, updated_at) VALUES (1, ?1, ?2, ?3)
         ON CONFLICT(id) DO UPDATE SET name = ?1, logo_url = ?2, updated_at = ?3",
        rusqlite::params![name, logo_url, now],
    )?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CompetitionStatus {
    pub name: String,
    pub logo_url: Option<String>,
    pub started: bool,
    pub ended: bool,
    /// When set and in the past, the public scoreboard shows scores as of
    /// this moment.
    pub frozen_at: Option<i64>,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
}

impl CompetitionStatus {
    pub fn is_frozen(&self, now: i64) -> bool {
        self.frozen_at.is_some_and(|at| at <= now)
    }

    /// Players may submit flags and unlock hints only while running.
    pub fn ensure_running(&self) -> Result<(), AppError> {
        if !self.started {
            return Err(AppError::BadRequest(
                "the competition has not started yet".into(),
            ));
        }
        if self.ended {
            return Err(AppError::BadRequest("the competition has ended".into()));
        }
        Ok(())
    }
}

fn parse_time(value: Option<&str>) -> Option<i64> {
    value
        .and_then(|v| chrono::DateTime::parse_from_rfc3339(v.trim()).ok())
        .map(|t| t.timestamp())
}

fn load(conn: &DbConn) -> Result<StoredState, AppError> {
    let result = conn.query_row(
        "SELECT started_at, ended_at, frozen_at FROM competition_state WHERE id = 1",
        [],
        |row| {
            Ok(StoredState {
                started_at: row.get(0)?,
                ended_at: row.get(1)?,
                frozen_at: row.get(2)?,
            })
        },
    );
    match result {
        Ok(state) => Ok(state),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(StoredState::default()),
        Err(err) => Err(AppError::Database(err)),
    }
}

fn resolve(
    stored: &StoredState,
    config: &CompetitionConfig,
    branding: &Branding,
    now: i64,
) -> CompetitionStatus {
    let config_start = parse_time(config.start_time.as_deref());
    let config_end = parse_time(config.end_time.as_deref());
    // An explicit admin start overrides the configured start time, and a
    // start after the configured end reopens the competition.
    let started = stored.started_at.is_some() || config_start.is_none_or(|start| start <= now);
    let config_end_applies =
        config_end.filter(|end| stored.started_at.is_none_or(|started| started < *end));
    let ended = stored.ended_at.is_some() || config_end_applies.is_some_and(|end| end <= now);
    let config_freeze = config_end_applies
        .filter(|_| config.score_freeze_minutes_before_end > 0)
        .map(|end| end - i64::from(config.score_freeze_minutes_before_end) * 60);
    let frozen_at = match (stored.frozen_at, config_freeze) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    };
    CompetitionStatus {
        name: branding.name.clone(),
        logo_url: branding.logo_url.clone(),
        started,
        ended,
        frozen_at,
        start_time: config.start_time.clone(),
        end_time: config.end_time.clone(),
    }
}

pub fn status(conn: &DbConn, config: &CompetitionConfig) -> Result<CompetitionStatus, AppError> {
    Ok(resolve(
        &load(conn)?,
        config,
        &branding(conn, config)?,
        chrono::Utc::now().timestamp(),
    ))
}

/// Admin "start": (re)opens the competition and lifts any freeze.
pub fn start(conn: &DbConn, now: i64) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO competition_state (id, started_at, ended_at, frozen_at)
         VALUES (1, ?1, NULL, NULL)
         ON CONFLICT(id) DO UPDATE SET started_at = ?1, ended_at = NULL, frozen_at = NULL",
        rusqlite::params![now],
    )?;
    Ok(())
}

pub fn end(conn: &DbConn, now: i64) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO competition_state (id, ended_at) VALUES (1, ?1)
         ON CONFLICT(id) DO UPDATE SET ended_at = ?1",
        rusqlite::params![now],
    )?;
    Ok(())
}

pub fn freeze(conn: &DbConn, now: i64) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO competition_state (id, frozen_at) VALUES (1, ?1)
         ON CONFLICT(id) DO UPDATE SET frozen_at = COALESCE(frozen_at, ?1)",
        rusqlite::params![now],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(start: Option<&str>, end: Option<&str>, freeze_minutes: u32) -> CompetitionConfig {
        CompetitionConfig {
            start_time: start.map(str::to_string),
            end_time: end.map(str::to_string),
            score_freeze_minutes_before_end: freeze_minutes,
            ..CompetitionConfig::default()
        }
    }

    fn plain() -> Branding {
        Branding {
            name: "FeralCTF".into(),
            logo_url: None,
        }
    }

    #[test]
    fn branding_falls_back_to_config_and_reports_logo_origin() {
        let pool = r2d2::Pool::new(r2d2_sqlite::SqliteConnectionManager::memory()).unwrap();
        let conn = pool.get().unwrap();
        crate::db::run_migrations(&conn).unwrap();
        let cfg = CompetitionConfig {
            name: "Config Cup".into(),
            ..CompetitionConfig::default()
        };
        let default = branding(&conn, &cfg).unwrap();
        assert_eq!(default.name, "Config Cup");
        assert_eq!(default.logo_url, None);
        assert_eq!(default.logo_origin(), None);

        set_branding(
            &conn,
            Some("Squirrel Games"),
            Some("https://cdn.example.org:8443/img/logo.png?v=2"),
            5,
        )
        .unwrap();
        let custom = branding(&conn, &cfg).unwrap();
        assert_eq!(custom.name, "Squirrel Games");
        assert_eq!(
            custom.logo_origin().as_deref(),
            Some("https://cdn.example.org:8443")
        );
        assert_eq!(status(&conn, &cfg).unwrap().name, "Squirrel Games");

        set_branding(&conn, None, None, 6).unwrap();
        assert_eq!(branding(&conn, &cfg).unwrap(), default);
    }

    const T0: &str = "2026-10-01T10:00:00Z";
    const T1: &str = "2026-10-01T18:00:00Z";

    fn ts(value: &str) -> i64 {
        parse_time(Some(value)).unwrap()
    }

    #[test]
    fn no_state_and_no_times_means_running() {
        let status = resolve(
            &StoredState::default(),
            &config(None, None, 0),
            &plain(),
            100,
        );
        assert!(status.started && !status.ended);
        assert_eq!(status.frozen_at, None);
        assert!(status.ensure_running().is_ok());
    }

    #[test]
    fn configured_window_controls_running() {
        let cfg = config(Some(T0), Some(T1), 30);
        let before = resolve(&StoredState::default(), &cfg, &plain(), ts(T0) - 1);
        assert!(!before.started);
        assert!(before.ensure_running().is_err());
        let during = resolve(&StoredState::default(), &cfg, &plain(), ts(T0) + 1);
        assert!(during.started && !during.ended);
        assert_eq!(during.frozen_at, Some(ts(T1) - 30 * 60));
        assert!(!during.is_frozen(ts(T0) + 1));
        assert!(during.is_frozen(ts(T1) - 60));
        let after = resolve(&StoredState::default(), &cfg, &plain(), ts(T1));
        assert!(after.ended);
    }

    #[test]
    fn admin_controls_override_config() {
        let cfg = config(Some(T0), Some(T1), 0);
        let early_start = StoredState {
            started_at: Some(ts(T0) - 3600),
            ..StoredState::default()
        };
        assert!(resolve(&early_start, &cfg, &plain(), ts(T0) - 60).started);

        let reopened = StoredState {
            started_at: Some(ts(T1) + 60),
            ..StoredState::default()
        };
        let status = resolve(&reopened, &cfg, &plain(), ts(T1) + 120);
        assert!(status.started && !status.ended);

        let ended = StoredState {
            ended_at: Some(5),
            ..StoredState::default()
        };
        assert!(resolve(&ended, &config(None, None, 0), &plain(), 10).ended);

        let frozen = StoredState {
            frozen_at: Some(50),
            ..StoredState::default()
        };
        let status = resolve(&frozen, &config(None, None, 0), &plain(), 60);
        assert!(status.is_frozen(60));
    }
}
