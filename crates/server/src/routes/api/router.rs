use std::sync::Arc;

use axum::Router;

use crate::{realtime::SocketAppState, state::AppState};

use super::{admin, auth, protected, public, realtime, todo_crud};

pub fn router(state: Arc<AppState>, realtime_runtime: Arc<SocketAppState>) -> Router {
    Router::new()
        .merge(public::router())
        .merge(auth::router(state.clone()))
        .merge(realtime::router(realtime_runtime))
        .merge(todo_crud::router(state.clone()))
        .merge(protected::router(state.clone()))
        .merge(admin::router(state))
}
