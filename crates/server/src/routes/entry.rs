use std::sync::Arc;

use axum::Router;

use crate::state::AppState;

use super::{api, views};

pub const API_PREFIX: &str = "/api/v1";

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .nest(API_PREFIX, api::router(state.clone()))
        .merge(views::router(state))
}
