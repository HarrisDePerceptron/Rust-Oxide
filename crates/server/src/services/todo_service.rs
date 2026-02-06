use sea_orm::Set;
use uuid::Uuid;

use crate::{
    db::dao::TodoDao,
    db::entities::{todo_item, todo_list},
    error::AppError,
    services::crud_service::CrudService,
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
        let model = todo_list::ActiveModel {
            title: Set(title.to_string()),
            ..Default::default()
        };

        CrudService::create(self, model).await
    }

    pub async fn list_lists(&self) -> Result<Vec<todo_list::Model>, AppError> {
        Ok(self.todo_dao.list_lists().await?)
    }

    pub async fn count_lists(&self) -> Result<u64, AppError> {
        Ok(self.todo_dao.count_lists().await.unwrap())
    }

    pub async fn require_list(&self, list_id: &Uuid) -> Result<todo_list::Model, AppError> {
        CrudService::find_by_id(self, *list_id).await
    }

    pub async fn update_list_title(
        &self,
        list_id: &Uuid,
        title: &str,
    ) -> Result<todo_list::Model, AppError> {
        let title = title.to_string();
        CrudService::update(self, *list_id, move |active| {
            active.title = Set(title);
        })
        .await
    }

    pub async fn delete_list(&self, list_id: &Uuid) -> Result<(), AppError> {
        CrudService::delete(self, *list_id).await
    }

    pub async fn create_item(
        &self,
        list_id: &Uuid,
        description: &str,
    ) -> Result<todo_item::Model, AppError> {
        Ok(self.todo_dao.create_item(list_id, description).await?)
    }

    pub async fn list_items(&self, list_id: &Uuid) -> Result<Vec<todo_item::Model>, AppError> {
        Ok(self.todo_dao.list_items(list_id).await?)
    }

    pub async fn count_items_by_list(&self, list_id: &Uuid) -> Result<u64, AppError> {
        Ok(self.todo_dao.count_items_by_list(list_id).await?)
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
            .await?
            .ok_or_else(|| AppError::not_found("Todo item not found"))
    }

    pub async fn delete_item(&self, list_id: &Uuid, item_id: &Uuid) -> Result<(), AppError> {
        let deleted = self.todo_dao.delete_item(list_id, item_id).await?;
        if !deleted {
            return Err(AppError::not_found("Todo item not found"));
        }
        Ok(())
    }
}

impl CrudService for TodoService {
    type Dao = TodoDao;

    fn dao(&self) -> &Self::Dao {
        &self.todo_dao
    }
}
