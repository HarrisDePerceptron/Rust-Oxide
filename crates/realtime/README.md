# realtime

Reusable realtime transport primitives for Axum servers and Rust websocket clients.

## Install

```sh
cargo add rust-oxide-realtime --rename realtime
```

## Server quick start

```rust
use std::sync::Arc;
use axum::Router;
use realtime::server::{RealtimeConfig, RealtimeHandle, RealtimeRuntimeState, RealtimeTokenVerifier, SessionAuth};

struct AllowAllVerifier;

#[async_trait::async_trait]
impl RealtimeTokenVerifier for AllowAllVerifier {
    async fn verify_token(&self, _token: &str) -> Result<SessionAuth, realtime::server::RealtimeError> {
        Ok(SessionAuth {
            user_id: "demo-user".to_string(),
            roles: vec!["user".to_string()],
            tenant_id: None,
        })
    }
}

let handle = RealtimeHandle::spawn(RealtimeConfig::default());
let runtime = Arc::new(RealtimeRuntimeState::new(handle, AllowAllVerifier));
let app = Router::new().nest("/api/v1", realtime::server::axum::router(runtime));
```

Default endpoint path: `/api/v1/realtime/socket`.

## Rust client quick start

```rust
use realtime::client::RealtimeClient;

let mut client = RealtimeClient::connect(
    "ws://127.0.0.1:3000/api/v1/realtime/socket",
    Some("your-token"),
).await?;
```

## Demo app

This crate includes a self-contained demo chat server with predefined users and tokens:

```sh
cargo run -p rust-oxide-realtime --example chat_demo
```

Then open `http://127.0.0.1:4001`.

Optional:

```sh
REALTIME_DEMO_ADDR=127.0.0.1:5001 cargo run -p rust-oxide-realtime --example chat_demo
```
