use std::sync::Arc;

use axum::Router;

use crate::{realtime::SocketAppState, state::AppState};

use super::{api, views};

pub const API_PREFIX: &str = "/api/v1";

pub fn router(state: Arc<AppState>, realtime_runtime: Arc<SocketAppState>) -> Router {
    Router::new()
        .nest(API_PREFIX, api::router(state.clone(), realtime_runtime))
        .merge(views::router(state))
}
