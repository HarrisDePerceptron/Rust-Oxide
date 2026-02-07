use axum::Router;

use super::{public, todo};

pub fn router() -> Router {
    Router::new().merge(public::router()).merge(todo::router())
}
