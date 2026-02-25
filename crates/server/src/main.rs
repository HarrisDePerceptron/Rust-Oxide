use std::{net::SocketAddr, sync::Arc};

use anyhow::Context;
use axum::{Router, middleware};
use tower_http::trace::TraceLayer;

#[cfg(feature = "realtime")]
use rust_oxide::realtime::{AppRealtimeVerifier, SocketAppState};
use rust_oxide::{
    auth::bootstrap::init_providers,
    config::AppConfig,
    db::connection,
    logging::init_tracing,
    routes::{
        middleware::{catch_panic_layer, json_error_middleware},
        router,
    },
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
    init_tracing(&cfg.logging.rust_log);

    #[cfg(debug_assertions)]
    {
        let app_config = serde_json::to_string_pretty(&cfg)
            .unwrap_or_else(|err| format!("{{\"serialize_error\":\"{err}\"}}"));
        tracing::info!(%app_config, "loaded app config (debug build)");
    }

    let db_cfg = cfg
        .database
        .as_ref()
        .context("database config missing; set APP_DATABASE__URL")?;
    let db = connection::connect(db_cfg).await?;
    let services = ServiceContext::new(&db);

    let providers = init_providers(cfg.auth.as_ref(), &services).await?;

    let state = AppState::new(cfg, db, providers);

    #[cfg(feature = "realtime")]
    let realtime_runtime = {
        let realtime =
            rust_oxide::realtime::SocketServerHandle::spawn(state.config.realtime.clone());
        Arc::new(SocketAppState::new(
            realtime,
            AppRealtimeVerifier::new(state.auth_providers.clone()),
        ))
    };

    #[cfg(feature = "realtime")]
    let app = Router::new()
        .merge(router(Arc::clone(&state), realtime_runtime))
        .layer(middleware::from_fn(json_error_middleware))
        .layer(catch_panic_layer())
        .layer(TraceLayer::new_for_http());

    #[cfg(not(feature = "realtime"))]
    let app = Router::new()
        .merge(router(Arc::clone(&state)))
        .layer(middleware::from_fn(json_error_middleware))
        .layer(catch_panic_layer())
        .layer(TraceLayer::new_for_http());

    let addr: SocketAddr = format!(
        "{}:{}",
        state.config.general.host.as_str(),
        state.config.general.port
    )
    .parse()
    .expect("invalid host/port");
    tracing::info!("listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
