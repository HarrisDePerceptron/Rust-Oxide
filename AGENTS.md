# Repository Guidelines

## Project Structure & Module Organization
- `src/main.rs`: Axum entrypoint; sets up public, JWT-protected, and role-gated routes, plus middleware and logging.  
- `src/role_layer.rs`: Custom `RequireRole` tower layer and shared JWT `Claims` extractor.  
- `src/lib.rs`: Re-exports the role layer for downstream use.  
- `examples/client.rs`: Minimal Reqwest client that logs in and calls the protected routes.  
- `target/`: Build artifacts (ignore in diffs).  
- Config lives in `Cargo.toml`; change the demo JWT secret in `main.rs` before shipping.

## Build, Test, and Development Commands
- `cargo run` — start the server on `0.0.0.0:3000` with trace logging.  
- `cargo test` — run unit/integration tests (add tests under `tests/` or alongside modules).  
- `cargo fmt` — format with Rust 2024 defaults; run before opening a PR.  
- `cargo clippy --all-targets --all-features` — lint for correctness and style.  
- `cargo run --example client` — exercise the API; override `BASE_URL`, `USERNAME`, `PASSWORD` env vars as needed.

## Coding Style & Naming Conventions
- Rust 2024 edition; keep `rustfmt` defaults and fix clippy warnings.  
- Files and modules: `snake_case`; types and traits: `PascalCase`; functions/vars: `snake_case`; constants: `SCREAMING_SNAKE_CASE`.  
- Prefer `Result<T, anyhow::Error>` for new fallible code and bubble errors with `?`.  
- Keep middleware/state lightweight; avoid storing secrets in source—pull from env when possible.

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
- Replace the demo secret (`super-secret-change-me`) with a strong key via env var before deploying.  
- Run locally with minimal privileges; bind to `127.0.0.1` when not containerized.  
- Avoid checking tokens or secrets into git; prefer `.env` (untracked) or a secrets manager.
