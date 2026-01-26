use std::{net::SocketAddr, sync::Arc};

use axum::{Router, middleware};
use tower_http::trace::TraceLayer;

use rust_oxide::{
    config::AppConfig, db::connection, db::dao::DaoContext, logging::init_tracing,
    middleware::json_error_middleware, routes::router, services::auth_service, state::AppState,
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
    let state = AppState::new(cfg, db);

    let daos = DaoContext::new(&state.db);
    let auth_service =
        auth_service::AuthService::new(daos.user(), daos.refresh_token(), state.jwt.clone());
    auth_service.seed_admin(&state.config).await?;

    let app = Router::new()
        .merge(router(Arc::clone(&state)))
        .layer(middleware::from_fn(json_error_middleware))
        .layer(TraceLayer::new_for_http());

    let addr: SocketAddr = format!("{}:{}", state.config.host.as_str(), state.config.port)
        .parse()
        .expect("invalid host/port");
    tracing::info!("listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
