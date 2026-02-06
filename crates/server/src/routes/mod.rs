use std::sync::Arc;

use axum::Router;

use crate::state::AppState;

pub mod api;
pub mod base_api_router;
pub mod base_router;
pub mod route_list;
pub mod views;

pub const API_PREFIX: &str = "/api/v1";

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .nest(API_PREFIX, api::router(state))
        .merge(views::router())
}
