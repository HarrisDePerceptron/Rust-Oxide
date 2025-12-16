use std::sync::Arc;

use axum::Router;

use crate::state::AppState;

pub mod admin;
pub mod auth;
pub mod protected;
pub mod public;

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .merge(public::router())
        .merge(auth::router(state.clone()))
        .merge(protected::router(state.clone()))
        .merge(admin::router(state))
}
