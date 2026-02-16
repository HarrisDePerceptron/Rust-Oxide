use std::sync::Arc;

use axum::{Extension, Router};

use crate::{realtime::RealtimeRuntimeState, state::AppState};

use super::{api, views};

pub const API_PREFIX: &str = "/api/v1";

pub fn router(state: Arc<AppState>, realtime_runtime: Arc<RealtimeRuntimeState>) -> Router {
    let realtime_handle = realtime_runtime.handle.clone();
    Router::new()
        .nest(API_PREFIX, api::router(state.clone(), realtime_runtime))
        .merge(views::router(state))
        .layer(Extension(realtime_handle))
}
