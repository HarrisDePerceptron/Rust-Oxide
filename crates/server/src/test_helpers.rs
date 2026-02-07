use std::sync::Arc;

use axum::Router;
use sea_orm::{DatabaseBackend, MockDatabase};

use crate::{
    auth::bootstrap::build_providers, config::AppConfig, routes::router, services::ServiceContext,
    state::AppState,
};

pub fn test_router(secret: &[u8]) -> Router {
    let db = MockDatabase::new(DatabaseBackend::Postgres).into_connection();
    let mut cfg = AppConfig::from_env().expect("load app config");
    cfg.jwt_secret = String::from_utf8_lossy(secret).into_owned();
    let services = ServiceContext::new(&db);
    let providers = build_providers(&cfg, &services).expect("create auth providers");
    let state = AppState::new(cfg, db, providers);
    router(Arc::clone(&state))
}
