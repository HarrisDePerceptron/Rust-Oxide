use std::sync::Arc;

use axum::Router;

use crate::{
    db::dao::DaoContext,
    routes::crud_router::CrudRouter,
    services::todo_service::TodoService,
    state::AppState,
};

pub struct TodoListCrud {
    service: TodoService,
}

impl TodoListCrud {
    pub fn new(service: TodoService) -> Self {
        Self { service }
    }
}

impl CrudRouter for TodoListCrud {
    type Service = TodoService;

    fn service(&self) -> Self::Service {
        self.service.clone()
    }

    fn base_path() -> &'static str {
        "/todo-crud"
    }
}

pub fn router(state: Arc<AppState>) -> Router {
    let daos = DaoContext::new(&state.db);
    let service = TodoService::new(daos.todo());
    TodoListCrud::new(service).router_for().with_state(state)
}
