use std::sync::Arc;

use axum::Router;

use crate::state::AppState;

#[cfg(feature = "realtime")]
use crate::realtime::SocketAppState;

use super::{api, views};

pub const API_PREFIX: &str = "/api/v1";

#[cfg(feature = "realtime")]
pub fn router(state: Arc<AppState>, realtime_runtime: Arc<SocketAppState>) -> Router {
    Router::new()
        .nest(API_PREFIX, api::router(state.clone(), realtime_runtime))
        .merge(views::router(state))
}

#[cfg(not(feature = "realtime"))]
pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .nest(API_PREFIX, api::router(state.clone()))
        .merge(views::router(state))
}
