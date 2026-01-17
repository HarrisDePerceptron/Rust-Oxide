use std::sync::Arc;

use axum::Router;
use sea_orm::{DatabaseBackend, MockDatabase};

use crate::{config::AppConfig, routes::router, state::AppState};

pub fn test_router(secret: &[u8]) -> Router {
    let db = MockDatabase::new(DatabaseBackend::Postgres).into_connection();
    let mut cfg = AppConfig::from_env().expect("load app config");
    cfg.jwt_secret = String::from_utf8_lossy(secret).into_owned();
    let state = AppState::new(cfg, db);
    router(Arc::clone(&state))
}
