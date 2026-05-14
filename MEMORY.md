# MEMORY.md

Persistent project memory for agents working on FeralCTF.

## Current Status

Version: **1.0rc5** — all sprints complete, post-sprint UI and admin improvements shipped.

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
- Sprint 12 - Frontend SPA
- Sprint 13 - CLI Subcommands + Hardening
- Post-Sprint (1.0rc) - UI + Admin fixes (see FERALCTF_SPRINTS.md §Post-Sprint 13)

Next sprint: none currently defined.

## Verified Baseline

Last known clean commands:

```bash
cargo check
cargo test
cargo clippy --all-targets --all-features
```

Last known test count:

```text
50 passed
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
- `src/scoring.rs` implements `dynamic_points`, `recalculate_challenge_points`, and full team-score recalculation from
  solves minus hint deductions.
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
- Admin APIs include dashboard, challenge CRUD, submission log, users, teams, competition controls, announcements, and
  SQLite backup.
- Admin routes are wired through `require_admin` middleware in `src/routes.rs`.
- Challenge creation/update hashes static flags before storage and stores regex flags as patterns.
- Backup uses SQLite backup API and returns a database download.

### Sprint 10

- `src/import_export.rs` implements `ExportBundle`, `ExportChallenge`, `ImportOptions`, `ImportResult`, and preview structs.
- `import_export::export()` exports competition metadata, categories, challenges, hints, files, tags, and slug-based
  unlock dependencies.
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
  - `POST /api/admin/import` as multipart upload with `file`, `overwrite`, `dry_run`, and optional `attachments`
    (ZIP of challenge files extracted to `storage.attachments_path` before import; skipped for dry_run)
- CLI import command:

```bash
feralctf import <file> [--attachments <dir>] [--overwrite] [--dry-run]
```

- Important caveat: the current schema stores static flags only as salted hashes, so existing static flags cannot be
  exported as plaintext. FeralCTF exports include verifier fields (`flag_hash`, `flag_salt`) to make FeralCTF-to-
  FeralCTF round-trips lossless. External/plaintext imports still hash flags before storage.

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
- `check_flag_sharing()` detects the same correct submitted flag from another team inside
  `rate_limit.flag_sharing_window_seconds` and logs a warning only.
- Sprint 11 decision: `migrations/001_initial.sql` includes `idx_submissions_flag_sharing` on `(challenge_id, flag,
  is_correct, submitted_at)` so flag-sharing detection can use a purpose-built index.

### Sprint 12

- `frontend/index.html`, `frontend/app.js`, and `frontend/style.css` implement the vanilla JS SPA.
- The SPA includes Challenges, Scoreboard, Profile, and Admin views.
- Challenge cards support category filtering, title search, solved markers, solve counts, point display, and a
  difficulty dot.
- Challenge detail modals load from `GET /api/challenges/{id}`, show files and hints, and submit flags through `POST /api/challenges/{id}/submit`.
- JWTs are stored in `sessionStorage` under `feralctf_token`.
- All authenticated frontend API calls send `Authorization: Bearer <token>`.
- The scoreboard connects to public `GET /ws` and re-renders on `score_update` events without a page reload.
- WebSocket reconnect uses exponential backoff.
- `src/routes.rs` embeds `frontend/` with `rust-embed` and serves the SPA for non-API paths.
- Admin middleware remains scoped to admin routes only; public auth, challenge, scoreboard, team, and WebSocket routes
  are not wrapped by `require_admin`.
- No new dependencies were added for Sprint 12; existing `rust-embed` and `mime_guess` are used.

### Sprint 13

- `src/main.rs` implements CLI parsing with `std::env::args`; no CLI dependency was added.
- Default `feralctf` starts the server.
- `feralctf --port <port>` overrides the configured port for server startup.
- `feralctf --config <path>` loads an alternate config file for server, migrate, and import.
- `feralctf init` writes `config.toml` with a random JWT secret, creates `ctf.db`, and creates `attachments/`.
- `feralctf migrate` runs embedded SQL migrations against the configured database.
- `feralctf import <file> [--dry-run] [--overwrite] [--attachments <dir>]` remains wired through the import/export module.
- Migrations are embedded with `include_str!` in `src/db/mod.rs`, so release binaries do not require a loose
  `migrations/` directory at runtime.
- `migrations/002_audit_log.sql` adds the admin audit table and indexes.
- `db::audit()` inserts admin audit rows.
- Admin mutating handlers record audit rows; explicit verified paths are challenge create, challenge delete, and team disqualify.
- `src/routes.rs` adds security headers on all responses: `X-Content-Type-Options`, `X-Frame-Options`, `Referrer-
  Policy`, and CSP with WebSocket connect support.
- `ServerConfig.allowed_origins` plus `FERALCTF_SERVER_ALLOWED_ORIGINS` configure CORS; default empty CORS config
  keeps same-origin behavior.
- Release verification on this workspace produced an 8.7 MB binary and idle RSS of about 9.5 MB.

### Post-Sprint 13 (v1.0rc)

- `frontend/app.js` adds `showRegisterModal()` and `registerUser()` — web registration form, no backend changes needed.
- `updateAdminNav()` in `frontend/app.js` adds/removes the Admin nav button based on `state.user.role === 'admin'`.
- `error_page()` added to `src/routes.rs`; the `frontend()` fallback no longer serves `index.html` for all unknown paths.
  Instead, unknown paths return a styled HTML error page. The SPA shell handles its own routing on the client.
- `frontend/index.html` asset paths changed to absolute (`/style.css`, `/app.js`) to prevent resolution failures when
  the error page is served from non-root paths.
- `list_admin_challenges` handler added to `src/handlers/admin.rs` using `Challenge::list_all()`. It is wired on
  `GET /api/admin/challenges` in `src/routes.rs` — protected by `require_admin` middleware.
- `renderAdminChallenges()` in `frontend/app.js` now fetches from `GET /api/admin/challenges` (admin endpoint) instead
  of `state.challenges` (populated by the player endpoint which omits hidden challenges).
- `toggleChallengeVisibility()` in `frontend/app.js` calls `PUT /api/admin/challenges/{id}` with
  `{ is_hidden: desired }` and now awaits `loadChallenges()` so the player view syncs without a page refresh.
- `frontend/feral10.jpg` copied from project root so `rust-embed` includes it; rendered as `.brand-icon` in topbar.
- Challenge cards updated: `.card-top / .card-bottom`, difficulty dot (`.easy/.medium/.hard`), category color border,
  solve count, solved marker. Category filter uses `.cat-pill` buttons instead of `<select>`.
- Scoreboard: rank medals for top 3, progress bar column, current-team row highlight via `.current-team`.
- CSS in `frontend/style.css` updated: topbar grid, `.brand` flex, `.nav button` underline-style active state,
  `.user-info / .user-badge` display, `.toolbar / .search-input / .category-pills / .cat-pill`, `.scoreboard-header /
  .live-dot`, admin sidebar `border-left` active style. Responsive breakpoints updated.
- `updateAuth()` and `loginUser()` both call `updateAuth()` after `Promise.all([loadChallenges(), loadScoreboard()])`
  to ensure score display is accurate on initial load and on login.

### Post-Sprint 13 continued (bug fixes + polish)

- **`list_challenges` / `get_challenge` team requirement removed** — Both handlers previously called
  `require_team_id()`, returning 400 for any user without a team (including fresh admin accounts). Fixed to use
  `user.team_id.unwrap_or(0)`; teamless users see all visible challenges with `solved_by_team: false`.
- **Challenge edit modal** — `openEditChallengeModal(challenge)` and `updateChallenge(event, id)` added to
  `frontend/app.js`. Edit button appears in each admin challenge row. Modal pre-fills title, category, points,
  description, and visibility toggle. Flag field is optional (blank = keep existing hash).
- **Challenge create defaults** — New challenges default to `is_hidden: true` (hidden) and
  `flag_case_sensitive: false`. Visibility toggle on the create form lets admin publish immediately.
- **`renderDescription()`** — Added to `frontend/app.js`. Escapes non-URL text, wraps `https?://...` patterns in
  `<a href="..." target="_blank" rel="noopener noreferrer">` links. Used in the challenge detail modal.
- **Description textarea** — `rows="6"` and `min-height: 120px; resize: vertical` in CSS. Applies to both create
  and edit forms.
- **Flag input placeholder** — Changed from `feralctf{...}` to `FLAG{...}`.
- **Empty file/hint messages removed** — "No files attached." and "No hints available." placeholder text removed from
  the challenge detail modal; empty lists render nothing.
- **Architecture spec docx/PDF** — Duplicate `rusqlite` rows in section 2.1 table (0.7.x and 0.31.x) merged into a
  single correct row (`0.32.x / SQLite queries, migrations, backup`). PDF regenerated from docx via LibreOffice.

### Post-Sprint 13 continued (session 2)

- **Solved challenges hidden from grid** — `filteredChallenges()` in `frontend/app.js` now filters
  out challenges where `solved_by_team === true`. Solved challenges disappear from the player grid
  immediately after a correct submission.
- **Empty file/hint containers** — `openChallenge()` now only renders `.file-list` and
  `.hint-list` wrapper divs when the corresponding arrays are non-empty. Previously empty divs
  produced visual whitespace from CSS margins.
- **No-team profile UI** — `renderProfile()` in `frontend/app.js` detects `!state.user.team_id`
  and renders a two-column panel with a Create Team form and a Join Team form. `createTeam()` calls
  `POST /api/teams`; `joinTeam()` calls `POST /api/teams/join`. After either succeeds, `state.user`
  is refreshed from `GET /api/auth/me`, challenges and scoreboard reload, and the profile
  re-renders showing the new team and invite code.
- **Team invite code display** — When the user is on a team, the profile shows a Team panel with
  the invite code in a styled `<code>` element and a Copy button (`navigator.clipboard.writeText`).
- **Invite code upgraded to UUID v4** — `generate_invite_code()` in `src/models/team.rs` now calls
  `uuid::Uuid::new_v4().to_string()` (36-char format). The `uuid` crate with the `v4` feature was
  already in `Cargo.toml`. Team test updated: `invite_code.len() == 36`.
- **Admin flag mouseover** — The flag input in `openEditChallengeModal()` in `frontend/app.js` now
  has a `title` attribute showing the stored value: `"Pattern: <regex>"` for regex-type challenges
  or `"Hash: <hash>"` for static ones. A `<small>` hint below the input tells the admin to hover.
  No backend change needed — `GET /api/admin/challenges` already returns the full `Challenge` struct
  including `flag_hash` and `flag_type`.
- **Topbar score refresh after solves** — `loadScoreboard()` in `frontend/app.js` now routes
  scoreboard updates through `setScoreboard()`, which updates `state.scoreboard` and calls
  `updateAuth()`. Correct flag submissions and `score_update` WebSocket events now refresh the
  topbar score immediately without requiring a browser refresh. No backend/API change needed.

## Known Cautions

- Do not revive old single-connection database abstractions.
- Do not add frontend build tooling.
- Do not store plaintext flags.
- Do not expose flag hashes or salts in public responses.
- Admin export is sensitive; FeralCTF export bundles may contain plaintext regex flags and static flag verifier data.
- Flag-sharing detection is alert-only; do not auto-disqualify teams from this signal.
- Do not edit spec files.
- Do not advance `FERALCTF_SPRINTS.state` until verification passes.
