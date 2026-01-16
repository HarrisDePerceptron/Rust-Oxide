# Repository Guidelines

## Project Structure & Module Organization
- `src/main.rs`: bootstraps config, state, logging, uses SeaORM's entity-first schema sync, and mounts the router.  
- `src/config.rs`: env-driven settings (HOST, PORT, JWT_SECRET, RUST_LOG, DATABASE_URL, DB_MAX_CONNS, DB_MIN_IDLE).  
- `src/state.rs`: shared `AppState` (JWT keys, SeaORM `DatabaseConnection`).  
- `src/auth/`: JWT helpers (`jwt.rs`), password hashing (`password.rs`), and role gate layer (`role_layer.rs`) with shared `Claims`/`Role` types.  
- `src/db/entities/`: SeaORM entities (one module per entity).  
- `src/db/dao/`: data access objects for queries/transactions.  
- `src/services/`: business logic that orchestrates DAOs.  
- `src/routes/`: feature routers — `public.rs`, `auth.rs` (register/login/refresh), `protected.rs` (/me), `admin.rs` (/admin/stats); merged in `routes/mod.rs`.  
- `src/error.rs`: consistent JSON error responses; `src/logging.rs`: tracing setup.  
- `examples/client.rs`: Reqwest demo hitting the API; uses `BASE_URL`, `USERNAME`, `PASSWORD`.  
- `views/`: HTML templates rendered with Askama (e.g., index page).  
- `target/`: build artifacts (ignore in diffs).

## Build, Test, and Development Commands
- `cargo run` — start the server on `0.0.0.0:3000` with trace logging; uses SeaORM's entity-first schema sync automatically.  
- `cargo test` — run unit/integration tests (add tests under `tests/` or alongside modules; DB-backed tests currently `#[ignore]` until a real Postgres is wired).  
- `cargo fmt` — format with Rust 2024 defaults; run before opening a PR.  
- `cargo clippy --all-targets --all-features` — lint for correctness and style.  
- `cargo run --example client` — exercise the API; override `BASE_URL`, `USERNAME`, `PASSWORD` env vars as needed.

## Coding Style & Naming Conventions
- Rust 2024 edition; keep `rustfmt` defaults and fix clippy warnings.  
- Files/modules: `snake_case`; types/traits: `PascalCase`; functions/vars: `snake_case`; constants: `SCREAMING_SNAKE_CASE`.  
- Prefer `Result<T, anyhow::Error>` for new fallible code; use `?` to bubble errors.  
- Keep middleware/state thin; load secrets from env, not source.
- Axum route params must use `{param}` segments (e.g., `/todo/{id}`), not `:param`.

## Testing Guidelines
- Add fast, isolated tests under `tests/` or in the same module with `#[cfg(test)]`.  
- Favor integration tests that hit Axum routes with hyper/axum testing utilities; seed JWTs with short expirations.  
- Name tests descriptively (`handles_missing_auth`, `rejects_wrong_role`).  
- Run `cargo test` and `cargo clippy` before pushing; aim to keep new tests under ~1s each.

## Commit & Pull Request Guidelines
- Repo has no commit history yet; use Conventional Commits (`feat:`, `fix:`, `chore:`, `docs:`) with ≤72-char subject lines.  
- Reference issues in the body (`Refs #123`) and note breaking changes explicitly.  
- PRs should include: what changed, why, how to verify (commands or curl examples), and any screenshots/log snippets for API responses.  
- Keep PRs small and focused; ensure CI (fmt, clippy, tests) is green before requesting review.

## Security & Configuration Tips
- Set a strong `JWT_SECRET` (required in release); defaults only in debug.  
- Provide `DATABASE_URL` for Postgres; tune `DB_MAX_CONNS`/`DB_MIN_IDLE` per environment.  
- Bind to `127.0.0.1:3000` by default; override with `HOST`/`PORT`.  
- Keep secrets out of git; use `.env` (gitignored) or a secrets manager.

## Database & ORM Standard
- Use SeaORM v2.x for all database access; keep entities in `src/db/entities` and queries in `src/db/dao`.  
- Sync schema from entities on startup (entity-first, current behavior).  
- Passwords hashed with Argon2 (min length 8); auth uses access/refresh tokens backed by Postgres.

## Layered Workflow
- Routes call services only; keep handlers thin and HTTP-focused.  
- Services hold business logic and can orchestrate multiple DAOs.  
- DAOs contain all SeaORM queries/transactions; entities stay schema-only.  
