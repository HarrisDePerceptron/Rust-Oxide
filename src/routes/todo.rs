use std::sync::Arc;

use askama::Template;
use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::Html,
    routing::{get, patch, post},
};
use chrono::Local;
use sea_orm::prelude::DateTimeWithTimeZone;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    db::dao::DaoContext,
    db::entities::{todo_item, todo_list},
    error::AppError,
    services::todo_service,
    state::AppState,
};

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

#[derive(Template)]
#[template(path = "todo.html")]
struct TodoUiTemplate {
    now: String,
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/todo/ui", get(todo_ui))
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
        .with_state(state)
}

async fn todo_ui() -> Result<Html<String>, AppError> {
    let now = Local::now().to_rfc3339();
    let rendered = TodoUiTemplate { now }.render().map_err(|_| {
        AppError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to render todo ui",
        )
    })?;
    Ok(Html(rendered))
}

async fn create_list(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateListRequest>,
) -> Result<(StatusCode, Json<TodoListResponse>), AppError> {
    let title = normalize_title(&body.title)?;
    let service = todo_service_from_state(state.as_ref());
    let list = service.create_list(title).await?;
    Ok((StatusCode::CREATED, Json(list.into())))
}

async fn list_lists(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<TodoListResponse>>, AppError> {
    let service = todo_service_from_state(state.as_ref());
    let lists = service.list_lists().await?;
    Ok(Json(
        lists.into_iter().map(TodoListResponse::from).collect(),
    ))
}

async fn get_list(
    State(state): State<Arc<AppState>>,
    Path(list_id): Path<Uuid>,
) -> Result<Json<TodoListDetailResponse>, AppError> {
    let list = require_list(state.as_ref(), &list_id).await?;
    let service = todo_service_from_state(state.as_ref());
    let items = service.list_items(&list_id).await?;
    let items = items.into_iter().map(TodoItemResponse::from).collect();
    Ok(Json(TodoListDetailResponse {
        list: list.into(),
        items,
    }))
}

async fn update_list(
    State(state): State<Arc<AppState>>,
    Path(list_id): Path<Uuid>,
    Json(body): Json<UpdateListRequest>,
) -> Result<Json<TodoListResponse>, AppError> {
    let title = normalize_title(&body.title)?;
    let service = todo_service_from_state(state.as_ref());
    let list = service.update_list_title(&list_id, title).await?;
    Ok(Json(list.into()))
}

async fn delete_list(
    State(state): State<Arc<AppState>>,
    Path(list_id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    let service = todo_service_from_state(state.as_ref());
    service.delete_list(&list_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn create_item(
    State(state): State<Arc<AppState>>,
    Path(list_id): Path<Uuid>,
    Json(body): Json<CreateItemRequest>,
) -> Result<(StatusCode, Json<TodoItemResponse>), AppError> {
    let description = normalize_description(&body.description)?;
    require_list(state.as_ref(), &list_id).await?;
    let service = todo_service_from_state(state.as_ref());
    let item = service.create_item(&list_id, description).await?;
    Ok((StatusCode::CREATED, Json(item.into())))
}

async fn list_items(
    State(state): State<Arc<AppState>>,
    Path(list_id): Path<Uuid>,
) -> Result<Json<Vec<TodoItemResponse>>, AppError> {
    require_list(state.as_ref(), &list_id).await?;
    let service = todo_service_from_state(state.as_ref());
    let items = service.list_items(&list_id).await?;
    Ok(Json(
        items.into_iter().map(TodoItemResponse::from).collect(),
    ))
}

async fn update_item(
    State(state): State<Arc<AppState>>,
    Path((list_id, item_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<UpdateItemRequest>,
) -> Result<Json<TodoItemResponse>, AppError> {
    let UpdateItemRequest { description, done } = body;
    let description = match description {
        Some(value) => Some(normalize_description(&value)?.to_string()),
        None => None,
    };
    if description.is_none() && done.is_none() {
        return Err(AppError::new(
            StatusCode::BAD_REQUEST,
            "Description or done required",
        ));
    }
    let service = todo_service_from_state(state.as_ref());
    let item = service
        .update_item(&list_id, &item_id, description, done)
        .await?;
    Ok(Json(item.into()))
}

async fn delete_item(
    State(state): State<Arc<AppState>>,
    Path((list_id, item_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, AppError> {
    let service = todo_service_from_state(state.as_ref());
    service.delete_item(&list_id, &item_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn require_list(state: &AppState, list_id: &Uuid) -> Result<todo_list::Model, AppError> {
    let service = todo_service_from_state(state);
    service.require_list(list_id).await
}

fn normalize_title(title: &str) -> Result<&str, AppError> {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        return Err(AppError::new(StatusCode::BAD_REQUEST, "Title required"));
    }
    Ok(trimmed)
}

fn normalize_description(description: &str) -> Result<&str, AppError> {
    let trimmed = description.trim();
    if trimmed.is_empty() {
        return Err(AppError::new(
            StatusCode::BAD_REQUEST,
            "Description required",
        ));
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
    let daos = DaoContext::new(&state.db);
    todo_service::TodoService::new(daos.todo())
}
