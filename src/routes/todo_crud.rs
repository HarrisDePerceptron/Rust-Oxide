use std::sync::Arc;

use axum::{
    Json, Router,
    extract::Path,
    middleware,
    routing::get,
};

use uuid::Uuid;

use crate::{
    auth::jwt::jwt_auth,
    db::dao::DaoContext,
    error::AppError,
    routes::base_api_router::BaseApiRouter,
    services::todo_service::TodoService,
    state::AppState,
};

pub struct TodoListCrud {
    service: TodoService,
    state: Arc<AppState>,
}

impl TodoListCrud {
    pub fn new(service: TodoService, state: Arc<AppState>) -> Self {
        Self { service, state }
    }
}

impl BaseApiRouter for TodoListCrud {
    type Service = TodoService;

    fn service(&self) -> Self::Service {
        self.service.clone()
    }

    fn base_path() -> &'static str {
        "/todo-crud"
    }

    fn apply_router_middleware<S>(&self, router: Router<S>) -> Router<S>
    where
        S: Clone + Send + Sync + 'static,
    {
        router.layer(middleware::from_fn_with_state(self.state.clone(), jwt_auth))
    }

    fn register_routes<S>(&self, router: Router<S>) -> Router<S>
    where
        S: Clone + Send + Sync + 'static,
    {
        let base = <Self as BaseApiRouter>::base_path();
        let list_count_path = format!("{}/count", base);
        let item_count_path = format!("{}/{{id}}/items/count", base);

        let list_count_route = get({
            let service = self.service();
            move || async move {
                let count = service.count_lists().await?;
                Ok::<_, AppError>(Json(CountResponse { count }))
            }
        });

        let item_count_route = get({
            let service = self.service();
            move |Path(id): Path<Uuid>| async move {
                let count = service.count_items_by_list(&id).await?;
                Ok::<_, AppError>(Json(CountResponse { count }))
            }
        });

        router
            .route(&list_count_path, list_count_route)
            .route(&item_count_path, item_count_route)
    }
}

pub fn router(state: Arc<AppState>) -> Router {
    let daos = DaoContext::new(&state.db);
    let service = TodoService::new(daos.todo());
    TodoListCrud::new(service, state.clone())
        .router_for()
        .with_state(state)
}

#[derive(Debug, serde::Serialize)]
struct CountResponse {
    count: u64,
}
