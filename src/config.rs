// FeralCTF - Configuration module
// Implements FERALCTF_SPEC.md section 5.1

use serde::{Deserialize, Serialize};

/// Application configuration structure
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AppConfig {
    pub app_name: String,
    pub version: String,
    pub database_url: String,
    pub cache_ttl_seconds: u64,
    pub max_connections: u32,
    pub cors_origins: Vec<String>,
    pub rate_limit_per_minute: u32,
    pub max_upload_size_mb: u32,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            app_name: String::from("FeralCTF"),
            version: env!("CARGO_PKG_VERSION").to_string(),
            database_url: String::from("sqlite:///feralctf.db"),
            cache_ttl_seconds: 300,
            max_connections: 10,
            cors_origins: Vec::new(),
            rate_limit_per_minute: 60,
            max_upload_size_mb: 10,
        }
    }
}

/// Cache configuration
#[derive(Debug, Clone, Default)]
pub struct CacheConfig {
    pub max_entries: usize,
    pub eviction_policy: CacheEvictionPolicy,
}

#[derive(Debug, Clone, Default)]
pub enum CacheEvictionPolicy {
    #[default]
    LRU,
    LFU,
    TTL,
}

/// Database connection pool configuration
#[derive(Debug, Clone)]
pub struct DatabaseConfig {
    pub pool_size: usize,
    pub max_lifetime: Option<std::time::Duration>,
}