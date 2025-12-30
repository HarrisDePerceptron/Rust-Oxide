use chrono::{Duration, Utc};
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};
use uuid::Uuid;

use super::entities::refresh_token::{self, Entity as RefreshToken};

const DEFAULT_REFRESH_TTL_DAYS: i64 = 30;

pub async fn create_refresh_token(
    db: &DatabaseConnection,
    user_id: &Uuid,
    ttl_days: Option<i64>,
) -> Result<refresh_token::Model, sea_orm::DbErr> {
    let expires_at =
        Utc::now().fixed_offset() + Duration::days(ttl_days.unwrap_or(DEFAULT_REFRESH_TTL_DAYS));
    let model = refresh_token::ActiveModel {
        id: Set(Uuid::new_v4()),
        token: Set(Uuid::new_v4().to_string()),
        user_id: Set(*user_id),
        expires_at: Set(expires_at),
        created_at: Set(Utc::now().fixed_offset()),
        revoked: Set(false),
    };
    model.insert(db).await
}

pub async fn find_active_by_token(
    db: &DatabaseConnection,
    token: &str,
) -> Result<Option<refresh_token::Model>, sea_orm::DbErr> {
    RefreshToken::find()
        .filter(refresh_token::Column::Token.eq(token))
        .filter(refresh_token::Column::Revoked.eq(false))
        .one(db)
        .await
}

pub async fn revoke_token(db: &DatabaseConnection, token: &str) -> Result<(), sea_orm::DbErr> {
    RefreshToken::update_many()
        .col_expr(
            refresh_token::Column::Revoked,
            sea_orm::sea_query::Expr::value(true),
        )
        .filter(refresh_token::Column::Token.eq(token))
        .exec(db)
        .await?;
    Ok(())
}
