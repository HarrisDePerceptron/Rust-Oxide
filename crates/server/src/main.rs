use std::{net::SocketAddr, sync::Arc};

use axum::{Router, middleware};
use tower_http::trace::TraceLayer;

use rust_oxide::{
    auth::bootstrap::init_providers,
    config::AppConfig,
    db::connection,
    logging::init_tracing,
    middleware::{catch_panic_layer, json_error_middleware},
    routes::router,
    services::ServiceContext,
    state::AppState,
};

#[tokio::main]
async fn main() {
    if let Err(err) = run().await {
        tracing::error!("server failed: {err:?}");
        std::process::exit(1);
    }
}

async fn run() -> anyhow::Result<()> {
    let cfg = AppConfig::from_env().expect("failed to load config");
    init_tracing(&cfg.log_level);

    let db = connection::connect(&cfg).await?;
    let services = ServiceContext::new(&db);

    let providers = init_providers(&cfg, &services).await?;

    let state = AppState::new(cfg, db, providers);

    let app = Router::new()
        .merge(router(Arc::clone(&state)))
        .layer(middleware::from_fn(json_error_middleware))
        .layer(catch_panic_layer())
        .layer(TraceLayer::new_for_http());

    let addr: SocketAddr = format!("{}:{}", state.config.host.as_str(), state.config.port)
        .parse()
        .expect("invalid host/port");
    tracing::info!("listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
