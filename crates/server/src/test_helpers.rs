use std::sync::Arc;

use axum::Router;
use sea_orm::{DatabaseBackend, MockDatabase};

use crate::{
    auth::providers::{AuthProviders, LocalAuthProvider},
    config::AppConfig,
    routes::router,
    services::ServiceContext,
    state::{AppState, JwtKeys},
};

pub fn test_router(secret: &[u8]) -> Router {
    let db = MockDatabase::new(DatabaseBackend::Postgres).into_connection();
    let mut cfg = AppConfig::from_env().expect("load app config");
    cfg.jwt_secret = String::from_utf8_lossy(secret).into_owned();
    let jwt = JwtKeys::from_secret(cfg.jwt_secret.as_bytes());
    let services = ServiceContext::new(&db);
    let user_service = services.user();
    let local_provider =
        LocalAuthProvider::new(user_service, services.refresh_token_dao(), jwt.clone());
    let mut providers = AuthProviders::new(cfg.auth_provider)
        .with_provider(std::sync::Arc::new(local_provider))
        .expect("create auth providers");
    providers
        .set_active(cfg.auth_provider)
        .expect("set active auth provider");
    let state = AppState::new(cfg, db, jwt, providers);
    router(Arc::clone(&state))
}
