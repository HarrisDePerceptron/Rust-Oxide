use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, patch, post},
};
use sea_orm::prelude::DateTimeWithTimeZone;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    auth::Role,
    db::entities::{todo_item, todo_list},
    error::AppError,
    middleware::{AuthGuard, AuthRolGuardLayer},
    response::{ApiResult, JsonApiResponse},
    routes::base_api_router::{CrudApiRouter, Method},
    services::{ServiceContext, todo_service},
    state::AppState,
};

const BASE_PATH: &str = "/todo-crud";

#[derive(Debug, Deserialize)]
pub struct CreateListRequest {
    pub title: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateListRequest {
    pub title: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateItemRequest {
    pub description: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateItemRequest {
    pub description: Option<String>,
    pub done: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct TodoListResponse {
    pub id: Uuid,
    pub title: String,
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
}

#[derive(Debug, Serialize)]
pub struct TodoItemResponse {
    pub id: Uuid,
    pub list_id: Uuid,
    pub description: String,
    pub done: bool,
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
}

#[derive(Debug, Serialize)]
pub struct TodoListDetailResponse {
    pub list: TodoListResponse,
    pub items: Vec<TodoItemResponse>,
}

pub fn router(state: Arc<AppState>) -> Router {
    let service = ServiceContext::from_state(state.as_ref()).todo();
    let crud_router = CrudApiRouter::new(service.clone(), BASE_PATH).set_method_middleware(
        Method::Create,
        AuthRolGuardLayer::new(state.clone(), Role::User),
    );

    let list_count_route = get(list_count_handler);
    let item_count_route = get(item_count_handler);

    let todo_routes = Router::new()
        .route("/todo", post(create_list).get(list_lists))
        .route(
            "/todo/{list_id}",
            get(get_list).patch(update_list).delete(delete_list),
        )
        .route("/todo/{list_id}/items", post(create_item).get(list_items))
        .route(
            "/todo/{list_id}/items/{item_id}",
            patch(update_item).delete(delete_item),
        )
        .route("/todo/panic", get(failing_handler));

    crud_router
        .router()
        .route("/todo-crud/count", list_count_route)
        .route("/todo-crud/{id}/items/count", item_count_route)
        .merge(todo_routes)
        .with_state(state)
}

async fn failing_handler() -> ApiResult<()> {
    Err::<&str, _>("this should fail").expect("inside the  failign handler");
    JsonApiResponse::ok(())
}

#[derive(Debug, serde::Serialize)]
struct CountResponse {
    count: u64,
}

async fn list_count_handler(
    State(state): State<Arc<AppState>>,
    _auth: AuthGuard,
) -> ApiResult<CountResponse> {
    let service = ServiceContext::from_state(state.as_ref()).todo();
    let count = service.count_lists().await?;
    JsonApiResponse::ok(CountResponse { count })
}

async fn item_count_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    _auth: AuthGuard,
) -> ApiResult<CountResponse> {
    let service = ServiceContext::from_state(state.as_ref()).todo();
    let count = service.count_items_by_list(&id).await?;
    JsonApiResponse::ok(CountResponse { count })
}

async fn create_list(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateListRequest>,
) -> ApiResult<TodoListResponse> {
    let title = normalize_title(&body.title)?;
    let service = todo_service_from_state(state.as_ref());
    let list = service.create_list(title).await?;
    JsonApiResponse::with_status(StatusCode::CREATED, "created", list.into())
}

async fn list_lists(State(state): State<Arc<AppState>>) -> ApiResult<Vec<TodoListResponse>> {
    let service = todo_service_from_state(state.as_ref());
    let lists = service.list_lists().await?;
    JsonApiResponse::ok(lists.into_iter().map(TodoListResponse::from).collect())
}

async fn get_list(
    State(state): State<Arc<AppState>>,
    Path(list_id): Path<Uuid>,
) -> ApiResult<TodoListDetailResponse> {
    let list = require_list(state.as_ref(), &list_id).await?;
    let service = todo_service_from_state(state.as_ref());
    let items = service.list_items(&list_id).await?;
    let items = items.into_iter().map(TodoItemResponse::from).collect();
    JsonApiResponse::ok(TodoListDetailResponse {
        list: list.into(),
        items,
    })
}

async fn update_list(
    State(state): State<Arc<AppState>>,
    Path(list_id): Path<Uuid>,
    Json(body): Json<UpdateListRequest>,
) -> ApiResult<TodoListResponse> {
    let title = normalize_title(&body.title)?;
    let service = todo_service_from_state(state.as_ref());
    let list = service.update_list_title(&list_id, title).await?;
    JsonApiResponse::ok(list.into())
}

async fn delete_list(
    State(state): State<Arc<AppState>>,
    Path(list_id): Path<Uuid>,
) -> ApiResult<serde_json::Value> {
    let service = todo_service_from_state(state.as_ref());
    service.delete_list(&list_id).await?;
    JsonApiResponse::with_status(StatusCode::NO_CONTENT, "deleted", serde_json::Value::Null)
}

async fn create_item(
    State(state): State<Arc<AppState>>,
    Path(list_id): Path<Uuid>,
    Json(body): Json<CreateItemRequest>,
) -> ApiResult<TodoItemResponse> {
    let description = normalize_description(&body.description)?;
    require_list(state.as_ref(), &list_id).await?;
    let service = todo_service_from_state(state.as_ref());
    let item = service.create_item(&list_id, description).await?;
    JsonApiResponse::with_status(StatusCode::CREATED, "created", item.into())
}

async fn list_items(
    State(state): State<Arc<AppState>>,
    Path(list_id): Path<Uuid>,
) -> ApiResult<Vec<TodoItemResponse>> {
    require_list(state.as_ref(), &list_id).await?;
    let service = todo_service_from_state(state.as_ref());
    let items = service.list_items(&list_id).await?;
    JsonApiResponse::ok(items.into_iter().map(TodoItemResponse::from).collect())
}

async fn update_item(
    State(state): State<Arc<AppState>>,
    Path((list_id, item_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<UpdateItemRequest>,
) -> ApiResult<TodoItemResponse> {
    let UpdateItemRequest { description, done } = body;
    let description = match description {
        Some(value) => Some(normalize_description(&value)?.to_string()),
        None => None,
    };
    if description.is_none() && done.is_none() {
        return Err(AppError::bad_request("Description or done required"));
    }
    let service = todo_service_from_state(state.as_ref());
    let item = service
        .update_item(&list_id, &item_id, description, done)
        .await?;
    JsonApiResponse::ok(item.into())
}

async fn delete_item(
    State(state): State<Arc<AppState>>,
    Path((list_id, item_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<serde_json::Value> {
    let service = todo_service_from_state(state.as_ref());
    service.delete_item(&list_id, &item_id).await?;
    JsonApiResponse::with_status(StatusCode::NO_CONTENT, "deleted", serde_json::Value::Null)
}

async fn require_list(state: &AppState, list_id: &Uuid) -> Result<todo_list::Model, AppError> {
    let service = todo_service_from_state(state);
    service.require_list(list_id).await
}

fn normalize_title(title: &str) -> Result<&str, AppError> {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        return Err(AppError::bad_request("Title required"));
    }
    Ok(trimmed)
}

fn normalize_description(description: &str) -> Result<&str, AppError> {
    let trimmed = description.trim();
    if trimmed.is_empty() {
        return Err(AppError::bad_request("Description required"));
    }
    Ok(trimmed)
}

impl From<todo_list::Model> for TodoListResponse {
    fn from(model: todo_list::Model) -> Self {
        Self {
            id: model.id,
            title: model.title,
            created_at: model.created_at,
            updated_at: model.updated_at,
        }
    }
}

impl From<todo_item::Model> for TodoItemResponse {
    fn from(model: todo_item::Model) -> Self {
        Self {
            id: model.id,
            list_id: model.list_id,
            description: model.description,
            done: model.done,
            created_at: model.created_at,
            updated_at: model.updated_at,
        }
    }
}

fn todo_service_from_state(state: &AppState) -> todo_service::TodoService {
    ServiceContext::from_state(state).todo()
}
