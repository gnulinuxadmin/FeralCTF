# MEMORY.md

Persistent project memory for agents working on FeralCTF.

## Current Status

Sprints complete:

- Sprint 0 - Project scaffold
- Sprint 1 - Database schema + connection pool
- Sprint 2 - Config loading
- Sprint 3 - Auth crypto
- Sprint 4 - Models
- Sprint 5 - Auth handlers
- Sprint 6 - Challenge handlers + flag submission
- Sprint 7 - Scoreboard + cache
- Sprint 8 - WebSocket hub
- Sprint 9 - Admin console APIs
- Sprint 10 - Import / Export
- Sprint 11 - Anti-Cheat + Rate Limiting

Current state file should read:

```text
SPRINT 11 DONE
DONE_COUNT: 12
TOTAL_SPRINTS: 14
SPRINTS_REMAINING: 2
```

Next sprint:

- Sprint 12 - Frontend SPA

## Verified Baseline

Last known clean commands:

```bash
cargo check
cargo test
cargo clippy --all-targets --all-features
```

Last known test count:

```text
49 passed
```

## Key Implementation Notes

### Sprint 1

- Canonical migrations are in `migrations/001_initial.sql`.
- `src/db/mod.rs` owns the canonical DB pool.
- Use `rusqlite` and `r2d2_sqlite`.
- Do not introduce `sqlx`.

### Sprint 2

- `src/config.rs` implements full `Config`.
- Config loading supports defaults, TOML file, env overrides, JWT secret generation, and attachment dir creation.
- Env vars use `FERALCTF_SECTION_KEY`.

### Sprint 3

- `src/errors.rs` defines `AppError`.
- Error responses serialize as:

```json
{ "error": "..." }
```

- Rate-limited responses serialize as:

```json
{ "error": "rate_limited", "retry_after_seconds": 45 }
```

- Rate-limited responses also include the `Retry-After` header.

- `src/auth.rs` implements:
  - Argon2id password hashing
  - password verification
  - flag hashing
  - flag verification
  - JWT signing and verification
  - session create/revoke/validate/cleanup

Argon2id params must match the spec:

```text
memory = 65536
iterations = 3
parallelism = 2
```

JWT uses HS256.

### Sprint 4

- Model files are schema-aligned:
  - `src/models/user.rs`
  - `src/models/team.rs`
  - `src/models/challenge.rs`
  - `src/models/scoreboard.rs`

- `ChallengePublic` must not expose:
  - `flag_hash`
  - `flag_salt`

- Challenge SQL helpers use static SQL constants, not `format!`.
- `src/lib.rs` re-exports canonical `models::scoreboard::ScoreboardState`, not the cache stub.

## Sprint 5 Notes

Sprint 5 should implement:

- real `AppState`
- register
- login
- logout
- `/me`
- password change
- route wiring

Acceptance flow:

```text
register -> login -> /me -> logout -> /me returns 401
```

Important Sprint 5 details:

- First registered user becomes admin.
- Username validation: 3-32 chars, alphanumeric, `_`, `-`.
- Password minimum: 8 chars.
- Login must verify Argon2 password hash.
- Login must create DB session.
- Logout must revoke DB session.
- `/me` must verify JWT and session validity.
- Handlers should return `Result<Json<T>, AppError>`.

### Sprint 6

- `src/handlers/challenges.rs` implements challenge list/detail, flag submit, and hint unlock.
- Static flags use salted hash verification via `auth::verify_flag`.
- Regex flags are supported with the `regex` crate; the regex pattern is read from `challenge.flag_hash`.
- Every flag submission is recorded in `submissions`.
- Correct submissions insert `solves`, recalculate scores, invalidate the cache stub, and insert `score_history`.
- Already-solved submissions return `correct: false` with message `"already solved"`.
- `src/scoring.rs` implements `dynamic_points`, `recalculate_challenge_points`, and full team-score recalculation from solves minus hint deductions.
- Sprint 6 originally used a placeholder anti-cheat hook; Sprint 11 replaced it with real `RateLimiter` enforcement.

### Sprint 7

- `src/cache.rs` now implements canonical `AppCache` with `RwLock<Option<ScoreboardState>>` and `RwLock<Option<Vec<Challenge>>>`.
- `AppCache::get_or_build_scoreboard()` and `get_or_build_challenges()` serve cached data until invalidated.
- `src/handlers/scoreboard.rs` implements:
  - `GET /api/scoreboard`
  - `GET /api/scoreboard/graph`
  - `GET /api/teams/{id}`
  - `POST /api/teams`
  - `POST /api/teams/join`
- `snapshot_scores()` inserts current team scores into `score_history`.
- `spawn_score_snapshot_task()` provides the 5-minute background snapshot loop for server startup wiring.
- Sprint 6 challenge listing now uses `AppCache::get_or_build_challenges()`.
- Correct submissions and team create/join invalidate the scoreboard cache.

### Sprint 8

- `src/handlers/ws.rs` implements the public WebSocket hub.
- `GET /ws` streams serialized `WsEvent` messages.
- Correct flag submissions broadcast `NewSolve` and `ScoreUpdate`.
- WebSocket sender/receiver work is split so broadcasts do not block socket reads.

### Sprint 9

- `src/handlers/admin.rs` implements admin authorization and admin APIs.
- Admin APIs include dashboard, challenge CRUD, submission log, users, teams, competition controls, announcements, and SQLite backup.
- Admin routes are wired through `require_admin` middleware in `src/routes.rs`.
- Challenge creation/update hashes static flags before storage and stores regex flags as patterns.
- Backup uses SQLite backup API and returns a database download.

### Sprint 10

- `src/import_export.rs` implements `ExportBundle`, `ExportChallenge`, `ImportOptions`, `ImportResult`, and preview structs.
- `import_export::export()` exports competition metadata, categories, challenges, hints, files, tags, and slug-based unlock dependencies.
- Inline attachment export uses base64 for stored files up to the Sprint 10 inline limit.
- `import_export::import()` validates first, supports dry-run, creates/skips/overwrites by slug, and wraps writes in a transaction.
- Import is idempotent for matching bundles.
- Import resolves `unlock_requires` by slug after newly created challenge IDs exist.
- `detect_and_convert_ctfd()` accepts FeralCTF JSON, CTFd-style JSON, and ZIPs containing JSON.
- CTFd dynamic challenges import as hidden for admin review.
- Admin endpoints:
  - `GET /api/admin/export` — JSON export
  - `GET /api/admin/export?attachments=inline` — JSON with base64 files
  - `GET /api/admin/export?attachments=zip` — ZIP containing challenges.json + attachment files
  - `POST /api/admin/import` as multipart upload with `file`, `overwrite`, `dry_run`, and optional `attachments` (ZIP of challenge files extracted to `storage.attachments_path` before import; skipped for dry_run)
- CLI import command:

```bash
feralctf import <file> [--attachments <dir>] [--overwrite] [--dry-run]
```

- Important caveat: the current schema stores static flags only as salted hashes, so existing static flags cannot be exported as plaintext. FeralCTF exports include verifier fields (`flag_hash`, `flag_salt`) to make FeralCTF-to-FeralCTF round-trips lossless. External/plaintext imports still hash flags before storage.

### Sprint 11

- `src/anticheat.rs` implements `RateLimiter`.
- Submission rate limiting is per team with a 60-second sliding window.
- Default submission limit is configured by `rate_limit.submissions_per_minute`.
- Wrong attempts are tracked per `(team_id, challenge_id)`.
- Exponential backoff starts at `rate_limit.backoff_base_seconds` after `rate_limit.wrong_attempts_before_backoff`.
- Correct submissions clear the wrong-attempt backoff for that team/challenge.
- Rate-limited responses return HTTP 429 with a `Retry-After` header and `retry_after_seconds` JSON field.
- `AppState` owns `Arc<anticheat::RateLimiter>`.
- `src/main.rs` now starts the Axum server, score snapshot task, and rate limiter GC task.
- `spawn_rate_limiter_gc_task()` runs every 60 seconds and only cleans in-memory limiter state.
- `check_flag_sharing()` detects the same correct submitted flag from another team inside `rate_limit.flag_sharing_window_seconds` and logs a warning only.
- Sprint 11 decision: `migrations/001_initial.sql` includes `idx_submissions_flag_sharing` on `(challenge_id, flag, is_correct, submitted_at)` so flag-sharing detection can use a purpose-built index.

## Known Cautions

- Do not revive old single-connection database abstractions.
- Do not add frontend build tooling.
- Do not store plaintext flags.
- Do not expose flag hashes or salts in public responses.
- Admin export is sensitive; FeralCTF export bundles may contain plaintext regex flags and static flag verifier data.
- Flag-sharing detection is alert-only; do not auto-disqualify teams from this signal.
- Do not edit spec files.
- Do not advance `FERALCTF_SPRINTS.state` until verification passes.
