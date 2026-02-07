use std::sync::Arc;

use axum::Router;

use crate::state::AppState;

use super::{admin, auth, protected, public, todo_crud};

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .merge(public::router())
        .merge(auth::router(state.clone()))
        .merge(todo_crud::router(state.clone()))
        .merge(protected::router(state.clone()))
        .merge(admin::router(state))
}
