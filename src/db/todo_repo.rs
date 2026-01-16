use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter,
    QueryOrder, Set,
};
use uuid::Uuid;

use super::entities::{todo_item, todo_list};
use super::entities::prelude::{TodoItem, TodoList};

pub async fn create_list(
    db: &DatabaseConnection,
    title: &str,
) -> Result<todo_list::Model, sea_orm::DbErr> {
    let model = todo_list::ActiveModel {
        id: Set(Uuid::new_v4()),
        title: Set(title.to_string()),
        ..Default::default()
    };
    model.insert(db).await
}

pub async fn list_lists(
    db: &DatabaseConnection,
) -> Result<Vec<todo_list::Model>, sea_orm::DbErr> {
    TodoList::find()
        .order_by_asc(todo_list::Column::CreatedAt)
        .all(db)
        .await
}

pub async fn find_list_by_id(
    db: &DatabaseConnection,
    id: &Uuid,
) -> Result<Option<todo_list::Model>, sea_orm::DbErr> {
    TodoList::find_by_id(*id).one(db).await
}

pub async fn update_list_title(
    db: &DatabaseConnection,
    id: &Uuid,
    title: &str,
) -> Result<Option<todo_list::Model>, sea_orm::DbErr> {
    let Some(list) = TodoList::find_by_id(*id).one(db).await? else {
        return Ok(None);
    };
    let now = Utc::now().fixed_offset();
    let mut active: todo_list::ActiveModel = list.into();
    active.title = Set(title.to_string());
    active.updated_at = Set(now);
    Ok(Some(active.update(db).await?))
}

pub async fn delete_list(db: &DatabaseConnection, id: &Uuid) -> Result<bool, sea_orm::DbErr> {
    let result = TodoList::delete_by_id(*id).exec(db).await?;
    Ok(result.rows_affected > 0)
}

pub async fn create_item(
    db: &DatabaseConnection,
    list_id: &Uuid,
    description: &str,
) -> Result<todo_item::Model, sea_orm::DbErr> {
    let model = todo_item::ActiveModel {
        id: Set(Uuid::new_v4()),
        list_id: Set(*list_id),
        description: Set(description.to_string()),
        done: Set(false),
        ..Default::default()
    };
    model.insert(db).await
}

pub async fn list_items(
    db: &DatabaseConnection,
    list_id: &Uuid,
) -> Result<Vec<todo_item::Model>, sea_orm::DbErr> {
    TodoItem::find()
        .filter(todo_item::Column::ListId.eq(*list_id))
        .order_by_asc(todo_item::Column::CreatedAt)
        .all(db)
        .await
}

pub async fn find_item_by_id(
    db: &DatabaseConnection,
    list_id: &Uuid,
    item_id: &Uuid,
) -> Result<Option<todo_item::Model>, sea_orm::DbErr> {
    TodoItem::find()
        .filter(todo_item::Column::Id.eq(*item_id))
        .filter(todo_item::Column::ListId.eq(*list_id))
        .one(db)
        .await
}

pub async fn update_item(
    db: &DatabaseConnection,
    list_id: &Uuid,
    item_id: &Uuid,
    description: Option<String>,
    done: Option<bool>,
) -> Result<Option<todo_item::Model>, sea_orm::DbErr> {
    let Some(item) = find_item_by_id(db, list_id, item_id).await? else {
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
    Ok(Some(active.update(db).await?))
}

pub async fn delete_item(
    db: &DatabaseConnection,
    list_id: &Uuid,
    item_id: &Uuid,
) -> Result<bool, sea_orm::DbErr> {
    let result = TodoItem::delete_many()
        .filter(todo_item::Column::Id.eq(*item_id))
        .filter(todo_item::Column::ListId.eq(*list_id))
        .exec(db)
        .await?;
    Ok(result.rows_affected > 0)
}

pub async fn count_items_by_list(
    db: &DatabaseConnection,
    list_id: &Uuid,
) -> Result<u64, sea_orm::DbErr> {
    TodoItem::find()
        .filter(todo_item::Column::ListId.eq(*list_id))
        .count(db)
        .await
}
