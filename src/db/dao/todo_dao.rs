use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter,
    QueryOrder, Set,
};
use uuid::Uuid;

use super::{DaoBase, DaoLayerError, DaoResult};
use crate::db::entities::{todo_item, todo_list};
use crate::db::entities::prelude::{TodoItem, TodoList};

#[derive(Clone)]
pub struct TodoDao {
    db: DatabaseConnection,
}

impl DaoBase for TodoDao {
    type Entity = TodoList;

    fn from_db(db: DatabaseConnection) -> Self {
        Self { db }
    }

    fn db(&self) -> &DatabaseConnection {
        &self.db
    }
}

impl TodoDao {
    pub async fn create_list(
        &self,
        title: &str,
    ) -> DaoResult<todo_list::Model> {
        let model = todo_list::ActiveModel {
            id: Set(Uuid::new_v4()),
            title: Set(title.to_string()),
            ..Default::default()
        };
        self.create(model).await
    }

    pub async fn list_lists(&self) -> DaoResult<Vec<todo_list::Model>> {
        TodoList::find()
            .order_by_asc(todo_list::Column::CreatedAt)
            .all(&self.db)
            .await
            .map_err(DaoLayerError::Db)
    }

    pub async fn find_list_by_id(&self, id: &Uuid) -> DaoResult<todo_list::Model> {
        self.find_by_id(*id).await
    }

    pub async fn update_list_title(
        &self,
        id: &Uuid,
        title: &str,
    ) -> DaoResult<todo_list::Model> {
        let title = title.to_string();
        let now = Utc::now().fixed_offset();
        self.update(*id, move |active| {
            active.title = Set(title);
            active.updated_at = Set(now);
        })
        .await
    }

    pub async fn delete_list(&self, id: &Uuid) -> DaoResult<Uuid> {
        self.delete(*id).await
    }

    pub async fn create_item(
        &self,
        list_id: &Uuid,
        description: &str,
    ) -> DaoResult<todo_item::Model> {
        let model = todo_item::ActiveModel {
            id: Set(Uuid::new_v4()),
            list_id: Set(*list_id),
            description: Set(description.to_string()),
            done: Set(false),
            ..Default::default()
        };
        model.insert(&self.db).await.map_err(DaoLayerError::Db)
    }

    pub async fn list_items(
        &self,
        list_id: &Uuid,
    ) -> DaoResult<Vec<todo_item::Model>> {
        TodoItem::find()
            .filter(todo_item::Column::ListId.eq(*list_id))
            .order_by_asc(todo_item::Column::CreatedAt)
            .all(&self.db)
            .await
            .map_err(DaoLayerError::Db)
    }

    pub async fn find_item_by_id(
        &self,
        list_id: &Uuid,
        item_id: &Uuid,
    ) -> DaoResult<Option<todo_item::Model>> {
        TodoItem::find()
            .filter(todo_item::Column::Id.eq(*item_id))
            .filter(todo_item::Column::ListId.eq(*list_id))
            .one(&self.db)
            .await
            .map_err(DaoLayerError::Db)
    }

    pub async fn update_item(
        &self,
        list_id: &Uuid,
        item_id: &Uuid,
        description: Option<String>,
        done: Option<bool>,
    ) -> DaoResult<Option<todo_item::Model>> {
        let Some(item) = self.find_item_by_id(list_id, item_id).await? else {
            return Ok(None);
        };
        let mut active: todo_item::ActiveModel = item.into();
        if let Some(description) = description {
            active.description = Set(description);
        }
        if let Some(done) = done {
            active.done = Set(done);
        }
        active.updated_at = Set(Utc::now().fixed_offset());
        let model = active.update(&self.db).await.map_err(DaoLayerError::Db)?;
        Ok(Some(model))
    }

    pub async fn delete_item(
        &self,
        list_id: &Uuid,
        item_id: &Uuid,
    ) -> DaoResult<bool> {
        let result = TodoItem::delete_many()
            .filter(todo_item::Column::Id.eq(*item_id))
            .filter(todo_item::Column::ListId.eq(*list_id))
            .exec(&self.db)
            .await
            .map_err(DaoLayerError::Db)?;
        Ok(result.rows_affected > 0)
    }

    pub async fn count_items_by_list(
        &self,
        list_id: &Uuid,
    ) -> DaoResult<u64> {
        TodoItem::find()
            .filter(todo_item::Column::ListId.eq(*list_id))
            .count(&self.db)
            .await
            .map_err(DaoLayerError::Db)
    }
}
