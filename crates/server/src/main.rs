use std::{net::SocketAddr, sync::Arc};

use axum::{Router, middleware};
use tower_http::trace::TraceLayer;

use rust_oxide::{
    auth::providers::{AuthProviders, LocalAuthProvider},
    config::AppConfig,
    db::connection,
    db::dao::DaoContext,
    logging::init_tracing,
    middleware::json_error_middleware,
    routes::router,
    services::auth_service,
    services::user_service,
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

    let jwt = rust_oxide::state::JwtKeys::from_secret(cfg.jwt_secret.as_bytes());
    let db = connection::connect(&cfg).await?;
    let daos = DaoContext::new(&db);
    let user_service = user_service::UserService::new(daos.user());
    let local_provider = LocalAuthProvider::new(
        user_service,
        daos.refresh_token(),
        jwt.clone(),
    );
    let mut providers = AuthProviders::new(cfg.auth_provider)
        .with_provider(std::sync::Arc::new(local_provider))
        .map_err(|err| anyhow::anyhow!(err.to_string()))?;
    providers
        .set_active(cfg.auth_provider)
        .map_err(|err| anyhow::anyhow!(err.to_string()))?;
    let state = AppState::new(cfg, db, jwt, providers);

    let auth_service = auth_service::AuthService::new(&state.auth_providers);
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
