use std::sync::Arc;

use axum::Router;

use crate::state::AppState;

pub fn router(state: Arc<AppState>) -> Router {
    realtime::server::axum::router(state)
}
