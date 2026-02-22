use std::sync::Arc;

use axum::Router;

use crate::realtime::SocketAppState;

pub fn router(runtime: Arc<SocketAppState>) -> Router {
    realtime::server::axum::router(runtime)
}
