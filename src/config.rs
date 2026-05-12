use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;
use std::{env, fs, path::Path};

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Config {
    pub server: ServerConfig,
    pub competition: CompetitionConfig,
    pub database: DatabaseConfig,
    pub auth: AuthConfig,
    pub storage: StorageConfig,
    pub rate_limit: RateLimitConfig,
    pub notifications: NotificationsConfig,
    pub logging: LoggingConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct ServerConfig {
    pub port: u16,
    pub host: String,
    pub base_url: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            port: 8080,
            host: "0.0.0.0".to_string(),
            base_url: "http://localhost:8080".to_string(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct CompetitionConfig {
    pub name: String,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
    pub team_mode: bool,
    pub max_team_size: u32,
    pub registration_open: bool,
    pub dynamic_scoring: bool,
    pub score_freeze_minutes_before_end: u32,
}

impl Default for CompetitionConfig {
    fn default() -> Self {
        Self {
            name: "FeralCTF".to_string(),
            start_time: None,
            end_time: None,
            team_mode: true,
            max_team_size: 4,
            registration_open: true,
            dynamic_scoring: true,
            score_freeze_minutes_before_end: 0,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct DatabaseConfig {
    pub path: String,
    pub backend: String,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            path: "./ctf.db".to_string(),
            backend: "sqlite".to_string(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct AuthConfig {
    pub jwt_secret: String,
    pub session_ttl_hours: u64,
    pub admin_session_ttl_hours: u64,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            jwt_secret: String::new(),
            session_ttl_hours: 24,
            admin_session_ttl_hours: 4,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct StorageConfig {
    pub attachments_path: String,
    pub max_file_size_mb: u64,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            attachments_path: "./attachments".to_string(),
            max_file_size_mb: 100,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct RateLimitConfig {
    pub submissions_per_minute: u32,
    pub wrong_attempts_before_backoff: u32,
    pub backoff_base_seconds: u64,
    pub flag_sharing_window_seconds: u64,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            submissions_per_minute: 10,
            wrong_attempts_before_backoff: 5,
            backoff_base_seconds: 30,
            flag_sharing_window_seconds: 300,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct NotificationsConfig {
    pub discord_webhook_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct LoggingConfig {
    pub level: String,
    pub format: String,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: "info".to_string(),
            format: "json".to_string(),
        }
    }
}

pub fn load(path: &str) -> Result<Config, anyhow::Error> {
    let mut config = if Path::new(path).exists() {
        let content = fs::read_to_string(path)?;
        toml::from_str::<Config>(&content)?
    } else {
        Config::default()
    };

    apply_env_overrides(&mut config)?;

    if config.auth.jwt_secret.trim().is_empty() {
        config.auth.jwt_secret = generate_secret();
    }

    validate(&config)?;
    fs::create_dir_all(&config.storage.attachments_path)?;

    Ok(config)
}

pub fn generate_example(path: &str) -> Result<(), anyhow::Error> {
    fs::write(path, EXAMPLE_CONFIG)?;
    Ok(())
}

fn apply_env_overrides(config: &mut Config) -> Result<(), anyhow::Error> {
    set_from_env(&mut config.server.port, "FERALCTF_SERVER_PORT")?;
    set_string_from_env(&mut config.server.host, "FERALCTF_SERVER_HOST");
    set_string_from_env(&mut config.server.base_url, "FERALCTF_SERVER_BASE_URL");

    set_string_from_env(&mut config.competition.name, "FERALCTF_COMPETITION_NAME");
    set_optional_string_from_env(
        &mut config.competition.start_time,
        "FERALCTF_COMPETITION_START_TIME",
    );
    set_optional_string_from_env(
        &mut config.competition.end_time,
        "FERALCTF_COMPETITION_END_TIME",
    );
    set_from_env(
        &mut config.competition.team_mode,
        "FERALCTF_COMPETITION_TEAM_MODE",
    )?;
    set_from_env(
        &mut config.competition.max_team_size,
        "FERALCTF_COMPETITION_MAX_TEAM_SIZE",
    )?;
    set_from_env(
        &mut config.competition.registration_open,
        "FERALCTF_COMPETITION_REGISTRATION_OPEN",
    )?;
    set_from_env(
        &mut config.competition.dynamic_scoring,
        "FERALCTF_COMPETITION_DYNAMIC_SCORING",
    )?;
    set_from_env(
        &mut config.competition.score_freeze_minutes_before_end,
        "FERALCTF_COMPETITION_SCORE_FREEZE_MINUTES_BEFORE_END",
    )?;

    set_string_from_env(&mut config.database.path, "FERALCTF_DATABASE_PATH");
    set_string_from_env(&mut config.database.backend, "FERALCTF_DATABASE_BACKEND");

    set_string_from_env(&mut config.auth.jwt_secret, "FERALCTF_AUTH_JWT_SECRET");
    set_from_env(
        &mut config.auth.session_ttl_hours,
        "FERALCTF_AUTH_SESSION_TTL_HOURS",
    )?;
    set_from_env(
        &mut config.auth.admin_session_ttl_hours,
        "FERALCTF_AUTH_ADMIN_SESSION_TTL_HOURS",
    )?;

    set_string_from_env(
        &mut config.storage.attachments_path,
        "FERALCTF_STORAGE_ATTACHMENTS_PATH",
    );
    set_from_env(
        &mut config.storage.max_file_size_mb,
        "FERALCTF_STORAGE_MAX_FILE_SIZE_MB",
    )?;

    set_from_env(
        &mut config.rate_limit.submissions_per_minute,
        "FERALCTF_RATE_LIMIT_SUBMISSIONS_PER_MINUTE",
    )?;
    set_from_env(
        &mut config.rate_limit.wrong_attempts_before_backoff,
        "FERALCTF_RATE_LIMIT_WRONG_ATTEMPTS_BEFORE_BACKOFF",
    )?;
    set_from_env(
        &mut config.rate_limit.backoff_base_seconds,
        "FERALCTF_RATE_LIMIT_BACKOFF_BASE_SECONDS",
    )?;
    set_from_env(
        &mut config.rate_limit.flag_sharing_window_seconds,
        "FERALCTF_RATE_LIMIT_FLAG_SHARING_WINDOW_SECONDS",
    )?;

    set_optional_string_from_env(
        &mut config.notifications.discord_webhook_url,
        "FERALCTF_NOTIFICATIONS_DISCORD_WEBHOOK_URL",
    );

    set_string_from_env(&mut config.logging.level, "FERALCTF_LOGGING_LEVEL");
    set_string_from_env(&mut config.logging.format, "FERALCTF_LOGGING_FORMAT");

    Ok(())
}

fn set_from_env<T>(target: &mut T, key: &str) -> Result<(), anyhow::Error>
where
    T: std::str::FromStr,
    T::Err: std::error::Error + Send + Sync + 'static,
{
    if let Ok(value) = env::var(key) {
        *target = value.parse::<T>()?;
    }
    Ok(())
}

fn set_string_from_env(target: &mut String, key: &str) {
    if let Ok(value) = env::var(key) {
        *target = value;
    }
}

fn set_optional_string_from_env(target: &mut Option<String>, key: &str) {
    if let Ok(value) = env::var(key) {
        *target = if value.trim().is_empty() {
            None
        } else {
            Some(value)
        };
    }
}

fn validate(config: &Config) -> Result<(), anyhow::Error> {
    if config.server.port == 0 {
        anyhow::bail!("server.port must be greater than 0");
    }
    if config.competition.max_team_size == 0 {
        anyhow::bail!("competition.max_team_size must be greater than 0");
    }
    if config.database.backend != "sqlite" && config.database.backend != "json" {
        anyhow::bail!("database.backend must be either 'sqlite' or 'json'");
    }
    if config.auth.session_ttl_hours == 0 || config.auth.admin_session_ttl_hours == 0 {
        anyhow::bail!("auth session TTL values must be greater than 0");
    }
    if config.storage.max_file_size_mb == 0 {
        anyhow::bail!("storage.max_file_size_mb must be greater than 0");
    }
    if config.rate_limit.submissions_per_minute == 0 {
        anyhow::bail!("rate_limit.submissions_per_minute must be greater than 0");
    }
    if config.rate_limit.flag_sharing_window_seconds == 0 {
        anyhow::bail!("rate_limit.flag_sharing_window_seconds must be greater than 0");
    }
    Ok(())
}

fn generate_secret() -> String {
    let mut bytes = [0_u8; 32];
    rand::rng().fill_bytes(&mut bytes);

    let mut secret = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(&mut secret, "{byte:02x}");
    }
    secret
}

const EXAMPLE_CONFIG: &str = r#"[server]
port = 8080
host = "0.0.0.0"
base_url = "http://localhost:8080"

[competition]
name = "FeralCTF"
start_time = ""
end_time = ""
team_mode = true
max_team_size = 4
registration_open = true
dynamic_scoring = true
score_freeze_minutes_before_end = 0

[database]
path = "./ctf.db"
backend = "sqlite"

[auth]
jwt_secret = ""
session_ttl_hours = 24
admin_session_ttl_hours = 4

[storage]
attachments_path = "./attachments"
max_file_size_mb = 100

[rate_limit]
submissions_per_minute = 10
wrong_attempts_before_backoff = 5
backoff_base_seconds = 30
flag_sharing_window_seconds = 300

[notifications]
discord_webhook_url = ""

[logging]
level = "info"
format = "json"
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
    }

    fn temp_config_path(name: &str) -> String {
        let path = std::env::temp_dir().join(format!("{name}-{}.toml", std::process::id()));
        path.to_string_lossy().to_string()
    }

    #[test]
    fn test_config_defaults() {
        let _guard = env_lock();
        clear_test_env();

        let config = load("/tmp/feralctf-missing-test-config.toml").unwrap();

        assert_eq!(config.server.port, 8080);
        assert_eq!(config.database.path, "./ctf.db");
        assert_eq!(config.storage.attachments_path, "./attachments");
        assert_eq!(config.auth.jwt_secret.len(), 64);
    }

    #[test]
    fn test_env_override() {
        let _guard = env_lock();
        clear_test_env();
        unsafe {
            env::set_var("FERALCTF_SERVER_PORT", "9999");
            env::set_var(
                "FERALCTF_STORAGE_ATTACHMENTS_PATH",
                "/tmp/feralctf-test-attachments",
            );
        }

        let config = load("/tmp/feralctf-missing-test-config.toml").unwrap();

        assert_eq!(config.server.port, 9999);
        assert_eq!(
            config.storage.attachments_path,
            "/tmp/feralctf-test-attachments"
        );
        clear_test_env();
    }

    #[test]
    fn test_file_load_and_generate_example() {
        let _guard = env_lock();
        clear_test_env();
        let path = temp_config_path("feralctf-config-test");
        let example_path = temp_config_path("feralctf-example-test");

        fs::write(
            &path,
            r#"
[server]
port = 7070

[database]
path = "/tmp/feralctf-test.db"

[storage]
attachments_path = "/tmp/feralctf-file-test-attachments"
"#,
        )
        .unwrap();

        let config = load(&path).unwrap();
        assert_eq!(config.server.port, 7070);
        assert_eq!(config.server.host, "0.0.0.0");
        assert_eq!(config.database.path, "/tmp/feralctf-test.db");

        generate_example(&example_path).unwrap();
        let example = fs::read_to_string(&example_path).unwrap();
        assert!(example.contains("[server]"));
        assert!(example.contains("[rate_limit]"));
    }

    fn clear_test_env() {
        for key in [
            "FERALCTF_SERVER_PORT",
            "FERALCTF_SERVER_HOST",
            "FERALCTF_SERVER_BASE_URL",
            "FERALCTF_COMPETITION_NAME",
            "FERALCTF_COMPETITION_START_TIME",
            "FERALCTF_COMPETITION_END_TIME",
            "FERALCTF_COMPETITION_TEAM_MODE",
            "FERALCTF_COMPETITION_MAX_TEAM_SIZE",
            "FERALCTF_COMPETITION_REGISTRATION_OPEN",
            "FERALCTF_COMPETITION_DYNAMIC_SCORING",
            "FERALCTF_COMPETITION_SCORE_FREEZE_MINUTES_BEFORE_END",
            "FERALCTF_DATABASE_PATH",
            "FERALCTF_DATABASE_BACKEND",
            "FERALCTF_AUTH_JWT_SECRET",
            "FERALCTF_AUTH_SESSION_TTL_HOURS",
            "FERALCTF_AUTH_ADMIN_SESSION_TTL_HOURS",
            "FERALCTF_STORAGE_ATTACHMENTS_PATH",
            "FERALCTF_STORAGE_MAX_FILE_SIZE_MB",
            "FERALCTF_RATE_LIMIT_SUBMISSIONS_PER_MINUTE",
            "FERALCTF_RATE_LIMIT_WRONG_ATTEMPTS_BEFORE_BACKOFF",
            "FERALCTF_RATE_LIMIT_BACKOFF_BASE_SECONDS",
            "FERALCTF_NOTIFICATIONS_DISCORD_WEBHOOK_URL",
            "FERALCTF_LOGGING_LEVEL",
            "FERALCTF_LOGGING_FORMAT",
        ] {
            unsafe {
                env::remove_var(key);
            }
        }
    }
}
