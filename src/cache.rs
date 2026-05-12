// FeralCTF - Cache Module
// Stub implementation for Sprint 0
// See FERALCTF_SPRINTS.md for requirements

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct Cache {
    data: HashMap<String, CacheEntry>,
}

#[derive(Debug)]
pub struct CacheEntry {
    pub value: String,
    pub expires_at: u64,
}

impl Cache {
    pub fn new() -> Self {
        Self {
            data: HashMap::new(),
        }
    }

    pub fn get(&self, key: &str) -> Option<&CacheEntry> {
        self.data.get(key)
    }

    pub fn set(&mut self, key: String, value: String, ttl_seconds: u64) {
        let expires_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + ttl_seconds;
        
        self.data.insert(key, CacheEntry { value, expires_at });
    }

    pub fn remove(&mut self, key: &str) -> Option<CacheEntry> {
        self.data.remove(key)
    }

    pub fn clear_expired(&mut self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        self.data.retain(|_, entry| entry.expires_at > now);
    }
}