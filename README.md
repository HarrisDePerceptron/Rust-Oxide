<p align="center">
  <img src="crates/server/public/images/ox-logo.svg" alt="Rust Oxide logo" width="120" />
</p>

# Rust Oxide - Opinionated Backend

A starter for building JSON APIs with Axum and SeaORM, plus a companion
CLI to scaffold new projects and CRUD APIs.

## What is included

- Server with JWT auth, role gates, and protected routes
- Entity-first schema sync on startup
- Layered architecture
- Templates for HTML views (docs, routes, entities)
- Companion CLI (`oxide`) for project scaffolding and API generation
- Optional release docs toggle via `APP_GENERAL__ENABLE_DOCS_IN_RELEASE`

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

## Docker

Build the production image:

```sh
docker build -t rust-oxide-server .
```

Run the container (replace env values with real secrets):

```sh
docker run --rm -p 3000:3000 \
  -e APP_DATABASE__URL='sqlite://app.db?mode=rwc' \
  -e APP_AUTH__JWT_SECRET='change-me' \
  -e APP_AUTH__ADMIN_EMAIL='admin@example.com' \
  -e APP_AUTH__ADMIN_PASSWORD='change-me-123' \
  rust-oxide-server
```

Notes:
- The final image contains only the server binary and `public/` static assets (no Rust source tree).
- Container defaults: `APP_GENERAL__HOST=0.0.0.0`, `APP_GENERAL__PORT=3000`.

### Docker Compose (app + Postgres)

Start everything:

```sh
docker compose up --build -d
```

Follow logs:

```sh
docker compose logs -f app
```

Stop and remove containers:

```sh
docker compose down
```

The compose stack includes:
- `app` (this Axum server)
- `postgres` (persistent DB via `postgres_data` volume)

Useful overrides:
- `APP_PORT` (default `3000`)
- `POSTGRES_PORT` (default `5432`)
- `APP_AUTH__JWT_SECRET`
- `APP_AUTH__ADMIN_EMAIL`
- `APP_AUTH__ADMIN_PASSWORD`

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
