// Reversible flag storage for admin verification (Sprint 16).
//
// Flags are encrypted with AES-256-GCM using one key and one nonce generated
// when the database is first created and stored in it (`flag_cipher`). This is
// an accepted risk for game flags: anyone with the database can decrypt them,
// and the shared nonce means equal flags have equal ciphertext. Submissions are
// always verified against `flag_hash`; nothing here is used at runtime.

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use rand::RngCore;
use ring::aead::{AES_256_GCM, Aad, LessSafeKey, NONCE_LEN, Nonce, UnboundKey};

use crate::errors::AppError;

const KEY_LEN: usize = 32;

/// Create the key/nonce row if it does not exist yet. Never overwrites.
pub fn ensure_key(conn: &rusqlite::Connection) -> Result<(), rusqlite::Error> {
    let mut key = [0u8; KEY_LEN];
    let mut nonce = [0u8; NONCE_LEN];
    rand::rng().fill_bytes(&mut key);
    rand::rng().fill_bytes(&mut nonce);
    conn.execute(
        "INSERT OR IGNORE INTO flag_cipher (id, key, nonce, created_at) VALUES (1, ?1, ?2, ?3)",
        rusqlite::params![
            key.as_slice(),
            nonce.as_slice(),
            chrono::Utc::now().timestamp()
        ],
    )?;
    Ok(())
}

fn load(conn: &rusqlite::Connection) -> Result<(LessSafeKey, [u8; NONCE_LEN]), AppError> {
    let (key, nonce): (Vec<u8>, Vec<u8>) = conn.query_row(
        "SELECT key, nonce FROM flag_cipher WHERE id = 1",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let unbound = UnboundKey::new(&AES_256_GCM, &key)
        .map_err(|_| anyhow::anyhow!("flag cipher key has the wrong length"))?;
    let nonce: [u8; NONCE_LEN] = nonce
        .try_into()
        .map_err(|_| anyhow::anyhow!("flag cipher nonce has the wrong length"))?;
    Ok((LessSafeKey::new(unbound), nonce))
}

/// Encrypt a flag; returns `base64(ciphertext ‖ tag)`.
pub fn encrypt(conn: &rusqlite::Connection, flag: &str) -> Result<String, AppError> {
    let (key, nonce) = load(conn)?;
    let mut buffer = flag.as_bytes().to_vec();
    key.seal_in_place_append_tag(
        Nonce::assume_unique_for_key(nonce),
        Aad::empty(),
        &mut buffer,
    )
    .map_err(|_| anyhow::anyhow!("flag encryption failed"))?;
    Ok(BASE64.encode(buffer))
}

/// Best-effort encryption for write paths. A missing or broken key (for
/// example a database restored without the `flag_cipher` row) must never block
/// creating, editing or importing challenges: the flag is still hashed and
/// playable, it just cannot be revealed until it is re-entered.
pub fn try_encrypt(conn: &rusqlite::Connection, flag: &str) -> Option<String> {
    match encrypt(conn, flag) {
        Ok(ciphertext) => Some(ciphertext),
        Err(err) => {
            tracing::warn!("flag stored hash-only; reversible copy unavailable: {err}");
            None
        }
    }
}

/// Decrypt a stored flag. Fails if the data was tampered with or the key changed.
pub fn decrypt(conn: &rusqlite::Connection, ciphertext: &str) -> Result<String, AppError> {
    let (key, nonce) = load(conn)?;
    let mut buffer = BASE64
        .decode(ciphertext.trim())
        .map_err(|_| anyhow::anyhow!("stored flag is not valid base64"))?;
    let plain = key
        .open_in_place(
            Nonce::assume_unique_for_key(nonce),
            Aad::empty(),
            &mut buffer,
        )
        .map_err(|_| anyhow::anyhow!("stored flag could not be decrypted"))?;
    String::from_utf8(plain.to_vec())
        .map_err(|_| anyhow::anyhow!("stored flag is not valid UTF-8").into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn() -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::run_migrations(&conn).unwrap();
        conn
    }

    fn key_row(conn: &rusqlite::Connection) -> (Vec<u8>, Vec<u8>) {
        conn.query_row("SELECT key, nonce FROM flag_cipher", [], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .unwrap()
    }

    #[test]
    fn round_trip_and_tamper_detection() {
        let conn = conn();
        let ciphertext = encrypt(&conn, "flag{Mixed_Case}").unwrap();
        assert!(!ciphertext.contains("flag"));
        assert_eq!(decrypt(&conn, &ciphertext).unwrap(), "flag{Mixed_Case}");

        let mut bytes = BASE64.decode(&ciphertext).unwrap();
        bytes[0] ^= 0x01;
        assert!(decrypt(&conn, &BASE64.encode(bytes)).is_err());
        assert!(decrypt(&conn, "not base64!").is_err());
    }

    #[test]
    fn key_is_created_once_and_kept() {
        let conn = conn();
        let (key, nonce) = key_row(&conn);
        assert_eq!((key.len(), nonce.len()), (KEY_LEN, NONCE_LEN));
        crate::db::run_migrations(&conn).unwrap();
        ensure_key(&conn).unwrap();
        assert_eq!(key_row(&conn), (key, nonce));
        let rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM flag_cipher", [], |row| row.get(0))
            .unwrap();
        assert_eq!(rows, 1);
    }

    #[test]
    fn separate_databases_get_separate_keys() {
        assert_ne!(key_row(&conn()), key_row(&conn()));
    }

    #[test]
    fn missing_key_degrades_to_hash_only() {
        let conn = conn();
        conn.execute("DELETE FROM flag_cipher", []).unwrap();
        assert!(encrypt(&conn, "flag{x}").is_err());
        assert_eq!(try_encrypt(&conn, "flag{x}"), None);
        // The next migration run (server start / `feralctf migrate`) restores a key.
        crate::db::run_migrations(&conn).unwrap();
        assert!(try_encrypt(&conn, "flag{x}").is_some());
    }
}
