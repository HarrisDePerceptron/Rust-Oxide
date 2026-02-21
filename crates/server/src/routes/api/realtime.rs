use std::sync::Arc;

use axum::Router;

use crate::realtime::RealtimeRuntimeState;

pub fn router(runtime: Arc<RealtimeRuntimeState>) -> Router {
    realtime::server::axum::router(runtime)
}
