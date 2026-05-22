use argon2::{
    Argon2, Params, PasswordHash, PasswordHasher, PasswordVerifier, password_hash::SaltString,
};
use jsonwebtoken::{
    Algorithm, DecodingKey, EncodingKey, Header, Validation, crypto::rust_crypto::DEFAULT_PROVIDER,
    decode, encode,
};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Once;

use crate::{db::DbPool, errors::AppError};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Claims {
    pub sub: i64,
    pub role: String,
    pub team_id: Option<i64>,
    pub exp: u64,
    pub iat: u64,
}

pub fn hash_password(password: &str) -> Result<String, AppError> {
    let mut salt_bytes = [0_u8; 16];
    rand::rng().fill_bytes(&mut salt_bytes);
    let salt = SaltString::encode_b64(&salt_bytes)
        .map_err(|err| anyhow::anyhow!("salt generation failed: {err}"))?;
    let params = Params::new(65536, 3, 2, None)
        .map_err(|err| anyhow::anyhow!("argon2 params error: {err}"))?;
    let argon2 = Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);
    let hash = argon2
        .hash_password(password.as_bytes(), &salt)
        .map_err(|err| anyhow::anyhow!("password hashing failed: {err}"))?
        .to_string();
    Ok(hash)
}

pub fn verify_password(password: &str, hash: &str) -> Result<bool, AppError> {
    let parsed_hash = PasswordHash::new(hash)
        .map_err(|err| AppError::BadRequest(format!("invalid password hash: {err}")))?;
    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed_hash)
        .is_ok())
}

pub fn hash_flag(flag: &str, salt: &str) -> String {
    let normalized = flag.trim().to_lowercase();
    let mut hasher = Sha256::new();
    hasher.update(normalized.as_bytes());
    hasher.update(salt.as_bytes());
    to_hex(&hasher.finalize())
}

pub fn verify_flag(submitted: &str, stored_hash: &str, salt: &str) -> bool {
    hash_flag(submitted, salt) == stored_hash
}

pub fn sign_jwt(claims: &Claims, secret: &str) -> Result<String, AppError> {
    install_crypto_provider();
    encode(
        &Header::new(Algorithm::HS256),
        claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|err| anyhow::anyhow!("jwt signing failed: {err}").into())
}

pub fn verify_jwt(token: &str, secret: &str) -> Result<Claims, AppError> {
    install_crypto_provider();
    let mut validation = Validation::new(Algorithm::HS256);
    validation.validate_exp = true;
    decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &validation,
    )
    .map(|data| data.claims)
    .map_err(|_| AppError::Unauthorized)
}

fn install_crypto_provider() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let _ = DEFAULT_PROVIDER.install_default();
    });
}

pub fn create_session(
    pool: &DbPool,
    user_id: i64,
    token: &str,
    ttl_hours: u64,
) -> Result<(), AppError> {
    let conn = pool
        .get()
        .map_err(|err| anyhow::anyhow!("database pool error: {err}"))?;
    let token_hash = hash_token(token);
    let expires_at = now_unix()?
        .checked_add(
            ttl_hours
                .checked_mul(3600)
                .ok_or_else(|| anyhow::anyhow!("session ttl overflow"))?,
        )
        .ok_or_else(|| anyhow::anyhow!("session expiry overflow"))?;

    conn.execute(
        "INSERT INTO sessions (user_id, token_hash, expires_at, revoked)
         VALUES (?1, ?2, ?3, 0)",
        rusqlite::params![user_id, token_hash, expires_at as i64],
    )?;
    Ok(())
}

pub fn revoke_session(pool: &DbPool, token_hash: &str) -> Result<(), AppError> {
    let conn = pool
        .get()
        .map_err(|err| anyhow::anyhow!("database pool error: {err}"))?;
    conn.execute(
        "UPDATE sessions SET revoked = 1 WHERE token_hash = ?1",
        rusqlite::params![token_hash],
    )?;
    Ok(())
}

pub fn revoke_user_sessions(pool: &DbPool, user_id: i64) -> Result<(), AppError> {
    let conn = pool
        .get()
        .map_err(|err| anyhow::anyhow!("database pool error: {err}"))?;
    conn.execute(
        "UPDATE sessions SET revoked = 1 WHERE user_id = ?1",
        rusqlite::params![user_id],
    )?;
    Ok(())
}

pub fn is_session_valid(pool: &DbPool, token_hash: &str) -> Result<bool, AppError> {
    let conn = pool
        .get()
        .map_err(|err| anyhow::anyhow!("database pool error: {err}"))?;
    let count: i64 = conn.query_row(
        "SELECT COUNT(*)
         FROM sessions
         WHERE token_hash = ?1 AND revoked = 0 AND expires_at > ?2",
        rusqlite::params![token_hash, now_unix()? as i64],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

pub fn cleanup_expired_sessions(pool: &DbPool) -> Result<usize, AppError> {
    let conn = pool
        .get()
        .map_err(|err| anyhow::anyhow!("database pool error: {err}"))?;
    let deleted = conn.execute(
        "DELETE FROM sessions WHERE expires_at <= ?1",
        rusqlite::params![now_unix()? as i64],
    )?;
    Ok(deleted)
}

pub fn hash_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    to_hex(&hasher.finalize())
}

fn now_unix() -> Result<u64, AppError> {
    let timestamp = chrono::Utc::now().timestamp();
    u64::try_from(timestamp)
        .map_err(|err| anyhow::anyhow!("system time before unix epoch: {err}").into())
}

fn to_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_round_trip() {
        let hash = hash_password("correct horse battery staple").unwrap();
        assert!(verify_password("correct horse battery staple", &hash).unwrap());
        assert!(!verify_password("wrong password", &hash).unwrap());
    }

    #[test]
    fn jwt_round_trip() {
        let claims = Claims {
            sub: 42,
            role: "admin".to_string(),
            team_id: Some(7),
            iat: now_unix().unwrap(),
            exp: now_unix().unwrap() + 3600,
        };
        let token = sign_jwt(&claims, "test-secret").unwrap();
        let decoded = verify_jwt(&token, "test-secret").unwrap();
        assert_eq!(decoded, claims);
    }

    #[test]
    fn expired_jwt_errors() {
        let claims = Claims {
            sub: 42,
            role: "player".to_string(),
            team_id: None,
            iat: 1,
            exp: 1,
        };
        let token = sign_jwt(&claims, "test-secret").unwrap();
        assert!(matches!(
            verify_jwt(&token, "test-secret"),
            Err(AppError::Unauthorized)
        ));
    }

    #[test]
    fn flag_hash_is_case_insensitive_and_trimmed() {
        let salt = "challenge-salt";
        let hash = hash_flag(" flag{Case_Mix} ", salt);
        assert!(verify_flag("FLAG{case_mix}", &hash, salt));
        assert!(verify_flag("  flag{case_mix}  ", &hash, salt));
        assert!(!verify_flag("flag{other}", &hash, salt));
    }
}
