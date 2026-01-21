use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, State},
    middleware,
    routing::get,
};

use uuid::Uuid;

use crate::{
    auth::jwt::jwt_auth,
    db::dao::DaoContext,
    error::AppError,
    routes::base_api_router::{CrudApiRouter, Method},
    services::todo_service::TodoService,
    state::AppState,
};

const BASE_PATH: &str = "/todo-crud";

pub fn router(state: Arc<AppState>) -> Router {
    let daos = DaoContext::new(&state.db);
    let service = TodoService::new(daos.todo());
    let auth_layer = middleware::from_fn_with_state(state.clone(), jwt_auth);
    let crud_router = CrudApiRouter::new(service.clone(), BASE_PATH)
        //.set_allowed_methods(&[
        //    Method::Create,
        //    Method::List,
        //    Method::Get,
        //    Method::Patch,
        //    Method::Delete,
        //])
        //.set_method_middleware(Method::Create, auth_layer.clone())
        //.set_method_middleware(Method::List, auth_layer.clone())
        //.set_method_middleware(Method::Get, auth_layer.clone())
        //.set_method_middleware(Method::Patch, auth_layer.clone())
        //.set_method_middleware(Method::Delete, auth_layer.clone())
        ;

    let list_count_route = get(list_count_handler).route_layer(auth_layer.clone());
    let item_count_route = get(item_count_handler).route_layer(auth_layer.clone());

    crud_router
        .router()
        .route("/todo-crud/count", list_count_route)
        .route("/todo-crud/{id}/items/count", item_count_route)
        .with_state(state)
}

#[derive(Debug, serde::Serialize)]
struct CountResponse {
    count: u64,
}

async fn list_count_handler(
    State(state): State<Arc<AppState>>,
) -> Result<Json<CountResponse>, AppError> {
    let daos = DaoContext::new(&state.db);
    let service = TodoService::new(daos.todo());
    let count = service.count_lists().await?;
    Ok(Json(CountResponse { count }))
}

async fn item_count_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<Json<CountResponse>, AppError> {
    let daos = DaoContext::new(&state.db);
    let service = TodoService::new(daos.todo());
    let count = service.count_items_by_list(&id).await?;
    Ok(Json(CountResponse { count }))
}
