use std::{net::SocketAddr, sync::Arc};

use anyhow::Context;
use axum::{Extension, Router, middleware};
use realtime::server::RealtimeError;
use tower_http::trace::TraceLayer;

use rust_oxide::{
    auth::bootstrap::init_providers,
    config::AppConfig,
    db::connection,
    logging::init_tracing,
    realtime::{AppChannelPolicy, AppRealtimeVerifier, ChatRoomRegistry, RealtimeRuntimeState},
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
    let auth_cfg = cfg.auth.as_ref().context(
        "auth config missing; set APP_AUTH__JWT_SECRET, APP_AUTH__ADMIN_EMAIL, APP_AUTH__ADMIN_PASSWORD",
    )?;

    let db = connection::connect(db_cfg).await?;
    let services = ServiceContext::new(&db);

    let providers = init_providers(auth_cfg, &services).await?;
    let chat_rooms = ChatRoomRegistry::new();
    let realtime = rust_oxide::realtime::RealtimeHandle::spawn_with_policy(
        cfg.realtime.clone(),
        Arc::new(AppChannelPolicy::new(chat_rooms.clone())),
    );
    let realtime_runtime = Arc::new(RealtimeRuntimeState::new(
        realtime.clone(),
        Arc::new(AppRealtimeVerifier::new(providers.clone())),
    ));

    let state = AppState::new(cfg, db, providers);

    let app = Router::new()
        .merge(router(Arc::clone(&state), realtime_runtime))
        .layer(Extension(chat_rooms))
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

async fn SomeFuncton(
    realtime: Arc<RealtimeRuntimeState>,
) -> Result<(), Box<dyn std::error::Error + 'static>> {
    let handle: Result<(), RealtimeError> = tokio::spawn(async move {
        let mut cont = true;

        realtime.on_message("common", |msg| print!("Got message: {}", msg));
        while cont {
            realtime.send("common", "hello there".into()).await?;

            cont = false;
        }

        Ok(())
    })
    .await?;

    Ok(())
}
