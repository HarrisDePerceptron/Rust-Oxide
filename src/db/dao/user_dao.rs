use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set,
    sea_query::Expr,
};
use uuid::Uuid;

use crate::db::entities::{prelude::User, user};

pub async fn find_by_email(
    db: &DatabaseConnection,
    email: &str,
) -> Result<Option<user::Model>, sea_orm::DbErr> {
    User::find()
        .filter(user::Column::Email.eq(email))
        .one(db)
        .await
}

pub async fn find_by_id(
    db: &DatabaseConnection,
    id: &Uuid,
) -> Result<Option<user::Model>, sea_orm::DbErr> {
    User::find_by_id(*id).one(db).await
}

pub async fn create_user(
    db: &DatabaseConnection,
    email: &str,
    password_hash: &str,
    role: &str,
) -> Result<user::Model, sea_orm::DbErr> {
    let model = user::ActiveModel {
        id: Set(Uuid::new_v4()),
        email: Set(email.to_string()),
        password_hash: Set(password_hash.to_string()),
        role: Set(role.to_string()),
        last_login_at: Set(None),
        ..Default::default()
    };
    model.insert(db).await
}

pub async fn touch_updated_at(db: &DatabaseConnection, id: &Uuid) -> Result<(), sea_orm::DbErr> {
    let now = Utc::now().fixed_offset();
    User::update_many()
        .col_expr(user::Column::UpdatedAt, Expr::value(now))
        .filter(user::Column::Id.eq(*id))
        .exec(db)
        .await?;
    Ok(())
}

pub async fn set_last_login(
    db: &DatabaseConnection,
    id: &Uuid,
    at: &chrono::DateTime<chrono::FixedOffset>,
) -> Result<(), sea_orm::DbErr> {
    user::ActiveModel {
        id: Set(*id),
        last_login_at: Set(Some(*at)),
        ..Default::default()
    }
    .update(db)
    .await?;
    Ok(())
}
