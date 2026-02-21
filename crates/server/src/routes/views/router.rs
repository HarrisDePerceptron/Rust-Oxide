use std::sync::Arc;

use axum::Router;

use crate::state::AppState;

use super::{public, todo};

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .merge(public::router(state))
        .merge(todo::router())
}
