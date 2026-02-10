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

    fn new(db: &DatabaseConnection) -> Self {
        Self { db: db.clone() }
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
        let expires_at = Utc::now().fixed_offset()
            + Duration::days(ttl_days.unwrap_or(DEFAULT_REFRESH_TTL_DAYS));
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

#[cfg(test)]
mod tests {
    use chrono::{Duration, FixedOffset, TimeZone};
    use sea_orm::{DatabaseBackend, DbErr, MockDatabase};
    use uuid::Uuid;

    use crate::db::entities::refresh_token;

    use super::RefreshTokenDao;
    use crate::db::dao::{DaoBase, DaoLayerError};

    fn ts() -> chrono::DateTime<chrono::FixedOffset> {
        FixedOffset::east_opt(0)
            .expect("offset should be valid")
            .with_ymd_and_hms(2026, 1, 1, 0, 0, 0)
            .single()
            .expect("timestamp should be valid")
    }

    fn token_model(token: &str, user_id: Uuid, revoked: bool) -> refresh_token::Model {
        let now = ts();
        refresh_token::Model {
            id: Uuid::new_v4(),
            created_at: now,
            updated_at: now,
            token: token.to_string(),
            user_id,
            expires_at: now + Duration::days(30),
            revoked,
        }
    }

    #[tokio::test]
    async fn find_active_by_token_returns_none_when_missing() {
        let db = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([Vec::<refresh_token::Model>::new()])
            .into_connection();
        let dao = RefreshTokenDao::new(&db);

        let result = dao
            .find_active_by_token("missing-token")
            .await
            .expect("query should succeed");
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn find_active_by_token_returns_token_when_present() {
        let user_id = Uuid::new_v4();
        let db = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[token_model("token-1", user_id, false)]])
            .into_connection();
        let dao = RefreshTokenDao::new(&db);

        let token = dao
            .find_active_by_token("token-1")
            .await
            .expect("query should succeed")
            .expect("token should exist");
        assert_eq!(token.user_id, user_id);
        assert_eq!(token.token, "token-1");
        assert!(!token.revoked);
    }

    #[tokio::test]
    async fn revoke_token_maps_database_errors() {
        let db = MockDatabase::new(DatabaseBackend::Postgres)
            .append_exec_errors([DbErr::Custom("update failed".to_string())])
            .into_connection();
        let dao = RefreshTokenDao::new(&db);

        let err = dao
            .revoke_token("token-1")
            .await
            .expect_err("update should fail");
        assert!(matches!(err, DaoLayerError::Db(_)));
    }
}
