# FeralCTF — Sprint Cards

> Reference: FERALCTF_SPEC.md (immutable — do not modify)
> Reference: Cargo.toml (do not add dependencies without approval)
> Rule: No sqlx. No unwrap() in non-test code. cargo check must pass clean.

---

## SPRINT-0 — Project Scaffold

**Goal:** Compilable skeleton. No logic. Structure only.

**Tasks:**

- Create all source files listed in SPEC section 9.3
- Declare all modules in their parent mod.rs
- Create empty frontend/ placeholder files
- Verify `cargo check` passes with zero errors and zero warnings

**Done when:** `cargo check` is clean.

---

## SPRINT-1 — Database Schema + Connection Pool

**Goal:** SQLite layer up and running.

**Tasks:**

- Write `migrations/001_initial.sql` — full schema per SPEC section 3.1
- Implement `db::init_pool()` — r2d2 + r2d2_sqlite, WAL mode on connect
- Implement `db::run_migrations()` — idempotent, safe to re-run
- Write unit test: migration runs twice without error

**Done when:** Pool initializes, WAL is set, migration is idempotent.

---

## SPRINT-2 — Config Loading

**Goal:** Load config.toml, env var overrides, sane defaults.

**Tasks:**

- Implement all Config structs per SPEC section 7
- Implement `config::load()` — file → defaults → env var overrides
- Auto-generate jwt_secret if empty
- Create attachments dir if missing
- Implement `config::generate_example()` — writes config.example.toml
- Unit tests: defaults load cleanly, FERALCTF_SERVER_PORT env var overrides port

**Done when:** Config loads from file or pure defaults. Env override works.

---

## SPRINT-3 — Auth Crypto

**Goal:** Password hashing, JWT, session management, unified error type.

**Tasks:**

- Implement `AppError` in errors.rs — all variants per SPEC section 4, implements `IntoResponse`
- Implement `auth::hash_password()` and `auth::verify_password()` — Argon2id
- Implement `auth::hash_flag()` and `auth::verify_flag()` — sha256 + salt, case-insensitive
- Implement `auth::sign_jwt()` and `auth::verify_jwt()`
- Implement `auth::create_session()`, `revoke_session()`, `is_session_valid()`, `cleanup_expired_sessions()`
- Unit tests: password round-trip, wrong password returns false, JWT claims correct, expired JWT errors, flag hash is case-insensitive

**Done when:** All auth primitives tested and passing.

---

## SPRINT-4 — Models

**Goal:** Rust structs for all DB entities with DB helper methods.

**Tasks:**

- Implement User, RegisterRequest, LoginRequest, LoginResponse, UserPublic in models/user.rs
- Implement Team in models/team.rs — invite_code is random 8-char alphanumeric
- Implement Challenge, ChallengePublic, Hint, ChallengeFile, Submission in models/challenge.rs
- Implement TeamScore, ScoreboardState in models/scoreboard.rs
- All DB helpers use parameterized queries — no string interpolation
- ChallengePublic must never expose flag_hash or flag_salt

**Done when:** cargo check clean. No flag data leaks through public types.

---

## SPRINT-5 — Auth Handlers

**Goal:** Registration, login, logout, /me, password change endpoints.

**Tasks:**

- Define AppState in main.rs (db, config, cache stub, ws_hub stub)
- Implement POST /api/auth/register — first user becomes admin, validate username + password
- Implement POST /api/auth/login — verify password, create session, return JWT
- Implement POST /api/auth/logout — revoke session
- Implement GET /api/auth/me — return current user
- Implement PUT /api/auth/password — change password
- Wire routes into Axum router

**Done when:** Register → login → /me → logout → /me returns 401.

---

## SPRINT-6 — Challenge Handlers + Flag Submission

**Goal:** Challenge browsing and flag submission working end to end.

**Tasks:**

- Implement GET /api/challenges — list visible challenges with per-team solve status
- Implement GET /api/challenges/:id — detail, hints (unlocked content only), files
- Implement POST /api/challenges/:id/submit — per SPEC section 4.2 and 6.5
- Implement POST /api/challenges/:id/hints/:hid/unlock — deduct points, record unlock
- Implement `scoring::dynamic_points()` formula per SPEC section 3.2
- Implement `scoring::recalculate_challenge_points()` — retroactive decay on new solve
- Implement `anticheat::check_rate_limit()` stub (full impl in Sprint 11)
- Record every submission in submissions table
- Insert score_history entry on correct solve

**Done when:** Correct flag scores points. Wrong flag scores nothing. Already-solved returns gracefully.

---

## SPRINT-7 — Scoreboard + Cache

**Goal:** Cached scoreboard endpoint and score graph data.

**Tasks:**

- Implement AppCache in cache.rs — RwLock-wrapped scoreboard and challenge list
- Implement `get_or_build_scoreboard()` and `get_or_build_challenges()`
- Implement cache invalidation methods
- Implement GET /api/scoreboard — served from cache
- Implement GET /api/scoreboard/graph — time-series from score_history
- Implement GET /api/teams/:id — team profile + solve history
- Implement POST /api/teams — create team
- Implement POST /api/teams/join — join by invite_code
- Spawn background Tokio task: snapshot scores to score_history every 5 minutes
- Wire cache invalidation into Sprint 6 submission handler

**Done when:** Scoreboard served from cache. Correct submission invalidates cache. Graph returns time-series data.

---

## SPRINT-8 — WebSocket Hub

**Goal:** Real-time event broadcast to all connected clients.

**Tasks:**

- Define WsEvent enum — NewSolve, Announcement, StateChange, ScoreUpdate
- Implement WsHub with tokio::sync::broadcast channel (capacity 256)
- Implement GET /ws upgrade handler — subscribe, forward events as JSON text frames
- Ping clients every 30s, silently drop lagged/disconnected receivers
- Add WsHub to AppState
- Wire NewSolve + ScoreUpdate broadcasts into Sprint 6 submission handler

**Done when:** Flag submission triggers WS event visible to connected client. Disconnect does not panic.

---

## SPRINT-9 — Admin Handlers

**Goal:** Admin CRUD, competition controls, backup.

**Tasks:**

- Implement require_admin middleware — checks JWT role, returns 403 if not admin
- Implement POST/PUT/DELETE /api/admin/challenges — slug auto-generated, flag hashed before storage
- Implement GET /api/admin/submissions — paginated, filterable
- Implement GET /api/admin/users + POST /api/admin/users/:id/ban
- Implement GET /api/admin/teams + POST /api/admin/teams/:id/disqualify
- Implement POST /api/admin/competition/start|end|freeze — broadcasts StateChange WS event
- Implement POST /api/admin/announce — inserts announcement, broadcasts WS event
- Implement GET /api/admin/backup — streams SQLite file via rusqlite online backup API
- Invalidate challenge cache on challenge create/update/delete

**Done when:** Non-admin JWT returns 403. Challenge flag is never stored plaintext. Backup downloads a valid SQLite file.

---

## SPRINT-10 — Import / Export

**Goal:** Full game JSON export and import. CTFd compatibility.

**Tasks:**

- Define ExportBundle and all export structs per SPEC section 5.1
- Implement `import_export::export()` — inline and zip attachment modes
- Implement GET /api/admin/export endpoint
- Define ImportOptions, ImportResult structs per SPEC section 5.2
- Implement `import_export::import()` — wrapped in transaction, conflict resolution per SPEC table
- Implement `import_export::detect_and_convert_ctfd()` — auto-detect CTFd format
- Implement POST /api/admin/import endpoint — supports dry_run query param
- Implement `feralctf import` CLI subcommand

**Done when:** Export → import round-trips without data loss. dry_run writes nothing. CTFd ZIP is detected and converted.

---

## SPRINT-11 — Anti-Cheat + Rate Limiting

**Goal:** Sliding window rate limit, exponential backoff, flag sharing detection.

**Tasks:**

- Implement RateLimiter struct in anticheat.rs — DashMap-backed, no DB required
- Implement sliding window: N submissions per minute per team
- Implement exponential backoff: doubles wait per wrong attempt past threshold
- Implement `check_flag_sharing()` — queries submissions table, logs alert on match
- Add RateLimiter to AppState
- Wire check_submission() and record_attempt() into Sprint 6 handler
- Spawn background GC task — cleans expired windows every 60s
- Rate limited responses include Retry-After header

**Done when:** 429 returned after limit exceeded. Backoff doubles correctly. GC runs without blocking.

---

## SPRINT-12 — Frontend SPA

**Goal:** Vanilla JS SPA embedded into binary via rust-embed.

**Tasks:**

- Implement Challenges view — card grid, category filter, search, solved markers, flag submit modal
- Implement Scoreboard view — ranked table, progress bars, current team highlight, WS live updates
- Implement Profile view — stats, solve history
- Implement Admin view — sidebar nav, overview stats, submission log, challenge/user/team tables, settings
- Store JWT in sessionStorage
- WebSocket: connect on load, reconnect with exponential backoff
- No external CDN dependencies
- Wire rust-embed into main.rs — serve frontend on all non-/api paths
- Dark terminal theme per SPEC section 12.3

**Done when:** cargo build produces single binary. `/` loads challenges view. Flag submit
works in browser. Scoreboard updates live.

---

## SPRINT-13 — CLI + Hardening

**Goal:** CLI subcommands, security headers, audit log, release build.

**Tasks:**

- Implement `feralctf init` — generates config.toml, creates ctf.db, creates attachments/
- Implement `feralctf migrate` — runs migrations on existing DB
- Add HTTP security headers via Tower middleware — X-Content-Type-Options, X-Frame-Options, Referrer-Policy, CSP
- Configure CORS via tower-http CorsLayer
- Add audit_log table to schema (migration 002)
- Implement `db::audit()` helper
- Call audit() from all admin handlers
- Verify `cargo build --release` produces single binary under 20 MB
- Verify RSS under 50 MB at idle

**Done when:** `feralctf init` bootstraps a working install. Security headers present on all
responses. Release binary is self-contained.

---

---

## POST-SPRINT — v1.0rc UI + Admin Fixes

**Status:** Complete (v1.0rc5)

**Scope:** Improvements made after Sprint 13. No new sprint number assigned.

**Tasks completed:**

- Web registration form — `showRegisterModal()` and `registerUser()` in `frontend/app.js`
- Admin nav hidden from non-admin accounts — `updateAdminNav()` in `frontend/app.js`
- Themed error page for unknown routes — `error_page()` in `src/routes.rs`; SPA fallback removed
- Absolute asset paths in `frontend/index.html` (prevents resolution failures from non-root paths)
- `GET /api/admin/challenges` endpoint — `list_admin_challenges` in `src/handlers/admin.rs` using `Challenge::list_all()`
- Route wired in `src/routes.rs` (`list_admin_challenges` on `GET /api/admin/challenges`)
- Admin challenge visibility toggle — `toggleChallengeVisibility()` via existing `PUT /api/admin/challenges/{id}`
- Challenge card layout — `.card-top / .card-bottom`, difficulty dot, category color, solve count
- Category filter pills replacing `<select>` element
- Scoreboard medals, progress bar, current-team highlight
- Brand icon (`images/feral10.jpg`) in topbar
- CSS layout — topbar grid, nav pill styles, admin sidebar, card grid updated to match §12 mockups
- Teamless users can browse challenges — `list_challenges` / `get_challenge` use `unwrap_or(0)` instead of `require_team_id()`
- Challenge edit modal — `openEditChallengeModal()` / `updateChallenge()` in `frontend/app.js`;
  Edit button in each admin challenge row
- New challenge defaults — `is_hidden: true` and `flag_case_sensitive: false`
- Visibility toggle syncs player view immediately — `toggleChallengeVisibility()` awaits `loadChallenges()`
- `renderDescription()` — renders URLs as `<a target="_blank" rel="noopener noreferrer">` hyperlinks in challenge modal
- Description textarea — `rows="6"`, `min-height: 120px`, `resize: vertical` on create and edit forms
- Flag input placeholder changed from `feralctf{...}` to `FLAG{...}`
- Empty section messages removed — "No files attached." / "No hints available." no longer shown in challenge modal
- Architecture spec duplicate `rusqlite` row fixed in `FeralCTF_Architecture_Spec_v2.docx` §2.1; PDF regenerated
- Solved challenges hidden from player grid — `filteredChallenges()` filters out `solved_by_team === true`
- Empty `.file-list` / `.hint-list` divs suppressed — containers only rendered when arrays are non-empty
- No-team profile UI — `renderProfile()` shows create/join team forms when `!state.user.team_id`;
  `createTeam()` calls `POST /api/teams`, `joinTeam()` calls `POST /api/teams/join`; state refreshed
  from `GET /api/auth/me` after success
- Team invite code shown in profile — `.invite-code` + Copy button (`navigator.clipboard.writeText`)
- Invite code upgraded to UUID v4 — `generate_invite_code()` uses `uuid::Uuid::new_v4().to_string()`;
  team test updated to check `invite_code.len() == 36`
- Admin flag mouseover in edit modal — flag `<input>` carries `title` showing stored value
  (`"Pattern: …"` for regex, `"Hash: …"` for static); `<small>` hint visible below field

**Done when:** `cargo test` passes (50/50). All features exercised in browser.

---

FeralCTF — Apache 2.0 · CyberSquirrels CTF Team
