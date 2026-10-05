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
    pub total_visible_points: i64, // SUM(points) for challenges where is_hidden = 0
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
        total_visible_points: i64,
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
- **Favicon** — `frontend/favicon.png` and `frontend/favicon.ico` generated from the approved feral10-based
  favicon preview and referenced from `frontend/index.html` through the rendered base path.
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
- **Admin user/team toggles** — Admin → Users now exposes exclusive Admin and Ban toggle sliders backed by
  `PUT /api/admin/users/{id}/role`; role changes audit `user.role_update`, revoke the target user's sessions,
  and reject removing the last admin. Admin → Teams now exposes a Ban toggle backed by
  `PUT /api/admin/teams/{id}/disqualified`, using existing `is_disqualified` semantics with score recalculation,
  scoreboard cache invalidation, and `team.disqualify` / `team.reinstate` audit actions.
- **Password management UI** — `renderProfile()` now includes a Change Password panel with current
  password, new password, and confirmation fields. Admin → Users includes a Password action that
  opens a modal for manual password assignment.

### Backend

- **`GET /api/admin/challenges`** — added to `src/handlers/admin.rs` using `Challenge::list_all()`.
  The existing player endpoint (`GET /api/challenges`) uses `Challenge::list_visible()` and is
  unchanged. Admin UI now fetches from the admin endpoint so hidden challenges are visible.
- **Route wiring** — `src/routes.rs` wires `list_admin_challenges` on `GET /api/admin/challenges`
  (previously only `POST` was registered on that path).
- **Password session revocation** — `PUT /api/auth/password` now revokes all sessions for the
  user after a successful password change. Admin password assignment is exposed at
  `PUT /api/admin/users/{id}/password`, validates confirmation, hashes with Argon2id, revokes the
  target user's sessions, and audits `user.password_update`.
- **Optional built-in HTTPS mode** — Production deployments should still prefer nginx/Caddy for
  TLS termination, certificate renewal, redirects, caching, request limits, and hosting adjacent
  static files. `[server]` also supports `tls_enabled`, `tls_cert_path`, `tls_key_path`, and
  optional `tls_chain_path`, with matching `FERALCTF_SERVER_TLS_*` environment overrides. When
  enabled, startup serves the existing Axum router through a Rustls-backed listener using the
  supplied PEM certificate, private key, and optional intermediate chain.

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

### Session 3 improvements

#### Backend (session 3)

- **Scoreboard total visible points** — `ScoreboardState` now serializes `total_visible_points`,
  computed as the sum of current `points` for visible challenges (`is_hidden = 0`). This gives the
  frontend a stable denominator for scoreboard progress bars.
- **Score update denominator** — `WsEvent::ScoreUpdate` now includes `total_visible_points` so live
  WebSocket updates preserve the same progress denominator as `GET /api/scoreboard`.

#### Frontend (session 3)

- **Logged-out challenge copy** — `renderChallenges()` shows `Log in to view challenges.` when no
  user session is present. Logged-in users with no visible unsolved challenges still see
  `No visible challenges.`
- **Registration password confirmation** — The browser registration modal includes a second
  password field, and `registerUser()` rejects mismatched passwords before calling
  `POST /api/auth/register`. The backend request shape is unchanged.
- **Scoreboard progress hover** — Scoreboard progress bars now use
  `team.score / total_visible_points`, clamp the visual bar to 0–100%, and expose hover text in
  the form `10%: 100 of 1000 points scored`.
- **Partial scoreboard update hardening** — `setScoreboard()` preserves the previous
  `total_visible_points` if a partial update omits it, preventing live updates from resetting the
  denominator to zero.
- **CSP-safe progress width** — `renderScoreboard()` renders progress widths as data attributes
  and applies them after render with DOM style assignment. This avoids inline `style` attributes
  being blocked by the app's CSP and making every progress bar look full-width.
- **Logo version hover** — The topbar logo image now exposes `Version 1.0` through its native
  mouseover tooltip.
- **Larger logo icon** — `.brand-icon` now renders the topbar logo at 60x60 px.

---

## Sprint 14 — v1.0.2 UI/API Consistency + Bug Fixes

**Status:** Complete (v1.0.2). **Goal:** close gaps between the API and the web UI found in a
full audit, starting from two reports: hints/attachments could not be edited in the admin UI, and
players could not see hints.

### Security

- **Invite code leak** — `GET /api/teams/{id}` needed no login and returned `invite_code`, so
  anyone could join any team. `TeamProfileInfo.invite_code` is now only set for members of that
  team and admins.

### Hints

- Admin CRUD: `GET/POST /api/admin/challenges/{id}/hints`, `PUT/DELETE /api/admin/hints/{id}`
  (`Hint::list_admin/create/update/delete`, `HintAdmin.unlock_count`). Deleting a hint deletes its
  unlocks and recalculates scores (refund). Content 1–4000 chars, cost ≥ 0. Audited
  `hint.create|update|delete`.
- Import overwrite used to delete and re-insert hints, orphaning `hint_unlocks` (teams kept paying
  for deleted hints). `import_export::sync_hints` now upserts by `(challenge_id, sort_order)` so
  hint ids and unlocks survive; removed hints are deleted with their unlocks; scores recalculated.
- `Hint::unlock` uses `INSERT OR IGNORE` and charges only when a row was inserted (double-click race).
- Policy: no unlock after the team solved the challenge (400); no unlock costing more than the
  team score (400); free hints always allowed.
- Player modal: hints labelled by position (`Hint 1 (free)`), confirm with cost and team score,
  unlocked row replaced in place (typed flag kept), content rendered with `renderDescription` and
  `white-space: pre-wrap`, "Join a team to unlock" for teamless users.

### Attachments (URL-only)

- An attachment is a label (`files.filename`) plus an absolute `http(s)://` URL
  (`files.storage_path`). FeralCTF does not host or serve files; the old `/<storage_path>` links
  always returned 404. Validation: `models::challenge::validate_attachment_url`.
- Admin CRUD: `GET/POST /api/admin/challenges/{id}/files`, `PUT/DELETE /api/admin/files/{id}`.
- Import/export: `ExportFile.url`; imports without an absolute URL are kept and reported in
  `attachment_warnings`; zip/inline export skips URL rows.

### Challenges and scoring

- `update_challenge` / `delete_challenge` now recalculate team scores and broadcast `score_update`.
- Delete cascades `hints`, `hint_unlocks`, `files`, `solves` in a transaction (submissions kept)
  and clears `unlock_requires` pointing at the deleted challenge.
- `flag_case_sensitive` is honoured: `auth::hash_flag/verify_flag(.., case_sensitive)`, regex
  via `RegexBuilder::case_insensitive`. Changing flag type to/from regex or case sensitivity
  requires re-entering the flag. Regex patterns are validated.
- `unlock_requires` enforced: `ChallengePublic.locked`; detail, submit and hint unlock return 403
  while locked; locked challenges are listed without a description.
- `UpdateChallengeRequest.author/unlock_requires` use a `double_option` deserializer so `null`
  clears the value (plain serde treated `null` as "keep").
- Admin list returns `AdminChallenge` with `hint_count` / `file_count`.

### Users and teams

- `POST /api/admin/users` (username, password + confirm, role, `team: null | {existing_id} |
  {new_name}`) creates accounts without a session and works when registration is closed.
- `PUT /api/admin/users/{id}/team` reassigns teams, revokes the user's sessions, audits
  `user.team_update`. Solves stay with the team that earned them.
- `registration_open` and `max_team_size` enforced (register, join, admin create/assign).
  Duplicate team names return 400 instead of 500. Admins cannot be banned without demotion first.

### Competition state

- Migration `003_competition_state.sql` + `src/competition.rs`: start/end/freeze are persisted
  and combined with `start_time`, `end_time`, `score_freeze_minutes_before_end`. No row = running.
- Players cannot submit or unlock hints before start/after end (admins exempt).
- During a freeze the public scoreboard, graph, other teams' profiles and WebSocket updates use
  `ScoreboardState::build_as_of(frozen_at)`; admins see live scores.
- Public `GET /api/competition` and `GET /api/announcements`.

### Admin and player UI

- Real Settings page (config read-only, start/freeze/end, announcements, JSON export, DB backup,
  import with dry-run/overwrite and warnings) replacing a form whose Save did nothing.
- Submissions section with filters/pagination; names instead of ids (`SubmissionRecord` joins).
- Users table shows team names; New user and Team modals with a shared team picker.
- Cards show 💡/📎 counts, lock and solved badges; "Show solved" toggle (solved challenges used to
  vanish). Profile shows real hints used / first bloods and hint rows in history.
- WebSocket `announcement`, `new_solve` (first blood) and `state_change` handled; banner for
  not started / ended / frozen. Inline-SVG score graph from `/api/scoreboard/graph`.
- Category colours applied via `data-category-color` (inline styles were blocked by the CSP);
  server error page inline styles moved to `style.css`. Search box keeps focus while typing.
- `DefaultBodyLimit` on `/api/admin/import` from `storage.max_file_size_mb` (axum default 2 MB).
- Version 1.0.2: `Cargo.toml`, `feralctf-version` meta tag injected from `CARGO_PKG_VERSION`.

**Upgrade notes:** static flags marked case sensitive were stored lowercased and must be
re-entered; legacy attachment paths show "link unavailable" until given a URL.

**Acceptance:** `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`
(100 tests at end of sprint); API flow and headless-browser flow verified end to end.

---

## Sprint 15 — Branding (Competition Name + Logo URL)

**Status:** Complete (v1.0.2). **Goal:** the competition name in Admin → Settings was ignored by
the player UI (header hard-coded "Feral CTF"); add a logo from a URL.

- Migration `004_branding.sql`: single-row `branding (name, logo_url, updated_at)`. `NULL` name
  falls back to `competition.name` from config.toml; `NULL` logo shows the built-in `feral10.jpg`.
- `competition::Branding`, `branding()`, `set_branding()`, `Branding::logo_origin()`;
  `CompetitionStatus` carries `name` and `logo_url` (`GET /api/competition`).
- `PUT /api/admin/branding` (`name` ≤ 64 chars or blank, `logo_url` absolute http(s) or blank),
  audited `settings.branding`, broadcasts `state_change` so clients refresh.
- `GET /api/admin/settings` includes `branding`. Export bundles use the branded name.
- `index.html` `<title>{{COMPETITION_NAME}}</title>` rendered server-side (escaped).
- CSP: `img-src 'self' <logo origin>` is added only when a logo is set; branding is cached in
  `AppCache.branding` (invalidated on update) because the CSP is computed on every response.
- Frontend: `applyBranding()` sets header name, logo and `document.title`; a broken logo falls
  back to the built-in one. Settings has a Branding form with live preview. Because a page's CSP
  is fixed at load, saving a logo on a new origin reloads the admin page (returning to Settings);
  players pick up a new logo origin on their next page load (name updates live).

**Acceptance:** 105 tests; browser test verified external logo loads under the CSP, server-rendered
title, player view, fallback for a missing logo, and revert to config name.

---

## Sprint 16 — Reversible Flag Storage for Admin Verification

**Status:** Complete (v1.0.2). **Goal:** store each static/dynamic flag a second time,
encrypted with AES-256, so admins can reveal and verify it. **Runtime checking stays hash-only:**
`verify_submission` never decrypts; the ciphertext exists only for admin verification.

### Design

- **Cipher:** AES-256-GCM (authenticated) via `ring::aead` in `src/flag_cipher.rs`. `ring` 0.17
  was already compiled in through rustls; it is now a direct dependency (approved; no new crate).
- **Key and IV (decided):** a 32-byte AES-256 key and one 96-bit GCM nonce (IV), both random,
  generated once when the database is first created and stored **in the SQLite database**:
  single-row table `flag_cipher (id = 1, key BLOB, nonce BLOB, created_at)`. The row is created by
  `run_migrations` when missing — at `feralctf init` for new installs, and on the first start or
  `feralctf migrate` after upgrading an existing database. It is never regenerated if present.
- **Ciphertext format:** `flag_ciphertext = base64(ciphertext ‖ tag)` encrypted with the shared
  key and nonce.
- **Accepted risk (decided 2026-10-05):** the key, nonce and ciphertexts all live in `ctf.db`, so
  anyone with the database file or an `/api/admin/backup` download can decrypt every flag. Reusing
  one GCM nonce also means identical flags have identical ciphertext and two ciphertexts reveal
  the XOR of their flags. These are game flags that grant no access; anyone with server access has
  bigger problems, and compromised flags are rotated by changing the challenges. The encryption
  only keeps flags from being stored as readable text.
- **Schema:** migration `005_flag_cipher.sql` creates `flag_cipher` and adds
  `challenges.flag_ciphertext TEXT NULL`. `ALTER TABLE ... ADD COLUMN` is not idempotent, so
  `run_migrations` must check `PRAGMA table_info(challenges)` before adding the column.
- **Write paths:** `create_challenge`, `update_challenge` (when a new flag is given), and import
  (when the bundle carries a plaintext `flag`) encrypt alongside hashing. Regex flags are already
  stored as plaintext patterns and are left as they are.
- **Read path:** `GET /api/admin/challenges/{id}/flag` → `{ "flag": "...", "stored": true }` or
  `{ "stored": false }` for legacy rows; admin-only, audited `challenge.flag_reveal`. Ciphertext is
  never included in list, public, or export responses (`Challenge` serialization must skip it).
- **Admin UI:** "Reveal flag" button in the edit modal (replaces the hash tooltip); legacy
  challenges show "not stored — re-enter the flag to enable reveal".
- **Legacy data:** existing challenges have only hashes and cannot be backfilled.
- **Backwards compatibility (decided):** running CTFs must not break. `run_migrations` (run on
  every start and by `feralctf migrate`) adds the table, column and key to existing databases.
  Write paths use `flag_cipher::try_encrypt`: if the key is missing or unusable the flag is stored
  hash-only and a warning is logged, so creating, editing and importing challenges never fails
  because of encryption. Reveal returns `stored: false` for hash-only rows and an `error` message
  (not a 500) when a stored copy cannot be decrypted. Import overwrites keep the existing
  ciphertext only while the flag hash is unchanged, so it can never go stale.

### Open decisions

1. ~~Key location~~ — decided: key and nonce stored in the SQLite database.
2. ~~Nonce strategy~~ — decided: GCM with one nonce created with the database.
3. Exports — kept as before: no plaintext static flags. Ciphertext is never exported, because the
   key is per database. Revisit if portable exports are needed.
4. Key rotation CLI (`feralctf rotate-flag-key`) — deferred.

### Tests / acceptance

- Encrypt → decrypt round trip; tampered ciphertext fails to decrypt (GCM tag check).
- `flag_cipher` row created once: running migrations again keeps the same key and nonce.
- Submissions verify against the hash even when the ciphertext is missing or undecryptable.
- Reveal endpoint: admin-only, audited, `stored: false` for legacy rows; ciphertext absent from
  `GET /api/admin/challenges`, `GET /api/challenges*`, and exports.
- Migration 005 idempotent on new and existing databases.
- Missing key: challenge create/update/import still succeed hash-only; reveal degrades.

**UI tests:** `tests/ui/run_ui_tests.py` drives the player and admin UI with Selenium and Chrome
(pinned in `tests/ui/requirements.txt`, installed into `tests/ui/.venv` on first run; Selenium
Manager downloads Chrome for Testing when Chrome is not installed) and seeds data with
`requests`. Page JavaScript runs via the DevTools protocol so the CSP stays active for the app. It builds the binary, runs it on a free
port with a temporary database, and fails on unexpected browser console errors (including CSP
violations).

**Result:** 116 tests (adds route-guard, pre-Sprint-16 upgrade, frozen WebSocket broadcast, regex
reveal, missing-key and wrong-key tests). Verified a database created by the v1.0.1 binary upgrades
in place: scores and sessions kept, existing flags still solve, competition not locked, legacy
challenges show "not stored" until the flag is re-entered.

---

*End of sprint definitions.*
*FeralCTF — Apache 2.0 · CyberSquirrels CTF Team*
