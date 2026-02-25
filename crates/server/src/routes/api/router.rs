use std::sync::Arc;

use axum::Router;

use crate::state::AppState;

use super::public;

#[cfg(feature = "realtime")]
use crate::realtime::SocketAppState;

#[cfg(feature = "auth-local")]
use super::{admin, auth, protected};

#[cfg(feature = "realtime")]
use super::realtime;

#[cfg(feature = "todo-example")]
use super::todo_crud;

#[cfg(feature = "realtime")]
pub fn router(state: Arc<AppState>, realtime_runtime: Arc<SocketAppState>) -> Router {
    #[cfg(not(any(feature = "auth-local", feature = "todo-example")))]
    let _ = &state;

    let router = Router::new().merge(public::router());

    #[cfg(feature = "auth-local")]
    let router = router
        .merge(auth::router(state.clone()))
        .merge(protected::router(state.clone()))
        .merge(admin::router(state.clone()));

    let router = router.merge(realtime::router(realtime_runtime));

    #[cfg(feature = "todo-example")]
    let router = router.merge(todo_crud::router(state.clone()));

    router
}

#[cfg(not(feature = "realtime"))]
pub fn router(state: Arc<AppState>) -> Router {
    #[cfg(not(any(feature = "auth-local", feature = "todo-example")))]
    let _ = &state;

    let router = Router::new().merge(public::router());

    #[cfg(feature = "auth-local")]
    let router = router
        .merge(auth::router(state.clone()))
        .merge(protected::router(state.clone()))
        .merge(admin::router(state.clone()));

    #[cfg(feature = "todo-example")]
    let router = router.merge(todo_crud::router(state.clone()));

    router
}
