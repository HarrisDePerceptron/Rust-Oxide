use chrono::{Duration, Utc};
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};
use uuid::Uuid;

use super::{DaoBase, DaoLayerError, DaoResult};
use crate::db::entities::refresh_token::{self, Entity as RefreshToken};

const DEFAULT_REFRESH_TTL_DAYS: i64 = 30;

#[derive(Clone)]
pub struct RefreshTokenDao {
    db: DatabaseConnection,
}

impl DaoBase for RefreshTokenDao {
    type Entity = RefreshToken;

    fn from_db(db: DatabaseConnection) -> Self {
        Self { db }
    }

    fn db(&self) -> &DatabaseConnection {
        &self.db
    }
}

impl RefreshTokenDao {
    pub async fn create_refresh_token(
        &self,
        user_id: &Uuid,
        ttl_days: Option<i64>,
    ) -> DaoResult<refresh_token::Model> {
        let expires_at =
            Utc::now().fixed_offset() + Duration::days(ttl_days.unwrap_or(DEFAULT_REFRESH_TTL_DAYS));
        let model = refresh_token::ActiveModel {
            token: Set(Uuid::new_v4().to_string()),
            user_id: Set(*user_id),
            expires_at: Set(expires_at),
            revoked: Set(false),
            ..Default::default()
        };
        self.create(model).await
    }

    pub async fn find_active_by_token(
        &self,
        token: &str,
    ) -> DaoResult<Option<refresh_token::Model>> {
        let token = token.to_string();
        self.find(1, 1, None, move |query| {
            query
                .filter(refresh_token::Column::Token.eq(token))
                .filter(refresh_token::Column::Revoked.eq(false))
        })
        .await
        .map(|response| response.data.into_iter().next())
    }

    pub async fn revoke_token(&self, token: &str) -> DaoResult<()> {
        RefreshToken::update_many()
            .col_expr(
                refresh_token::Column::Revoked,
                sea_orm::sea_query::Expr::value(true),
            )
            .filter(refresh_token::Column::Token.eq(token))
            .exec(&self.db)
            .await
            .map_err(DaoLayerError::Db)?;
        Ok(())
    }
}
