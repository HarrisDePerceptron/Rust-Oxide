use std::sync::Arc;

use axum::Router;

use crate::{routes::router, state::AppState};

pub fn test_router(secret: &[u8]) -> Router {
    let state = AppState::new(secret);
    router(Arc::clone(&state))
}
