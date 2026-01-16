use axum::http::StatusCode;
use sea_orm::DatabaseConnection;
use uuid::Uuid;

use crate::{
    db::dao::todo_dao,
    db::entities::{todo_item, todo_list},
    error::AppError,
};

pub async fn create_list(
    db: &DatabaseConnection,
    title: &str,
) -> Result<todo_list::Model, AppError> {
    todo_dao::create_list(db, title)
        .await
        .map_err(|_| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, "Create list failed"))
}

pub async fn list_lists(db: &DatabaseConnection) -> Result<Vec<todo_list::Model>, AppError> {
    todo_dao::list_lists(db)
        .await
        .map_err(|_| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, "List fetch failed"))
}

pub async fn require_list(
    db: &DatabaseConnection,
    list_id: &Uuid,
) -> Result<todo_list::Model, AppError> {
    todo_dao::find_list_by_id(db, list_id)
        .await
        .map_err(|_| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, "List fetch failed"))?
        .ok_or_else(|| AppError::new(StatusCode::NOT_FOUND, "Todo list not found"))
}

pub async fn update_list_title(
    db: &DatabaseConnection,
    list_id: &Uuid,
    title: &str,
) -> Result<todo_list::Model, AppError> {
    todo_dao::update_list_title(db, list_id, title)
        .await
        .map_err(|_| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, "Update list failed"))?
        .ok_or_else(|| AppError::new(StatusCode::NOT_FOUND, "Todo list not found"))
}

pub async fn delete_list(db: &DatabaseConnection, list_id: &Uuid) -> Result<(), AppError> {
    let deleted = todo_dao::delete_list(db, list_id)
        .await
        .map_err(|_| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, "Delete list failed"))?;
    if !deleted {
        return Err(AppError::new(StatusCode::NOT_FOUND, "Todo list not found"));
    }
    Ok(())
}

pub async fn create_item(
    db: &DatabaseConnection,
    list_id: &Uuid,
    description: &str,
) -> Result<todo_item::Model, AppError> {
    todo_dao::create_item(db, list_id, description)
        .await
        .map_err(|_| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, "Create item failed"))
}

pub async fn list_items(
    db: &DatabaseConnection,
    list_id: &Uuid,
) -> Result<Vec<todo_item::Model>, AppError> {
    todo_dao::list_items(db, list_id)
        .await
        .map_err(|_| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, "Item fetch failed"))
}

pub async fn update_item(
    db: &DatabaseConnection,
    list_id: &Uuid,
    item_id: &Uuid,
    description: Option<String>,
    done: Option<bool>,
) -> Result<todo_item::Model, AppError> {
    todo_dao::update_item(db, list_id, item_id, description, done)
        .await
        .map_err(|_| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, "Update item failed"))?
        .ok_or_else(|| AppError::new(StatusCode::NOT_FOUND, "Todo item not found"))
}

pub async fn delete_item(
    db: &DatabaseConnection,
    list_id: &Uuid,
    item_id: &Uuid,
) -> Result<(), AppError> {
    let deleted = todo_dao::delete_item(db, list_id, item_id)
        .await
        .map_err(|_| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, "Delete item failed"))?;
    if !deleted {
        return Err(AppError::new(StatusCode::NOT_FOUND, "Todo item not found"));
    }
    Ok(())
}
