use axum::Router;

pub mod public;
pub mod todo;

pub fn router() -> Router {
    Router::new()
        .merge(public::router())
        .merge(todo::router())
}
