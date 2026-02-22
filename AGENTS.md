# Workspace Repository Guidelines

## Scope & Agent File Precedence
- This root file applies to the entire workspace.
- If a subdirectory has its own `AGENTS.md`, that file is authoritative for that subtree.
- Current sub-guides:
  - [`crates/server/AGENTS.md`](crates/server/AGENTS.md) (server-template crate guidance; `src/` focused).
- Crates without a local `AGENTS.md` follow this root guide.

## Workspace Structure
- Workspace manifest: `Cargo.toml`.
- Members:
  - `crates/server` (`rust-oxide`): Axum + SeaORM backend template.
  - `crates/companion_cli` (`rust-oxide-cli`, binary `oxide`): project/API scaffolding CLI.
  - `crates/base_entity_derive` (`base_entity_derive`): proc-macro crate used by server entities.
- Default workspace member: `crates/server`.

## Build, Test, and Dev Commands (workspace root)
- `cargo check --workspace`
- `cargo test --workspace`
- `cargo clippy --workspace --all-targets --all-features`
- `cargo fmt`

## Crate-Specific Commands
- Server:
  - `cargo run -p rust-oxide`
  - `cargo test -p rust-oxide`
  - `cargo run -p rust-oxide --example client`
- Companion CLI:
  - `cargo run -p rust-oxide-cli -- --help`
  - `cargo run -p rust-oxide-cli -- init my_app`
  - `cargo run -p rust-oxide-cli -- api add todo_item --fields "title:string,done:bool"`
- Proc macro crate:
  - `cargo check -p base_entity_derive`
  - `cargo test -p base_entity_derive`

## Coding Style & Naming
- Rust edition: 2024.
- Keep `rustfmt` defaults; fix clippy warnings instead of suppressing them unless justified.
- Naming: modules/files `snake_case`, types/traits `PascalCase`, constants `SCREAMING_SNAKE_CASE`.
- Keep crate boundaries clean:
  - `crates/server`: HTTP concerns in routes/middleware, business logic in services, DB in DAOs.
  - `crates/companion_cli`: argument parsing in `cli.rs`, command behavior in `init/`, `add_api/`, `api_remove/`.
  - `crates/base_entity_derive`: macro parsing/expansion only; emit clear compile errors for invalid attributes.

## Testing Guidelines
- Prefer fast, isolated tests close to the code they validate.
- For workspace changes, run targeted package tests first, then `cargo test --workspace`.
- For server DB-backed tests, keep `#[ignore]` where external Postgres is required.
- For CLI changes, test both success and safety paths (`--dry-run`, `--force`, and refusal behavior).

## Change Management
- Use Conventional Commits (`feat:`, `fix:`, `chore:`, `docs:`) with concise subjects.
- Keep PRs focused by crate or feature slice.
- In PR descriptions, include:
  - what changed,
  - why it changed,
  - how to verify (exact commands).

## Security & Config
- Never commit secrets or real credentials.
- Keep environment-specific values in `.env` (gitignored) or a secret manager.
- Server-specific auth/DB/security rules live in `crates/server/AGENTS.md`.
