# Repository Guidelines

## Project Structure & Module Organization
- `src/main.rs`: bootstraps config, state, logging, and mounts the router.  
- `src/config.rs`: env-driven settings (HOST, PORT, JWT_SECRET, RUST_LOG).  
- `src/state.rs`: shared `AppState` (JWT keys, HTTP client).  
- `src/auth/`: JWT helpers (`jwt.rs`) and role gate layer (`role_layer.rs`) with shared `Claims`/`Role` types.  
- `src/routes/`: feature routers — `public.rs`, `auth.rs` (login), `protected.rs` (/me), `admin.rs` (/admin/stats); merged in `routes/mod.rs`.  
- `src/error.rs`: consistent JSON error responses; `src/logging.rs`: tracing setup.  
- `examples/client.rs`: Reqwest demo hitting the API; uses `BASE_URL`, `USERNAME`, `PASSWORD`.  
- `target/`: build artifacts (ignore in diffs).

## Build, Test, and Development Commands
- `cargo run` — start the server on `0.0.0.0:3000` with trace logging.  
- `cargo test` — run unit/integration tests (add tests under `tests/` or alongside modules).  
- `cargo fmt` — format with Rust 2024 defaults; run before opening a PR.  
- `cargo clippy --all-targets --all-features` — lint for correctness and style.  
- `cargo run --example client` — exercise the API; override `BASE_URL`, `USERNAME`, `PASSWORD` env vars as needed.

## Coding Style & Naming Conventions
- Rust 2024 edition; keep `rustfmt` defaults and fix clippy warnings.  
- Files/modules: `snake_case`; types/traits: `PascalCase`; functions/vars: `snake_case`; constants: `SCREAMING_SNAKE_CASE`.  
- Prefer `Result<T, anyhow::Error>` for new fallible code; use `?` to bubble errors.  
- Keep middleware/state thin; load secrets from env, not source.

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
- Bind to `127.0.0.1:3000` by default; override with `HOST`/`PORT`.  
- Keep secrets out of git; use `.env` (gitignored) or a secrets manager.
