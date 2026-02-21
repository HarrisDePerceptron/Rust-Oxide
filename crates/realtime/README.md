# Realtime crate

Reusable realtime transport for Axum + Rust clients.

## Demo app

This crate includes a self-contained demo chat server with predefined users and tokens.

Run:

```sh
cargo run -p realtime --example chat_demo
```

Then open:

```text
http://127.0.0.1:4001
```

Optional:

```sh
REALTIME_DEMO_ADDR=127.0.0.1:5001 cargo run -p realtime --example chat_demo
```
