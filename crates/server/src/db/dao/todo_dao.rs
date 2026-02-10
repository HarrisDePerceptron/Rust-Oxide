use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter, Set};
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

    fn new(db: &DatabaseConnection) -> Self {
        Self { db: db.clone() }
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

    fn new(db: &DatabaseConnection) -> Self {
        Self { db: db.clone() }
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
        let mut pager = self.find_iter(None, None, |query| query);
        let mut lists = Vec::new();
        while let Some(mut response) = pager.next_page().await? {
            lists.append(&mut response.data);
        }
        Ok(lists)
    }

    pub async fn count_lists(&self) -> DaoResult<u64> {
        TodoList::find()
            .count(&self.db)
            .await
            .map_err(DaoLayerError::Db)
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
        let mut pager = self.item_dao().find_iter(None, None, |query| {
            query.filter(todo_item::Column::ListId.eq(*list_id))
        });
        let mut items = Vec::new();
        while let Some(mut response) = pager.next_page().await? {
            items.append(&mut response.data);
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

#[cfg(test)]
mod tests {
    use chrono::{FixedOffset, TimeZone};
    use sea_orm::{DatabaseBackend, DbErr, MockDatabase};
    use uuid::Uuid;

    use crate::db::entities::todo_item;

    use super::TodoDao;
    use crate::db::dao::{DaoBase, DaoLayerError};

    fn ts() -> chrono::DateTime<chrono::FixedOffset> {
        FixedOffset::east_opt(0)
            .expect("offset should be valid")
            .with_ymd_and_hms(2026, 1, 1, 0, 0, 0)
            .single()
            .expect("timestamp should be valid")
    }

    fn item_model(id: Uuid, list_id: Uuid, description: &str, done: bool) -> todo_item::Model {
        let now = ts();
        todo_item::Model {
            id,
            created_at: now,
            updated_at: now,
            list_id,
            description: description.to_string(),
            done,
        }
    }

    #[tokio::test]
    async fn update_item_returns_none_when_item_is_missing() {
        let list_id = Uuid::new_v4();
        let item_id = Uuid::new_v4();
        let db = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([Vec::<todo_item::Model>::new()])
            .into_connection();
        let dao = TodoDao::new(&db);

        let result = dao
            .update_item(&list_id, &item_id, Some("new".to_string()), Some(true))
            .await
            .expect("query should succeed");
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn delete_item_returns_false_when_item_is_missing() {
        let list_id = Uuid::new_v4();
        let item_id = Uuid::new_v4();
        let db = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([Vec::<todo_item::Model>::new()])
            .into_connection();
        let dao = TodoDao::new(&db);

        let deleted = dao
            .delete_item(&list_id, &item_id)
            .await
            .expect("query should succeed");
        assert!(!deleted);
    }

    #[tokio::test]
    async fn find_item_by_id_returns_item_when_present() {
        let list_id = Uuid::new_v4();
        let item_id = Uuid::new_v4();
        let db = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[item_model(item_id, list_id, "item", false)]])
            .into_connection();
        let dao = TodoDao::new(&db);

        let result = dao
            .find_item_by_id(&list_id, &item_id)
            .await
            .expect("query should succeed");
        assert_eq!(result.map(|item| item.id), Some(item_id));
    }

    #[tokio::test]
    async fn count_items_by_list_maps_database_errors() {
        let list_id = Uuid::new_v4();
        let db = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_errors([DbErr::Custom("count failed".to_string())])
            .into_connection();
        let dao = TodoDao::new(&db);

        let err = dao
            .count_items_by_list(&list_id)
            .await
            .expect_err("count should fail");
        assert!(matches!(err, DaoLayerError::Db(_)));
    }
}
