use axum::http::StatusCode;
use uuid::Uuid;

use crate::{
    db::dao::{DaoLayerError, TodoDao},
    db::entities::{todo_item, todo_list},
    error::AppError,
};

#[derive(Clone)]
pub struct TodoService {
    todo_dao: TodoDao,
}

impl TodoService {
    pub fn new(todo_dao: TodoDao) -> Self {
        Self { todo_dao }
    }

    pub async fn create_list(&self, title: &str) -> Result<todo_list::Model, AppError> {
        self.todo_dao
            .create_list(title)
            .await
            .map_err(|_| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, "Create list failed"))
    }

    pub async fn list_lists(&self) -> Result<Vec<todo_list::Model>, AppError> {
        self.todo_dao
            .list_lists()
            .await
            .map_err(|_| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, "List fetch failed"))
    }

    pub async fn require_list(&self, list_id: &Uuid) -> Result<todo_list::Model, AppError> {
        match self.todo_dao.find_list_by_id(list_id).await {
            Ok(list) => Ok(list),
            Err(DaoLayerError::NotFound { .. }) => {
                Err(AppError::new(StatusCode::NOT_FOUND, "Todo list not found"))
            }
            Err(_) => Err(AppError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "List fetch failed",
            )),
        }
    }

    pub async fn update_list_title(
        &self,
        list_id: &Uuid,
        title: &str,
    ) -> Result<todo_list::Model, AppError> {
        match self.todo_dao.update_list_title(list_id, title).await {
            Ok(list) => Ok(list),
            Err(DaoLayerError::NotFound { .. }) => {
                Err(AppError::new(StatusCode::NOT_FOUND, "Todo list not found"))
            }
            Err(_) => Err(AppError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Update list failed",
            )),
        }
    }

    pub async fn delete_list(&self, list_id: &Uuid) -> Result<(), AppError> {
        match self.todo_dao.delete_list(list_id).await {
            Ok(_) => Ok(()),
            Err(DaoLayerError::NotFound { .. }) => {
                Err(AppError::new(StatusCode::NOT_FOUND, "Todo list not found"))
            }
            Err(_) => Err(AppError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Delete list failed",
            )),
        }
    }

    pub async fn create_item(
        &self,
        list_id: &Uuid,
        description: &str,
    ) -> Result<todo_item::Model, AppError> {
        self.todo_dao
            .create_item(list_id, description)
            .await
            .map_err(|_| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, "Create item failed"))
    }

    pub async fn list_items(&self, list_id: &Uuid) -> Result<Vec<todo_item::Model>, AppError> {
        self.todo_dao
            .list_items(list_id)
            .await
            .map_err(|_| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, "Item fetch failed"))
    }

    pub async fn update_item(
        &self,
        list_id: &Uuid,
        item_id: &Uuid,
        description: Option<String>,
        done: Option<bool>,
    ) -> Result<todo_item::Model, AppError> {
        self.todo_dao
            .update_item(list_id, item_id, description, done)
            .await
            .map_err(|_| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, "Update item failed"))?
            .ok_or_else(|| AppError::new(StatusCode::NOT_FOUND, "Todo item not found"))
    }

    pub async fn delete_item(&self, list_id: &Uuid, item_id: &Uuid) -> Result<(), AppError> {
        let deleted = self
            .todo_dao
            .delete_item(list_id, item_id)
            .await
            .map_err(|_| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, "Delete item failed"))?;
        if !deleted {
            return Err(AppError::new(StatusCode::NOT_FOUND, "Todo item not found"));
        }
        Ok(())
    }
}
