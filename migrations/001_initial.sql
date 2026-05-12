-- FeralCTF - Initial Database Migration
-- Schema per FERALCTF_SPRINTS.md Sprint 1 (canonical source of truth)

PRAGMA journal_mode=WAL;
PRAGMA synchronous=NORMAL;
PRAGMA foreign_keys=OFF;  -- enforced at application layer

CREATE TABLE IF NOT EXISTS users (
    id            INTEGER PRIMARY KEY,
    username      TEXT NOT NULL UNIQUE,
    email         TEXT UNIQUE,
    password_hash TEXT NOT NULL,
    role          TEXT NOT NULL DEFAULT 'player',
    team_id       INTEGER,
    created_at    INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS teams (
    id            INTEGER PRIMARY KEY,
    name          TEXT NOT NULL UNIQUE,
    invite_code   TEXT NOT NULL UNIQUE,
    score         INTEGER NOT NULL DEFAULT 0,
    last_solve_at INTEGER
);

CREATE TABLE IF NOT EXISTS challenges (
    id               INTEGER PRIMARY KEY,
    slug             TEXT NOT NULL UNIQUE,
    title            TEXT NOT NULL,
    description      TEXT NOT NULL,
    category         TEXT NOT NULL,
    flag_hash        TEXT NOT NULL,
    flag_salt        TEXT NOT NULL,
    flag_type        TEXT NOT NULL DEFAULT 'static',
    flag_case_sensitive INTEGER NOT NULL DEFAULT 0,
    points           INTEGER NOT NULL,
    max_points       INTEGER NOT NULL DEFAULT 500,
    min_points       INTEGER NOT NULL DEFAULT 50,
    decay_rate       INTEGER NOT NULL DEFAULT 12,
    author           TEXT,
    tags             TEXT,
    unlock_requires  INTEGER,
    is_hidden        INTEGER NOT NULL DEFAULT 1,
    created_at       INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS solves (
    id           INTEGER PRIMARY KEY,
    team_id      INTEGER NOT NULL,
    user_id      INTEGER NOT NULL,
    challenge_id INTEGER NOT NULL,
    solved_at    INTEGER NOT NULL,
    UNIQUE(team_id, challenge_id)
);

CREATE TABLE IF NOT EXISTS submissions (
    id           INTEGER PRIMARY KEY,
    team_id      INTEGER NOT NULL,
    user_id      INTEGER NOT NULL,
    challenge_id INTEGER NOT NULL,
    flag         TEXT NOT NULL,
    is_correct   INTEGER NOT NULL,
    ip_address   TEXT,
    submitted_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS hints (
    id           INTEGER PRIMARY KEY,
    challenge_id INTEGER NOT NULL,
    content      TEXT NOT NULL,
    cost_points  INTEGER NOT NULL DEFAULT 0,
    sort_order   INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS hint_unlocks (
    id              INTEGER PRIMARY KEY,
    team_id         INTEGER NOT NULL,
    hint_id         INTEGER NOT NULL,
    points_deducted INTEGER NOT NULL,
    unlocked_at     INTEGER NOT NULL,
    UNIQUE(team_id, hint_id)
);

CREATE TABLE IF NOT EXISTS files (
    id           INTEGER PRIMARY KEY,
    challenge_id INTEGER NOT NULL,
    filename     TEXT NOT NULL,
    storage_path TEXT NOT NULL,
    size_bytes   INTEGER NOT NULL,
    sha256       TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS announcements (
    id           INTEGER PRIMARY KEY,
    title        TEXT NOT NULL,
    body         TEXT NOT NULL,
    challenge_id INTEGER,
    is_visible   INTEGER NOT NULL DEFAULT 1,
    created_at   INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS score_history (
    id          INTEGER PRIMARY KEY,
    team_id     INTEGER NOT NULL,
    score       INTEGER NOT NULL,
    recorded_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS sessions (
    id         INTEGER PRIMARY KEY,
    user_id    INTEGER NOT NULL,
    token_hash TEXT NOT NULL UNIQUE,
    expires_at INTEGER NOT NULL,
    revoked    INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_solves_team      ON solves(team_id);
CREATE INDEX IF NOT EXISTS idx_solves_chal      ON solves(challenge_id);
CREATE INDEX IF NOT EXISTS idx_submissions_team ON submissions(team_id, challenge_id);
CREATE INDEX IF NOT EXISTS idx_score_history    ON score_history(team_id, recorded_at);
CREATE INDEX IF NOT EXISTS idx_sessions_token   ON sessions(token_hash);
