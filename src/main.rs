use std::{net::SocketAddr, sync::Arc, time::Duration};

use axum::Router;
use sea_orm::{ConnectOptions, Database};
use tower_http::trace::TraceLayer;

use sample_server::{
    auth::{Role, password},
    config::AppConfig,
    db::user_repo,
    logging::init_tracing,
    routes::router,
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

    let mut opt = ConnectOptions::new(cfg.database_url.clone());
    opt.max_connections(cfg.db_max_connections)
        .min_connections(cfg.db_min_idle)
        .connect_timeout(Duration::from_secs(5))
        .sqlx_logging(false);

    let db = Database::connect(opt).await?;
    tracing::info!("syncing database schema from entities");
    db.get_schema_registry("sample_server::db::entities::*")
        .sync(&db)
        .await?;

    seed_admin(&cfg, &db).await?;

    let state = AppState::new(cfg.jwt_secret.as_bytes(), db);

    let app = Router::new()
        .merge(router(Arc::clone(&state)))
        .layer(TraceLayer::new_for_http());

    let addr: SocketAddr = format!("{}:{}", cfg.host, cfg.port)
        .parse()
        .expect("invalid host/port");
    tracing::info!("listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn seed_admin(cfg: &AppConfig, db: &sea_orm::DatabaseConnection) -> anyhow::Result<()> {
    if let Some(existing) = user_repo::find_by_email(db, &cfg.admin_email).await? {
        tracing::info!("admin user already present: {}", existing.email);
        return Ok(());
    }

    let hash = password::hash_password(&cfg.admin_password)
        .map_err(|e| anyhow::anyhow!("admin seed hash error: {}", e.message))?;
    let user = user_repo::create_user(db, &cfg.admin_email, &hash, Role::Admin.as_str()).await?;
    tracing::info!("seeded admin user {}", user.email);
    Ok(())
}
