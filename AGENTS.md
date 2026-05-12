# AGENTS.md

Project guidance for AI coding agents working on FeralCTF.

## Source Of Truth

Read these before coding:

1. `FERALCTF_SPEC.md` - immutable technical specification.
2. `FERALCTF_SPRINTS.md` - detailed sprint requirements.
3. `FERALCTF_SPRINT_CARDS.md` - sprint acceptance criteria.
4. `FERALCTF_SPRINTS.state` - current sprint progress.
5. `CLAUDE.md` and `.clinerules` - local agent behavior rules.

Do not edit spec files unless explicitly instructed:

- `FERALCTF_SPEC.md`
- `FERALCTF_SPRINTS.md`
- `FERALCTF_SPRINT_CARDS.md`

## Project Rules

- Do not add dependencies without approval.
- Do not use `sqlx`; this project uses `rusqlite` and `r2d2_sqlite`.
- No `unwrap()` in non-test code.
- Keep changes surgical.
- Fix compilation errors before moving to the next task.
- Match sprint interfaces exactly.
- Prefer minimal working implementation over speculative abstraction.
- Run verification before declaring work done.

Required verification for sprint work:

```bash
cargo fmt
cargo check
cargo test
cargo clippy --all-targets --all-features
```

## Sprint Workflow

Before implementing a sprint:

1. Read the sprint section in `FERALCTF_SPRINTS.md`.
2. Read the matching card in `FERALCTF_SPRINT_CARDS.md`.
3. Inspect existing code for drift.
4. State blockers or inconsistencies.
5. Implement only the current sprint scope.
6. Add focused tests where risk justifies it.
7. Run full verification.
8. Update `FERALCTF_SPRINTS.state` only after verification passes.

## Model Usage

Use local or smaller models for:

- Simple compile fixes.
- Mechanical refactors.
- Formatting.
- Straightforward tests.
- Searching code and summarizing local files.

Use frontier models for:

- Architecture decisions.
- Security-sensitive work: auth, sessions, flags, admin controls.
- Cross-module changes.
- Confusing compiler or borrow-checker failures.
- Reviewing sprint completion against the spec.

When using multiple agents or models:

- Assign disjoint files or responsibilities.
- Do not let agents overwrite each other's work.
- Require each agent to report changed files and verification commands.
- One coordinator should integrate and run final verification.

## Architecture Notes

- SQLite schema lives in `migrations/001_initial.sql`.
- The DB layer uses `src/db/mod.rs` with `DbPool = r2d2::Pool<SqliteConnectionManager>`.
- `models::scoreboard::ScoreboardState` is the canonical scoreboard model.
- `cache::ScoreboardState` is not canonical for Sprint 4+.
- Flags must never be exposed through public challenge types.
- Flags are stored as salted hashes, not plaintext.
- Auth uses Argon2id and HS256 JWTs with server-side session revocation.

## Editing Discipline

- Do not refactor unrelated code.
- Do not clean up old stubs unless required by the sprint or compilation.
- Preserve unrelated user changes.
- Prefer parameterized SQL with `rusqlite::params!`.
- Avoid SQL string interpolation. Static SQL constants are preferred.
- Keep public API names aligned with sprint docs.

## Done Means

A task is not done until:

- It matches the sprint acceptance criteria.
- `cargo check` is clean.
- Relevant tests pass.
- `cargo clippy --all-targets --all-features` is clean.
- No sensitive data leaks through public response structs.
