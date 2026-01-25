# Rust Oxide - Opinionated Backend

A starter for building JSON APIs with Axum and SeaORM, plus a companion
CLI to scaffold new projects and CRUD APIs.

## What is included

- Server with JWT auth, role gates, and protected routes
- Entity-first schema sync on startup
- Layered architecture
- Templates for HTML views (docs, routes, entities)
- Companion CLI (`oxide`) for project scaffolding and API generation

## Project layout (high level)

```
crates/server/            # Main backend template
crates/companion_cli/     # CLI (binary name: oxide)
crates/base_entity_derive/# derive helpers 
```

## Development Quick start (server)

```sh
cargo run -p rust_oxide
```

## CLI (oxide)

### Install from crates.io

```sh
cargo install rust-oxide-cli
```

This installs the `oxide` binary.

### Install without Rust (macOS/Linux)

```sh
curl -fsSL https://raw.githubusercontent.com/HarrisDePerceptron/Rust-Oxide/master/scripts/install.sh | sh
```

Update to the latest version:

```sh
curl -fsSL https://raw.githubusercontent.com/HarrisDePerceptron/Rust-Oxide/master/scripts/install.sh | sh -s -- --update
```

Uninstall:

```sh
curl -fsSL https://raw.githubusercontent.com/HarrisDePerceptron/Rust-Oxide/master/scripts/install.sh | sh -s -- --uninstall
```


### Add/remove a CRUD API

```sh
oxide api add todo_item --fields "title:string,done:bool"
oxide api remove todo_item
```

## Tests

```sh
cargo test -p rust_oxide
```

Note: DB-backed tests are currently `#[ignore]` until a real Postgres is wired.

## Release flow (CLI)

This repo uses release-please to automate versioning and tags for the CLI.
Conventional Commits determine SemVer:

- `feat:` -> minor
- `fix:` -> patch
- `feat!:` or `BREAKING CHANGE:` -> major
- `chore`:  Non source changes

Merging a release-please PR creates a `vX.Y.Z` tag, which triggers the release
workflow to build binaries and publish `rust-oxide-cli` to crates.io.

