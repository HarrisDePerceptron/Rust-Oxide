use std::sync::Arc;

use axum::Router;
use sea_orm::{DatabaseBackend, MockDatabase};

use crate::{
    auth::providers::{AuthProviders, LocalAuthProvider},
    config::AppConfig,
    db::dao::DaoContext,
    routes::router,
    services::user_service,
    state::{AppState, JwtKeys},
};

pub fn test_router(secret: &[u8]) -> Router {
    let db = MockDatabase::new(DatabaseBackend::Postgres).into_connection();
    let mut cfg = AppConfig::from_env().expect("load app config");
    cfg.jwt_secret = String::from_utf8_lossy(secret).into_owned();
    let jwt = JwtKeys::from_secret(cfg.jwt_secret.as_bytes());
    let daos = DaoContext::new(&db);
    let user_service = user_service::UserService::new(daos.user());
    let local_provider = LocalAuthProvider::new(
        user_service,
        daos.refresh_token(),
        jwt.clone(),
    );
    let providers = AuthProviders::new(
        cfg.auth_provider,
        vec![std::sync::Arc::new(local_provider)],
    )
    .expect("create auth providers");
    let state = AppState::new(cfg, db, jwt, providers);
    router(Arc::clone(&state))
}
