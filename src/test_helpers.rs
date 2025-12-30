use std::sync::Arc;

use axum::Router;
use sea_orm::{DatabaseBackend, MockDatabase};

use crate::{routes::router, state::AppState};

pub fn test_router(secret: &[u8]) -> Router {
    let db = MockDatabase::new(DatabaseBackend::Postgres).into_connection();
    let state = AppState::new(secret, db);
    router(Arc::clone(&state))
}
