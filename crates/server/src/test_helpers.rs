use std::sync::Arc;

use axum::Router;
use sea_orm::{DatabaseBackend, MockDatabase};

use crate::{
    auth::{bootstrap::build_providers, providers::AuthProviderId},
    config::{AppConfig, AuthConfig},
    realtime::{AppRealtimeVerifier, SocketAppState, SocketServerHandle},
    routes::router,
    services::ServiceContext,
    state::AppState,
};

pub fn test_router(secret: &[u8]) -> Router {
    let db = MockDatabase::new(DatabaseBackend::Postgres).into_connection();
    let mut cfg = AppConfig::from_env().expect("load app config");
    cfg.auth = Some(AuthConfig {
        provider: AuthProviderId::Local,
        jwt_secret: String::from_utf8_lossy(secret).into_owned(),
        admin_email: "admin@example.com".to_string(),
        admin_password: "adminpassword".to_string(),
    });
    let services = ServiceContext::new(&db);
    let providers = build_providers(
        cfg.auth.as_ref().expect("auth config should be present"),
        &services,
    )
    .expect("create auth providers");
    let realtime = SocketServerHandle::spawn(cfg.realtime.clone());
    let realtime_runtime = Arc::new(SocketAppState::new(
        realtime,
        AppRealtimeVerifier::new(providers.clone()),
    ));
    let state = AppState::new(cfg, db, providers);
    router(Arc::clone(&state), realtime_runtime)
}
