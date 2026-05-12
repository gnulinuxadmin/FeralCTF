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

Current state file should read:

```text
SPRINT 6 DONE
DONE_COUNT: 7
TOTAL_SPRINTS: 14
SPRINTS_REMAINING: 7
```

Next sprint:

- Sprint 7 - Scoreboard + Cache

## Verified Baseline

Last known clean commands:

```bash
cargo check
cargo test
cargo clippy --all-targets --all-features
```

Last known test count:

```text
26 passed
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
- `src/anticheat.rs` has the Sprint 6 `check_rate_limit` hook; full enforcement remains Sprint 11.

## Known Cautions

- Do not revive old single-connection database abstractions.
- Do not add frontend build tooling.
- Do not store plaintext flags.
- Do not expose flag hashes or salts in public responses.
- Do not edit spec files.
- Do not advance `FERALCTF_SPRINTS.state` until verification passes.
