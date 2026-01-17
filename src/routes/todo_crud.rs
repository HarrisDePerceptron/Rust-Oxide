use std::sync::Arc;

use axum::Router;

use crate::{
    db::dao::DaoContext,
    routes::crud_router::CrudRouter,
    services::todo_service::TodoService,
    state::AppState,
};

pub struct TodoListCrud;

impl CrudRouter for TodoListCrud {
    type Service = TodoService;

    fn service(state: &AppState) -> Self::Service {
        let daos = DaoContext::new(&state.db);
        TodoService::new(daos.todo())
    }

    fn base_path() -> &'static str {
        "/todo-crud"
    }
}

pub fn router(state: Arc<AppState>) -> Router {
    <TodoListCrud as CrudRouter>::router(state)
}
