use std::sync::Arc;

use axum::Router;

use crate::state::AppState;

pub mod admin;
pub mod auth;
pub mod crud_router;
pub mod protected;
pub mod public;
pub mod route_list;
pub mod todo;
pub mod todo_crud;

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .merge(public::router())
        .merge(auth::router(state.clone()))
        .merge(todo::router(state.clone()))
        .merge(todo_crud::router(state.clone()))
        .merge(protected::router(state.clone()))
        .merge(admin::router(state))
}
