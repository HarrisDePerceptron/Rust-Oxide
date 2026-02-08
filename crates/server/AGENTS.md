# Server Template Guidelines (`src/` scope)

## Architecture Contract
- Boot flow in `src/main.rs`: config -> logging -> DB connect/schema sync -> auth provider init -> `AppState` -> router + middleware.
- `AppState` carries `config`, `jwt`, `db`, and `auth_providers`.
- API prefix is `/api/v1` (`routes::API_PREFIX`); keep API routes nested under it.
- Router composition is split into `routes/api/*` (JSON) and `routes/views/*` (HTML).
- Keep debug-only docs pages (`/docs`, `/routes`, `/entities`) behind `cfg(debug_assertions)`.

## Module Map
- `src/config.rs`: env-driven `AppConfig`.
- `src/state.rs`: shared app state and JWT keys.
- `src/auth/`: claims, roles, JWT/password helpers, auth providers.
- `src/middleware/`: auth guards, JSON error normalization, panic-to-JSON.
- `src/db/entities/`: SeaORM schema entities.
- `src/db/dao/`: all DB access; `DaoBase` shared CRUD primitives.
- `src/services/`: business logic (`CrudService` + feature services).
- `src/routes/api/`: API handlers.
- `src/routes/views/`: Askama-rendered pages.
- `src/routes/base_api_router.rs`: generic CRUD router builder.
- `src/routes/route_list.rs` and `src/db/entity_catalog.rs`: generated catalogs used by docs/views.

## Module Index Files
- `mod.rs` files should only contain module declarations and re-exports (plus optional module docs).
- Do not keep executable logic, type definitions, trait definitions, or impl blocks in `mod.rs`.
- Prefer moving module logic into a single sibling file first (for example `providers.rs`) before considering further splits.

## Rust Rules
- Prefer single-responsibility functions and types: one function should do one coherent job.
- Keep handlers thin; move reusable business/data logic into `services/` or `db/dao/`.


## Extension Workflow (Entity -> DAO -> Service -> Router)
- Add/modify entity in `src/db/entities/`.
- Implement DAO in `src/db/dao/` via `DaoBase`.
- Implement/extend service in `src/services/` (use `CrudService` for CRUD resources).
- Mount route in `src/routes/api/` and merge in `src/routes/api/mod.rs`.
- When behavior changes, update `views/docs.html` examples so docs match runtime.

## Auth & Authorization Rules
- Auth is provider-driven via `AuthProvider` trait and `AuthProviders` registry.
- Active provider comes from `AUTH_PROVIDER`; providers are registered once at startup.
- Route handlers should use extractors/guards:
- `AuthGuard` for authenticated access.
- `AuthRoleGuard<R>` for role-checked access.
- `AuthRolGuardLayer` can be used for method-level middleware on CRUD routers.
- Password hashing uses Argon2; minimum password length is 8.

## API & Error Conventions
- JSON responses use `JsonApiResponse<T>` (`{ status, message, data }`).
- Route/service errors use `AppError` with consistent HTTP mapping.
- Keep handlers thin and HTTP-focused; do not embed raw SeaORM queries in routes.
- Axum path params must use `{param}` syntax, not `:param`.

## CRUD Router & Filter Conventions
- `CrudApiRouter` default endpoints:
- `POST /base`
- `GET /base`
- `GET /base/{id}`
- `PATCH /base/{id}`
- `DELETE /base/{id}`
- Default list pagination is `page=1`, `page_size=25`.
- Max `page_size` is 100 (`DaoBase::MAX_PAGE_SIZE`).
- Filter parsing is column-type aware by default (`FilterMode::AllColumns` + `ByColumnType`).
- String wildcard syntax only supports edge wildcards (`prefix*`, `*suffix`, `*contains*`).
- Unknown/denied columns and invalid filter shapes should return `400`.

## Docs/Route Catalog Generation Constraints
- Keep route paths parseable by build-time route scanner:
- Prefer string literals or simple string consts in `.route(...)`.
- Keep request/response structs `serde`-derived when you want rich route docs/catalog output.
- Keep method builder usage straightforward (`get/post/patch/delete`) for route extraction.

## Config & Security
- Supported env vars include:
- `HOST`, `PORT`, `RUST_LOG`
- `DATABASE_URL`, `DB_MAX_CONNS`, `DB_MIN_IDLE`
- `JWT_SECRET`
- `ADMIN_EMAIL`, `ADMIN_PASSWORD`
- `AUTH_PROVIDER`
- In release builds, required secrets/config must come from env (no debug defaults).
- Never commit secrets; use `.env` locally (gitignored) or a secrets manager.

## Testing
- Prefer fast route/service tests.
- Use mock DB for unit-style route tests where possible.
- DB-backed integration tests may be `#[ignore]` with explicit reason.
- Keep test names behavior-oriented (for example: `rejects_missing_auth`, `applies_filter_range`).


## View JavaScript Rules (`views/*.html`)
- Keep high-level flow in concrete named functions (for example `initDocsPage`, `initNavigation`, `syncFromLocation`).
- Anonymous callbacks are allowed only for small local leaf behavior (for example short event handlers or timers).
- If a JavaScript routine is reusable across functions, extract it into a separate named function.


## Commands (workspace root)
- `cargo run -p rust_oxide`
- `cargo test -p rust_oxide`
- `cargo clippy --all-targets --all-features`
- `cargo fmt`



