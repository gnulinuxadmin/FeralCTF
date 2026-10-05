-- AES-256-GCM key and nonce for reversible flag storage (admin reveal only;
-- submissions are always checked against flag_hash). The row is created once
-- by run_migrations and never regenerated. challenges.flag_ciphertext is added
-- in db::run_migrations because ALTER TABLE ADD COLUMN is not idempotent.
CREATE TABLE IF NOT EXISTS flag_cipher (
    id         INTEGER PRIMARY KEY CHECK (id = 1),
    key        BLOB NOT NULL,
    nonce      BLOB NOT NULL,
    created_at INTEGER NOT NULL
);
