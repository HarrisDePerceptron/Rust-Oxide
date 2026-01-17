use sea_orm::{
    ColumnTrait, DatabaseConnection, EntityTrait, Order, PaginatorTrait, QueryFilter, Set,
};
use uuid::Uuid;

use super::{DaoBase, DaoLayerError, DaoResult};
use crate::db::entities::prelude::{TodoItem, TodoList};
use crate::db::entities::{todo_item, todo_list};

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

#[derive(Clone)]
struct TodoItemDao {
    db: DatabaseConnection,
}

impl DaoBase for TodoItemDao {
    type Entity = TodoItem;

    fn from_db(db: DatabaseConnection) -> Self {
        Self { db }
    }

    fn db(&self) -> &DatabaseConnection {
        &self.db
    }
}

impl TodoDao {
    fn item_dao(&self) -> TodoItemDao {
        TodoItemDao::new(&self.db)
    }

    pub async fn create_list(&self, title: &str) -> DaoResult<todo_list::Model> {
        let model = todo_list::ActiveModel {
            title: Set(title.to_string()),
            ..Default::default()
        };
        self.create(model).await
    }

    pub async fn list_lists(&self) -> DaoResult<Vec<todo_list::Model>> {
        let mut page = 1;
        let page_size = <Self as DaoBase>::MAX_PAGE_SIZE;
        let mut lists = Vec::new();
        loop {
            let mut response = self.find(page, page_size, None, |query| query).await?;
            let has_next = response.has_next;
            lists.append(&mut response.data);
            if !has_next {
                break;
            }
            page += 1;
        }
        Ok(lists)
    }

    pub async fn find_list_by_id(&self, id: &Uuid) -> DaoResult<todo_list::Model> {
        self.find_by_id(*id).await
    }

    pub async fn update_list_title(&self, id: &Uuid, title: &str) -> DaoResult<todo_list::Model> {
        let title = title.to_string();
        self.update(*id, move |active| {
            active.title = Set(title);
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
            list_id: Set(*list_id),
            description: Set(description.to_string()),
            done: Set(false),
            ..Default::default()
        };
        self.item_dao().create(model).await
    }

    pub async fn list_items(&self, list_id: &Uuid) -> DaoResult<Vec<todo_item::Model>> {
        let mut page = 1;
        let page_size = <Self as DaoBase>::MAX_PAGE_SIZE;
        let mut items = Vec::new();
        loop {
            let mut response = self
                .item_dao()
                .find(page, page_size, None, |query| {
                    query.filter(todo_item::Column::ListId.eq(*list_id))
                })
                .await?;
            let has_next = response.has_next;
            items.append(&mut response.data);
            if !has_next {
                break;
            }
            page += 1;
        }
        Ok(items)
    }

    pub async fn find_item_by_id(
        &self,
        list_id: &Uuid,
        item_id: &Uuid,
    ) -> DaoResult<Option<todo_item::Model>> {
        self.item_dao()
            .find(1, 1, None, |query| {
                query
                    .filter(todo_item::Column::Id.eq(*item_id))
                    .filter(todo_item::Column::ListId.eq(*list_id))
            })
            .await
            .map(|response| response.data.into_iter().next())
    }

    pub async fn update_item(
        &self,
        list_id: &Uuid,
        item_id: &Uuid,
        description: Option<String>,
        done: Option<bool>,
    ) -> DaoResult<Option<todo_item::Model>> {
        let Some(_) = self.find_item_by_id(list_id, item_id).await? else {
            return Ok(None);
        };
        let model = self
            .item_dao()
            .update(*item_id, move |active| {
                if let Some(description) = description {
                    active.description = Set(description);
                }
                if let Some(done) = done {
                    active.done = Set(done);
                }
            })
            .await?;
        Ok(Some(model))
    }

    pub async fn delete_item(&self, list_id: &Uuid, item_id: &Uuid) -> DaoResult<bool> {
        let Some(_) = self.find_item_by_id(list_id, item_id).await? else {
            return Ok(false);
        };
        self.item_dao().delete(*item_id).await?;
        Ok(true)
    }

    pub async fn count_items_by_list(&self, list_id: &Uuid) -> DaoResult<u64> {
        TodoItem::find()
            .filter(todo_item::Column::ListId.eq(*list_id))
            .count(&self.db)
            .await
            .map_err(DaoLayerError::Db)
    }
}
