# FeralCTF — Implementation Sprints

> **Instructions for agents receiving these sprints:**
>
> - Read the entire sprint before writing any code.
> - Do not modify `SPEC.md` — it is immutable. If the spec conflicts with your assumptions, the spec wins.
> - Do not add dependencies not listed in `Cargo.toml`.
> - Do not use `sqlx` — it conflicts with `rusqlite` via a libsqlite3-sys feature flag bug.
> - Each sprint has explicit inputs, outputs, and acceptance criteria.
> - Write idiomatic Rust. Use `thiserror` for error types, `anyhow` for application-level errors.
> - All `unwrap()` calls are forbidden in non-test code. Use `?` propagation.
> - Run `cargo check` before declaring a sprint done.

---

## Sprint 0 — Project Scaffold

**Goal:** Clean directory structure, compiling skeleton, no logic.

**Inputs:**

- `Cargo.toml` (provided, do not modify)

**Outputs — create these files with stub content that compiles:**

```text
src/main.rs
src/config.rs
src/errors.rs
src/db/mod.rs
src/handlers/mod.rs
src/handlers/auth.rs
src/handlers/challenges.rs
src/handlers/scoreboard.rs
src/handlers/admin.rs
src/handlers/ws.rs
src/models/mod.rs
src/models/user.rs
src/models/team.rs
src/models/challenge.rs
src/models/scoreboard.rs
src/cache.rs
src/scoring.rs
src/anticheat.rs
src/storage.rs
src/import_export.rs
src/auth.rs
frontend/index.html
frontend/app.js
frontend/style.css
migrations/001_initial.sql
```

**Rules:**

- `main.rs` must compile with `cargo check` — stubs only, no logic
- Every module must be declared in its parent `mod.rs`
- `frontend/` files can be empty placeholders
- `migrations/001_initial.sql` must contain the full schema (see Sprint 1)

**Acceptance criteria:**

- `cargo check` passes with zero errors
- `cargo check` passes with zero warnings (use `#[allow(dead_code)]` on stubs if needed)

---

## Sprint 1 — Database Schema + Connection Pool

**Goal:** SQLite database layer. Schema, migrations, connection pool, helper traits.

**Inputs:**

- `src/db/mod.rs` (stub from Sprint 0)
- `migrations/001_initial.sql` (stub from Sprint 0)

**Schema — write this exactly into `migrations/001_initial.sql`:**

```sql
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

CREATE INDEX IF NOT EXISTS idx_solves_team     ON solves(team_id);
CREATE INDEX IF NOT EXISTS idx_solves_chal     ON solves(challenge_id);
CREATE INDEX IF NOT EXISTS idx_submissions_team ON submissions(team_id, challenge_id);
CREATE INDEX IF NOT EXISTS idx_score_history   ON score_history(team_id, recorded_at);
CREATE INDEX IF NOT EXISTS idx_sessions_token  ON sessions(token_hash);
```

**Implement in `src/db/mod.rs`:**

```rust
// Public API this module must expose:

pub type DbPool = r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>;
pub type DbConn = r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>;

pub fn init_pool(db_path: &str) -> Result<DbPool, anyhow::Error>
// Creates pool, runs migrations, returns pool.
// Must set WAL mode and synchronous=NORMAL on every new connection via connection customizer.

pub fn run_migrations(conn: &rusqlite::Connection) -> Result<(), anyhow::Error>
// Reads and executes migrations/001_initial.sql
// Must be idempotent — safe to run on an existing database
```

**Acceptance criteria:**

- `cargo check` passes
- Pool initializes and WAL pragma is set on connection open
- Migration is idempotent (running twice does not error)
- Unit test: `#[test] fn test_migration_idempotent()` passes

---

## Sprint 2 — Config Loading

**Goal:** Load `config.toml`, override with environment variables, validate.

**Implement in `src/config.rs`:**

```rust
// Structs to implement (all fields must have serde defaults):

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

pub struct ServerConfig {
    pub port: u16,           // default: 8080
    pub host: String,        // default: "0.0.0.0"
    pub base_url: String,    // default: "http://localhost:8080"
}

pub struct CompetitionConfig {
    pub name: String,                           // default: "FeralCTF"
    pub start_time: Option<String>,             // ISO 8601
    pub end_time: Option<String>,               // ISO 8601
    pub team_mode: bool,                        // default: true
    pub max_team_size: u32,                     // default: 4
    pub registration_open: bool,                // default: true
    pub dynamic_scoring: bool,                  // default: true
    pub score_freeze_minutes_before_end: u32,   // default: 0
}

pub struct DatabaseConfig {
    pub path: String,        // default: "./ctf.db"
    pub backend: String,     // default: "sqlite"
}

pub struct AuthConfig {
    pub jwt_secret: String,              // auto-generated if empty
    pub session_ttl_hours: u64,          // default: 24
    pub admin_session_ttl_hours: u64,    // default: 4
}

pub struct StorageConfig {
    pub attachments_path: String,   // default: "./attachments"
    pub max_file_size_mb: u64,      // default: 100
}

pub struct RateLimitConfig {
    pub submissions_per_minute: u32,         // default: 10
    pub wrong_attempts_before_backoff: u32,  // default: 5
    pub backoff_base_seconds: u64,           // default: 30
}

pub struct NotificationsConfig {
    pub discord_webhook_url: Option<String>,
}

pub struct LoggingConfig {
    pub level: String,   // default: "info"
    pub format: String,  // default: "json"
}

// Public API:
pub fn load(path: &str) -> Result<Config, anyhow::Error>
// 1. Read config.toml (ok if missing — use all defaults)
// 2. Override any field with env var FERALCTF_SECTION_KEY
//    e.g. FERALCTF_SERVER_PORT=9090, FERALCTF_DATABASE_PATH=/data/ctf.db
// 3. If auth.jwt_secret is empty, generate 32-byte random hex and set it
// 4. Create storage.attachments_path directory if it does not exist
// 5. Return validated Config

pub fn generate_example(path: &str) -> Result<(), anyhow::Error>
// Write a config.example.toml with all fields and comments
```

**Acceptance criteria:**

- `cargo check` passes
- Loads from file, falls back to defaults if file missing
- Env var `FERALCTF_SERVER_PORT=9999` overrides port
- Unit test: `#[test] fn test_config_defaults()` passes
- Unit test: `#[test] fn test_env_override()` passes

---

## Sprint 3 — Auth (Argon2id + JWT + Sessions)

**Goal:** Password hashing, JWT signing/verification, session management.

**Implement in `src/auth.rs`:**

```rust
// Password hashing
pub fn hash_password(password: &str) -> Result<String, AppError>
// Argon2id, params: memory=65536, iterations=3, parallelism=2

pub fn verify_password(password: &str, hash: &str) -> Result<bool, AppError>

// Flag hashing
pub fn hash_flag(flag: &str, salt: &str) -> String
// sha256(flag.to_lowercase().trim() + salt) — hex encoded

pub fn verify_flag(submitted: &str, stored_hash: &str, salt: &str) -> bool

// JWT
pub struct Claims {
    pub sub: i64,        // user_id
    pub role: String,    // "admin" | "player" | "spectator"
    pub team_id: Option<i64>,
    pub exp: u64,        // unix timestamp
    pub iat: u64,
}

pub fn sign_jwt(claims: &Claims, secret: &str) -> Result<String, AppError>
pub fn verify_jwt(token: &str, secret: &str) -> Result<Claims, AppError>

// Session management (requires DbPool)
pub fn create_session(pool: &DbPool, user_id: i64, token: &str, ttl_hours: u64)
    -> Result<(), AppError>
pub fn revoke_session(pool: &DbPool, token_hash: &str) -> Result<(), AppError>
pub fn is_session_valid(pool: &DbPool, token_hash: &str) -> Result<bool, AppError>
pub fn cleanup_expired_sessions(pool: &DbPool) -> Result<usize, AppError>
// Returns count of deleted sessions
```

**Implement in `src/errors.rs`:**

```rust
#[derive(thiserror::Error, Debug)]
pub enum AppError {
    #[error("unauthorized")]
    Unauthorized,
    #[error("forbidden")]
    Forbidden,
    #[error("not found: {0}")]
    NotFound(String),
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("rate limited")]
    RateLimited,
    #[error("internal error: {0}")]
    Internal(#[from] anyhow::Error),
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
}

// Must implement axum::response::IntoResponse for AppError
// Unauthorized -> 401, Forbidden -> 403, NotFound -> 404,
// BadRequest -> 400, RateLimited -> 429, Internal/Database -> 500
// All responses are JSON: { "error": "<message>" }
```

**Acceptance criteria:**

- `cargo check` passes
- Unit test: hash and verify a password round-trips correctly
- Unit test: wrong password returns false, not error
- Unit test: JWT signs and verifies with correct claims
- Unit test: expired JWT returns error
- Unit test: flag hash is case-insensitive and trims whitespace

---

## Sprint 4 — Models

**Goal:** Rust structs for all database entities. No handlers yet.

**Implement in `src/models/user.rs`:**

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub email: Option<String>,
    pub password_hash: String,
    pub role: String,
    pub team_id: Option<i64>,
    pub created_at: i64,
}

#[derive(Debug, serde::Deserialize)]
pub struct RegisterRequest {
    pub username: String,
    pub email: Option<String>,
    pub password: String,
    pub team_name: Option<String>,    // create team on register
    pub invite_code: Option<String>,  // join existing team
}

#[derive(Debug, serde::Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, serde::Serialize)]
pub struct LoginResponse {
    pub token: String,
    pub expires_at: u64,
    pub user: UserPublic,
}

#[derive(Debug, serde::Serialize)]
pub struct UserPublic {
    pub id: i64,
    pub username: String,
    pub role: String,
    pub team_id: Option<i64>,
}

// DB helpers on User:
impl User {
    pub fn find_by_id(conn: &DbConn, id: i64) -> Result<Option<Self>, AppError>
    pub fn find_by_username(conn: &DbConn, username: &str) -> Result<Option<Self>, AppError>
    pub fn create(conn: &DbConn, req: &RegisterRequest, password_hash: &str)
        -> Result<Self, AppError>
    pub fn count(conn: &DbConn) -> Result<i64, AppError>
}
```

**Implement in `src/models/team.rs`:**

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Team {
    pub id: i64,
    pub name: String,
    pub invite_code: String,
    pub score: i64,
    pub last_solve_at: Option<i64>,
}

impl Team {
    pub fn find_by_id(conn: &DbConn, id: i64) -> Result<Option<Self>, AppError>
    pub fn find_by_invite_code(conn: &DbConn, code: &str) -> Result<Option<Self>, AppError>
    pub fn create(conn: &DbConn, name: &str) -> Result<Self, AppError>
    // invite_code is a random 8-char alphanumeric string generated here
    pub fn add_member(conn: &DbConn, team_id: i64, user_id: i64) -> Result<(), AppError>
    pub fn update_score(conn: &DbConn, team_id: i64, delta: i64, solved_at: i64)
        -> Result<(), AppError>
}
```

**Implement in `src/models/challenge.rs`:**

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Challenge { /* all DB fields */ }

#[derive(Debug, serde::Serialize)]
pub struct ChallengePublic {
    // Safe for player view — no flag_hash, no flag_salt
    pub id: i64,
    pub slug: String,
    pub title: String,
    pub description: String,
    pub category: String,
    pub points: i64,
    pub solve_count: i64,
    pub solved_by_team: bool,  // injected per-request
    pub tags: Vec<String>,
    pub file_count: i64,
    pub hint_count: i64,
    pub unlock_requires: Option<i64>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Hint {
    pub id: i64,
    pub challenge_id: i64,
    pub content: String,
    pub cost_points: i64,
    pub sort_order: i64,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct ChallengeFile {
    pub id: i64,
    pub challenge_id: i64,
    pub filename: String,
    pub storage_path: String,
    pub size_bytes: i64,
    pub sha256: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Submission {
    pub id: i64,
    pub team_id: i64,
    pub user_id: i64,
    pub challenge_id: i64,
    pub flag: String,
    pub is_correct: bool,
    pub ip_address: Option<String>,
    pub submitted_at: i64,
}

impl Challenge {
    pub fn find_by_id(conn: &DbConn, id: i64) -> Result<Option<Self>, AppError>
    pub fn find_by_slug(conn: &DbConn, slug: &str) -> Result<Option<Self>, AppError>
    pub fn list_visible(conn: &DbConn) -> Result<Vec<Self>, AppError>
    pub fn list_all(conn: &DbConn) -> Result<Vec<Self>, AppError>
    pub fn solve_count(conn: &DbConn, challenge_id: i64) -> Result<i64, AppError>
    pub fn is_solved_by_team(conn: &DbConn, challenge_id: i64, team_id: i64)
        -> Result<bool, AppError>
}
```

**Implement in `src/models/scoreboard.rs`:**

```rust
#[derive(Debug, Clone, serde::Serialize)]
pub struct TeamScore {
    pub rank: i64,
    pub team_id: i64,
    pub team_name: String,
    pub score: i64,
    pub solve_count: i64,
    pub last_solve_at: Option<i64>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ScoreboardState {
    pub teams: Vec<TeamScore>,
    pub generated_at: i64,
}

impl ScoreboardState {
    pub fn build(conn: &DbConn) -> Result<Self, AppError>
    // Query teams ordered by score DESC, last_solve_at ASC for tiebreaking
    // Assign rank (ties share same rank)
}
```

**Acceptance criteria:**

- `cargo check` passes with zero errors
- All DB helper methods use parameterized queries (no string interpolation)
- `ChallengePublic` never exposes `flag_hash` or `flag_salt`

---

## Sprint 5 — Auth Handlers

**Goal:** HTTP handlers for registration, login, logout, /me, password change.

**Implement in `src/handlers/auth.rs`:**

```text
POST /api/auth/register
POST /api/auth/login
POST /api/auth/logout
GET  /api/auth/me
PUT  /api/auth/password
```

**Rules:**

- Extract JWT from `Authorization: Bearer <token>` header
- On register: if no other users exist, set role = "admin"
- On register: validate username (3-32 chars, alphanumeric + underscore + hyphen)
- On register: validate password (minimum 8 chars)
- On register: if `team_name` provided, create team; if `invite_code` provided, join team
- On login: verify password, create session, return JWT
- On logout: revoke session token
- All handlers return `Result<Json<T>, AppError>`
- Use `axum::extract::State<AppState>` for shared state

**AppState — define in `src/main.rs`:**

```rust
#[derive(Clone)]
pub struct AppState {
    pub db: DbPool,
    pub config: Arc<Config>,
    pub cache: Arc<AppCache>,  // stub for now, implement in Sprint 8
}
```

**Acceptance criteria:**

- `cargo check` passes
- First registered user gets role = "admin"
- Login with wrong password returns 401
- JWT from login is valid and contains correct claims
- Logout invalidates session (subsequent /me with same token returns 401)

---

## Sprint 6 — Challenge Handlers + Flag Submission

**Goal:** Challenge list, detail, flag submission, hint unlock.

**Implement in `src/handlers/challenges.rs`:**

```text
GET  /api/challenges                     -> Vec<ChallengePublic>
GET  /api/challenges/:id                 -> ChallengePublic + hints + files
POST /api/challenges/:id/submit          -> SubmitResponse
POST /api/challenges/:id/hints/:hid/unlock -> HintUnlockResponse
```

**Flag submission rules:**

- Strip leading/trailing whitespace from submitted flag
- Max length 256 chars
- Hash and compare: `hash_flag(submitted, challenge.flag_salt) == challenge.flag_hash`
- For `flag_type = "regex"`: stored flag is a regex pattern, match against submission directly
- Record every submission in `submissions` table regardless of correct/wrong
- On correct: insert into `solves`, call `scoring::recalculate_challenge_points()`,
  update team score, invalidate scoreboard cache
- On correct first solve: set `first_blood = true` in response, broadcast WS event
- Return `SubmitResponse { correct, points_earned, first_blood, new_score }`

**Rate limiting — use `src/anticheat.rs`:**

```rust
// Call before processing submission:
pub fn check_rate_limit(state: &AppState, team_id: i64, ip: &str)
    -> Result<(), AppError>
// Returns AppError::RateLimited if over limit
// Response must include Retry-After header with seconds
```

**Implement in `src/scoring.rs`:**

```rust
pub fn dynamic_points(max_points: i64, min_points: i64, decay_rate: i64, solves: i64) -> i64 {
    // points = max(min_points, ceil(max_points - decay_rate * (solves - 1)^2))
}

pub fn recalculate_challenge_points(conn: &DbConn, challenge_id: i64) -> Result<(), AppError>
// After a new solve: recalculate points for this challenge based on new solve count
// Update all existing team scores for this challenge accordingly
// This is the "retroactive decay" behavior matching CTFd
```

**Acceptance criteria:**

- `cargo check` passes
- Correct flag returns 200 with `correct: true`
- Wrong flag returns 200 with `correct: false` (not 4xx)
- Submitting after already solved returns `correct: false, message: "already solved"`
- Rate limit kicks in after configured attempts
- Dynamic scoring reduces value as solve count increases
- Score history entry created on each accepted solve

---

## Sprint 7 — Scoreboard Handler + Cache

**Goal:** Scoreboard endpoint served from in-process cache. Score graph data.

**Implement in `src/cache.rs`:**

```rust
pub struct AppCache {
    pub scoreboard: RwLock<Option<ScoreboardState>>,
    pub challenges: RwLock<Option<Vec<Challenge>>>,
}

impl AppCache {
    pub fn new() -> Self
    pub fn invalidate_scoreboard(&self)
    pub fn invalidate_challenges(&self)
    pub fn get_or_build_scoreboard(
        &self,
        conn: &DbConn
    ) -> Result<ScoreboardState, AppError>
    pub fn get_or_build_challenges(
        &self,
        conn: &DbConn
    ) -> Result<Vec<Challenge>, AppError>
}
```

**Implement in `src/handlers/scoreboard.rs`:**

```text
GET /api/scoreboard        -> ScoreboardState (from cache)
GET /api/scoreboard/graph  -> Vec<TeamGraphData>
GET /api/teams/:id         -> TeamProfile
POST /api/teams            -> Team (create)
POST /api/teams/join       -> Team (join by invite_code)
```

**TeamGraphData:**

```rust
pub struct TeamGraphData {
    pub team_id: i64,
    pub team_name: String,
    pub points: Vec<(i64, i64)>,  // (timestamp, score) pairs from score_history
}
```

**Background task — start in `main.rs`:**

```rust
// Spawn a Tokio task that records score snapshots every 5 minutes:
// INSERT INTO score_history (team_id, score, recorded_at) for all teams
```

**Acceptance criteria:**

- `cargo check` passes
- Scoreboard is served from cache (not a fresh DB query on every request)
- Cache is invalidated when Sprint 6 records a correct submission
- Graph endpoint returns time-series data suitable for a line chart
- Background score snapshot task runs without blocking the main thread

---

## Sprint 8 — WebSocket Hub

**Goal:** Real-time event broadcast to all connected clients.

**Implement in `src/handlers/ws.rs`:**

```rust
// Event types
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WsEvent {
    NewSolve {
        team: String,
        challenge: String,
        points: i64,
        first_blood: bool,
    },
    Announcement {
        title: String,
        body: String,
    },
    StateChange {
        started: bool,
        ended: bool,
        frozen: bool,
    },
    ScoreUpdate {
        scoreboard: Vec<TeamScore>,
    },
}

// Hub — holds a tokio::sync::broadcast::Sender<WsEvent>
pub struct WsHub {
    pub tx: broadcast::Sender<WsEvent>,
}

impl WsHub {
    pub fn new() -> Self  // capacity: 256
    pub fn broadcast(&self, event: WsEvent)
}

// WebSocket upgrade handler
// GET /ws  (no auth required — events are already public scoreboard data)
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse
// On connect: subscribe to hub, forward events to client as JSON text frames
// On disconnect: subscription drops automatically
// Ping client every 30s to detect dead connections
```

**Wire into AppState:**

```rust
pub struct AppState {
    pub db: DbPool,
    pub config: Arc<Config>,
    pub cache: Arc<AppCache>,
    pub ws_hub: Arc<WsHub>,  // add this
}
```

**Wire broadcast calls:**

- In Sprint 6 flag submission handler: broadcast `WsEvent::NewSolve` on correct submission
- In Sprint 6 flag submission handler: broadcast `WsEvent::ScoreUpdate` after score recalculation

**Acceptance criteria:**

- `cargo check` passes
- Client connecting to `/ws` receives a ping every 30s
- Correct flag submission triggers a broadcast visible to all connected clients
- Disconnected clients do not cause panics (lagged receiver is silently dropped)

---

## Sprint 9 — Admin Handlers

**Goal:** Admin-only endpoints for challenge CRUD, user/team management, competition controls.

**Implement in `src/handlers/admin.rs`:**

**Middleware — apply to all /api/admin/* routes:**

```rust
// Extract JWT, verify role == "admin", else return 403
pub async fn require_admin(/* ... */) -> Result<Next, AppError>
```

**Challenge CRUD:**

```text
POST   /api/admin/challenges          CreateChallengeRequest -> Challenge
PUT    /api/admin/challenges/:id      UpdateChallengeRequest -> Challenge
DELETE /api/admin/challenges/:id      -> { deleted: true }
```

```rust
pub struct CreateChallengeRequest {
    pub title: String,
    pub category: String,
    pub description: String,
    pub flag: String,           // plaintext, will be hashed before storage
    pub flag_type: String,      // "static" | "regex" | "dynamic"
    pub flag_case_sensitive: bool,
    pub points: i64,
    pub max_points: i64,
    pub min_points: i64,
    pub decay_rate: i64,
    pub author: Option<String>,
    pub tags: Vec<String>,
    pub unlock_requires: Option<i64>,
    pub is_hidden: bool,
}
// slug is auto-generated from title: lowercase, spaces->hyphens, strip non-alphanumeric
// flag_salt is randomly generated
// flag is hashed via hash_flag() before storage — never stored plaintext
```

**Submission log:**

```text
GET /api/admin/submissions?team_id=&challenge_id=&correct=&page=&per_page=
-> PaginatedSubmissions
```

**User + team management:**

```text
GET    /api/admin/users                -> Vec<UserPublic>
POST   /api/admin/users/:id/ban        -> { banned: true }
GET    /api/admin/teams                -> Vec<Team>
POST   /api/admin/teams/:id/disqualify -> { disqualified: true }
// disqualify: set score=0, last_solve_at=null, add "disqualified" flag to team
```

**Competition controls:**

```text
POST /api/admin/competition/start   -> { started: true }
POST /api/admin/competition/end     -> { ended: true }
POST /api/admin/competition/freeze  -> { frozen: true }
// Each broadcasts a WsEvent::StateChange
POST /api/admin/announce            -> { sent: true }
// Body: { title, body, challenge_id? }
// Inserts into announcements table, broadcasts WsEvent::Announcement
```

**Backup:**

```text
GET /api/admin/backup
// Streams the raw SQLite file as application/octet-stream
// Filename: feralctf-backup-{timestamp}.db
// Uses rusqlite online backup API (no file lock required)
```

**Acceptance criteria:**

- `cargo check` passes
- Non-admin JWT returns 403 on all /api/admin/* routes
- Challenge creation hashes flag before DB insert (verify flag_hash != plaintext)
- Disqualify sets team score to 0 and invalidates scoreboard cache
- Backup endpoint produces a valid SQLite file

---

## Sprint 10 — Import / Export

**Goal:** Full game JSON export and import with CTFd compatibility.

**Implement in `src/import_export.rs`:**

**Export:**

```rust
pub struct ExportBundle {
    pub feralctf_export_version: u32,   // always 1
    pub exported_at: String,             // ISO 8601
    pub competition: CompetitionMeta,
    pub categories: Vec<String>,
    pub challenges: Vec<ExportChallenge>,
}

pub struct ExportChallenge {
    pub slug: String,
    pub title: String,
    pub category: String,
    pub description: String,
    pub flag: String,           // PLAINTEXT — admin export only
    pub flag_type: String,
    pub flag_case_sensitive: bool,
    pub points: i64,
    pub max_points: i64,
    pub min_points: i64,
    pub decay_rate: i64,
    pub author: Option<String>,
    pub tags: Vec<String>,
    pub hints: Vec<ExportHint>,
    pub files: Vec<ExportFile>,  // data field is base64 if inline mode
    pub unlock_requires: Option<String>,  // slug reference, not id
    pub is_hidden: bool,
}

pub fn export(conn: &DbConn, config: &Config, inline_attachments: bool)
    -> Result<ExportBundle, AppError>
```

**Import:**

```rust
pub struct ImportOptions {
    pub overwrite: bool,
    pub dry_run: bool,
}

pub struct ImportResult {
    pub valid: bool,
    pub challenges_created: usize,
    pub challenges_skipped: usize,
    pub challenges_overwritten: usize,
    pub attachment_warnings: Vec<String>,
    pub validation_errors: Vec<String>,
    pub preview: Vec<ImportPreviewItem>,
}

pub fn import(
    conn: &DbConn,
    bundle: &ExportBundle,
    attachments_dir: Option<&Path>,
    options: &ImportOptions,
) -> Result<ImportResult, AppError>
// Conflict resolution:
// - slug not in DB: create
// - slug exists, identical content: skip (no-op)
// - slug exists, differs, overwrite=false: skip + warn
// - slug exists, differs, overwrite=true: overwrite
// Invalid JSON schema: return error, NO partial writes (wrap in transaction)

pub fn detect_and_convert_ctfd(raw: &[u8]) -> Result<ExportBundle, AppError>
// Detect CTFd export format, convert to ExportBundle
// CTFd dynamic challenges: set is_hidden=true, add warning to result
```

**Endpoints — add to `src/handlers/admin.rs`:**

```text
GET  /api/admin/export                      -> JSON download
GET  /api/admin/export?attachments=inline   -> JSON with base64 files
GET  /api/admin/export?attachments=zip      -> JSON + ZIP (multipart or separate endpoint)
POST /api/admin/import                      -> ImportResult
POST /api/admin/import?dry_run=true         -> ImportResult (no DB writes)
```

**CLI subcommand — add to `src/main.rs`:**

```bash
feralctf import <file> [--attachments <dir>] [--overwrite] [--dry-run]
```

**Acceptance criteria:**

- `cargo check` passes
- Export then import round-trips all challenge data without loss
- Import is idempotent (running twice with same bundle produces same DB state)
- CTFd export ZIP is detected and converted automatically
- `dry_run=true` returns accurate preview without any DB writes
- Import is wrapped in a transaction: invalid bundle = zero changes

---

## Sprint 11 — Anti-Cheat + Rate Limiting

**Goal:** Submission rate limiting, exponential backoff, flag sharing detection.

**Implement in `src/anticheat.rs`:**

```rust
pub struct RateLimiter {
    // Per-team sliding window: DashMap<team_id, VecDeque<Instant>>
    team_windows: DashMap<i64, VecDeque<std::time::Instant>>,
    // Per-team-challenge wrong attempt counter: DashMap<(team_id, challenge_id), u32>
    wrong_attempts: DashMap<(i64, i64), (u32, std::time::Instant)>,
}

impl RateLimiter {
    pub fn new() -> Self

    pub fn check_submission(
        &self,
        team_id: i64,
        challenge_id: i64,
        config: &RateLimitConfig,
    ) -> Result<(), AppError>
    // 1. Check sliding window: if team has >= submissions_per_minute in last 60s -> RateLimited
    // 2. Check wrong attempts: if >= wrong_attempts_before_backoff wrong attempts for this
    //    (team, challenge) pair, enforce exponential backoff:
    //    wait = backoff_base_seconds * 2^(wrong_attempts - threshold)
    //    if time since last attempt < wait -> RateLimited with Retry-After header

    pub fn record_attempt(&self, team_id: i64, challenge_id: i64, correct: bool)
    // Update internal counters after an attempt

    pub fn gc(&self)
    // Remove expired windows and old attempt records
    // Called every 60s by background task
}

pub fn check_flag_sharing(
    conn: &DbConn,
    challenge_id: i64,
    team_id: i64,
    window_seconds: u64,
) -> Result<bool, AppError>
// Returns true if same correct flag was submitted by another team within window_seconds
// Log warning if detected — do not auto-disqualify, flag for admin review
```

**Wire up:**

- Add `RateLimiter` to `AppState`
- Call `check_submission()` at start of flag submission handler (Sprint 6)
- Call `record_attempt()` after each submission
- Spawn background GC task in `main.rs` (every 60s)

**Acceptance criteria:**

- `cargo check` passes
- 11th submission in 60s returns 429 with `Retry-After` header
- Exponential backoff doubles wait time on each wrong attempt past threshold
- GC task runs without blocking handlers
- Flag sharing check queries DB efficiently (uses existing index)

---

## Sprint 12 — Frontend SPA

**Goal:** Vanilla JS single-page application embedded into the binary.

**Files:**

```text
frontend/index.html
frontend/app.js
frontend/style.css
```

**Views to implement:**

1. **Challenges** — grid of challenge cards, filter by category, search by name.
   Each card shows: title, category (color-coded), points, solve count, difficulty dot, solved marker.
   Click opens modal: description, files (download links), flag input, hint list (locked/unlocked).

2. **Scoreboard** — table: rank, team name, solves, progress bar, score.
   Current team row highlighted. Auto-updates via WebSocket.
   On `score_update` WS event: re-render table without page reload.

3. **Profile** — avatar, username, team, rank, score, solves, hints used, first bloods.
   Solve history list: category, challenge name, points, time.

4. **Admin** — sidebar nav (Overview, Challenges, Users, Teams, Settings).
   Overview: 4 stat cards (teams, challenges, solves, submissions) + recent submission log.
   Challenges: table with edit/delete buttons + "Add Challenge" form.
   Users: table with ban button.
   Teams: table with disqualify button.
   Settings: competition name, times, toggles for team mode / dynamic scoring / score freeze.

**Design constraints:**

- Dark terminal aesthetic: background `#0a0e1a`, accent `#63d28c`, font `'Courier New', monospace`
- No external CDN dependencies — all JS/CSS inline in the files
- No frameworks (no React, Vue, jQuery)
- WebSocket: connect on load, reconnect with exponential backoff on disconnect
- Auth: store JWT in `sessionStorage` (not localStorage)
- All API calls use `fetch()` with `Authorization: Bearer <token>` header

**Embed in binary — add to `src/main.rs`:**

```rust
#[derive(rust_embed::Embed)]
#[folder = "frontend/"]
struct FrontendAssets;

// Serve GET /* -> index.html for all non-/api paths (SPA routing)
// Serve GET /static/* -> embedded assets
```

**Acceptance criteria:**

- `cargo check` passes
- `cargo build` produces a single binary with frontend embedded
- Navigating to `/` in a browser shows the challenges view
- Flag submission works end-to-end in browser
- Scoreboard updates without page refresh when a flag is submitted

---

## Sprint 13 — CLI Subcommands + Hardening

**Goal:** `feralctf init`, `feralctf migrate`, HTTP security headers, final hardening.

**CLI — implement in `src/main.rs` using `std::env::args`:**

```bash
feralctf                          # start server (default)
feralctf --port 8080              # start server on port
feralctf --config /path/config.toml  # start with config file
feralctf init                     # generate config.toml + empty DB
feralctf migrate                  # run migrations on existing DB
feralctf import <file>            # import challenges
feralctf import <file> --dry-run
feralctf import <file> --overwrite
feralctf import <file> --attachments <dir>
```

**Security headers — add Tower middleware layer:**

```text
X-Content-Type-Options: nosniff
X-Frame-Options: DENY
Referrer-Policy: strict-origin-when-cross-origin
Content-Security-Policy: default-src 'self'; connect-src 'self' ws: wss:
```

**CORS — configure tower-http CorsLayer:**

```rust
// Default: same-origin only
// Configurable via config.toml [server] allowed_origins field
```

**Admin audit log — add to `src/db/mod.rs`:**

```sql
CREATE TABLE IF NOT EXISTS audit_log (
    id         INTEGER PRIMARY KEY,
    user_id    INTEGER NOT NULL,
    action     TEXT NOT NULL,
    target     TEXT,
    detail     TEXT,
    ip_address TEXT,
    created_at INTEGER NOT NULL
);
```

```rust
pub fn audit(conn: &DbConn, user_id: i64, action: &str, target: Option<&str>,
             detail: Option<&str>, ip: Option<&str>) -> Result<(), AppError>
// Call this from all admin handlers
```

**`feralctf init` output:**

```text
config.toml          (generated with all defaults + random jwt_secret)
ctf.db               (empty database with schema applied)
attachments/         (empty directory)
```

**Acceptance criteria:**

- `cargo check` passes
- `cargo build --release` produces a single binary
- `./feralctf init` creates all three files/dirs
- Security headers present on all responses
- Audit log entry created on challenge create/delete and team disqualify
- `./feralctf import challenges.json --dry-run` prints preview without writing DB

---

---

## Post-Sprint 13 — v1.0rc Improvements

Sprints 0–13 are complete. The following improvements were made during the 1.0rc series
(1.0rc1–1.0rc5) after sprint completion. They are implemented in the existing sprint files;
no new sprint scope is required.

### Frontend

- **Web registration UI** — `showRegisterModal()` and `registerUser()` added to `frontend/app.js`.
  Users can register directly from the browser; no out-of-band admin setup required.
- **Admin nav gating** — `updateAdminNav()` in `frontend/app.js` adds the Admin nav button only
  when the authenticated user has `role === 'admin'`. Non-admin accounts never see admin routes.
- **Themed error page** — `error_page()` in `src/routes.rs` returns a styled HTML error response
  for unknown routes. The SPA fallback was removed; `frontend/index.html` is rendered with the
  public path prefix derived from `server.base_url`, so reverse-proxy mounts such as
  `https://server.tld/feralctf/` emit `/feralctf/style.css` and `/feralctf/app.js`.
- **Challenge card layout** — `challengeCard()` updated with `.card-top / .card-bottom` structure,
  difficulty dot, category color, solve count, and solved marker.
- **Category filter pills** — `.cat-pill` buttons replace the old category `<select>` element.
- **Scoreboard polish** — rank medals (🥇🥈🥉), progress bar, current-team highlight.
- **Brand icon** — `images/feral10.jpg` (pixel-art squirrel) rendered in the topbar via `.brand-icon`.
- **Layout** — topbar, nav, auth-form, user-info, admin sidebar, and card grid CSS updated to
  match the §12 specification mockups.
- **Reverse-proxy base path support** — `src/routes.rs` derives a normalized path prefix from
  `Config.server.base_url`, injects it into the frontend shell as a CSP-safe
  `feralctf-base-path` meta tag, and also accepts prefixed API/static requests when a proxy
  forwards the mount path unchanged. `frontend/app.js` uses this prefix for app-owned URLs
  including API calls, WebSocket `/ws`, the brand image, and challenge file downloads; it can
  also infer the prefix from the loaded `/feralctf/app.js` script URL. `index.html`, `app.js`,
  and `style.css` are served with `Cache-Control: no-cache` to revalidate prefix-aware frontend
  assets after deploys.

### Backend

- **`GET /api/admin/challenges`** — added to `src/handlers/admin.rs` using `Challenge::list_all()`.
  The existing player endpoint (`GET /api/challenges`) uses `Challenge::list_visible()` and is
  unchanged. Admin UI now fetches from the admin endpoint so hidden challenges are visible.
- **Route wiring** — `src/routes.rs` wires `list_admin_challenges` on `GET /api/admin/challenges`
  (previously only `POST` was registered on that path).

### Bug Fixes + Polish (post-rc5)

#### Backend fixes

- **Teamless users can browse challenges** — `list_challenges` and `get_challenge` in
  `src/handlers/challenges.rs` previously called `require_team_id()`, returning HTTP 400 for any
  user without a team (including fresh admin accounts). Both handlers now use
  `user.team_id.unwrap_or(0)`; team 0 never exists so `solved_by_team` is always `false` for
  teamless users, which is correct.

#### Frontend fixes

- **Challenge edit modal** — `openEditChallengeModal(challenge)` and `updateChallenge(event, id)`
  added to `frontend/app.js`. An Edit button appears in each admin challenge row. The modal
  pre-fills title, category, points, description, and visibility. Flag field is optional (blank
  keeps the existing hash). Calls `PUT /api/admin/challenges/{id}`.
- **New challenge defaults** — `createChallenge` sends `is_hidden: true` by default (hidden until
  published) and `flag_case_sensitive: false` (case-insensitive) by default.
- **Visibility toggle sync** — `toggleChallengeVisibility()` now awaits `loadChallenges()` before
  re-rendering, so the player challenge view updates immediately without a page refresh.
- **URL rendering in descriptions** — `renderDescription(text)` escapes non-URL content and wraps
  `https?://…` patterns in `<a href="…" target="_blank" rel="noopener noreferrer">` links.
  Used in the challenge detail modal.
- **Description textarea** — `rows="6"` and `min-height: 120px; resize: vertical` applied to both
  create and edit forms.
- **Flag input placeholder** — changed from `feralctf{...}` to `FLAG{...}`.
- **Empty section messages removed** — "No files attached." and "No hints available." placeholder
  text removed from the challenge detail modal; empty lists render nothing.

#### Documentation

- **Architecture spec duplicate row** — `FeralCTF_Architecture_Spec_v2.docx` section 2.1 had two
  `rusqlite` rows (version 0.7.x and 0.31.x with inaccurate descriptions). Both merged into one
  correct row (`rusqlite 0.32.x / SQLite queries, migrations, backup`). PDF regenerated from the
  patched docx via LibreOffice headless.

### Session 2 improvements

#### Backend (session 2)

- **Invite code upgraded to UUID v4** — `generate_invite_code()` in `src/models/team.rs` replaced
  with `uuid::Uuid::new_v4().to_string()`. The `uuid = { version = "1", features = ["v4"] }` crate
  was already in `Cargo.toml`. Team model test updated: `invite_code.len() == 36`.

#### Frontend (session 2)

- **Solved challenges hidden from player grid** — `filteredChallenges()` in `frontend/app.js` now
  excludes any challenge with `solved_by_team === true`. The filter runs client-side on the cached
  list; no backend change was needed.
- **Empty file/hint containers suppressed** — `openChallenge()` wraps `.file-list` and `.hint-list`
  in a conditional: the container div is only emitted when the array is non-empty, eliminating the
  ghost margin from empty CSS grid wrappers.
- **No-team profile — create / join team** — `renderProfile()` detects `!state.user.team_id` and
  renders a two-column "Join or Create a Team" panel. `createTeam()` calls `POST /api/teams`;
  `joinTeam()` calls `POST /api/teams/join`. After success, `state.user` is re-fetched from
  `GET /api/auth/me`, challenges and scoreboard reload, and the profile re-renders.
- **Team invite code display with copy** — When the user has a team, the profile shows the invite
  code in a styled `<code class="invite-code">` element with a Copy button that writes to the
  clipboard via `navigator.clipboard.writeText()`. CSS classes `.invite-row` and `.invite-code`
  added to `frontend/style.css`.
- **Admin flag mouseover in edit modal** — The flag `<input>` in `openEditChallengeModal()` now
  carries a `title` attribute: `"Pattern: <regex>"` for regex challenges, `"Hash: <hash>"` for
  static ones. A `<small>` note below the field tells the admin to hover. No API change needed;
  `GET /api/admin/challenges` already returns the full `Challenge` struct with `flag_hash` and
  `flag_type`.
- **Topbar score refresh after correct solves** — `loadScoreboard()` now uses `setScoreboard()` to
  refresh `state.scoreboard` and call `updateAuth()` together. Successful flag submissions and
  `score_update` WebSocket events update the authenticated user's topbar score immediately without
  a forced browser refresh. No backend or API change needed.

---

*End of sprint definitions.*
*FeralCTF — Apache 2.0 · CyberSquirrels CTF Team*
