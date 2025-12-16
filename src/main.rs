use std::{net::SocketAddr, sync::Arc};

use axum::Router;
use tower_http::trace::TraceLayer;

mod auth;
mod config;
mod error;
mod logging;
mod routes;
mod state;

use crate::config::AppConfig;
use crate::logging::init_tracing;
use crate::routes::router;
use crate::state::AppState;

#[tokio::main]
async fn main() {
    let cfg = AppConfig::from_env().expect("failed to load config");
    init_tracing(&cfg.log_level);

    let state = AppState::new(cfg.jwt_secret.as_bytes());

    let app = Router::new()
        .merge(router(Arc::clone(&state)))
        .layer(TraceLayer::new_for_http());

    let addr: SocketAddr = format!("{}:{}", cfg.host, cfg.port)
        .parse()
        .expect("invalid host/port");
    tracing::info!("listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
