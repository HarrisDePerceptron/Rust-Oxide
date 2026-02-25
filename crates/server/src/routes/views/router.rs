use std::sync::Arc;

use axum::Router;

use crate::state::AppState;

use super::public;

#[cfg(feature = "todo-example")]
use super::todo;

pub fn router(state: Arc<AppState>) -> Router {
    #[cfg(feature = "todo-example")]
    {
        return Router::new()
            .merge(public::router(state))
            .merge(todo::router());
    }

    #[cfg(not(feature = "todo-example"))]
    {
        Router::new().merge(public::router(state))
    }
}
