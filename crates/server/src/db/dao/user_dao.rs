use sea_orm::{ColumnTrait, DatabaseConnection, QueryFilter, Set};
use uuid::Uuid;

use super::{DaoBase, DaoResult};
use crate::db::entities::user as entity;
use crate::db::entities::{prelude::User, user};

#[derive(Clone)]
pub struct UserDao {
    db: DatabaseConnection,
}

impl DaoBase for UserDao {
    type Entity = User;

    fn new(db: &DatabaseConnection) -> Self {
        Self { db: db.clone() }
    }

    fn db(&self) -> &DatabaseConnection {
        &self.db
    }
}

impl UserDao {
    pub async fn find_by_email(&self, email: &str) -> DaoResult<Option<user::Model>> {
        let email = email.to_string();
        self.find(1, 1, None, move |query| {
            query.filter(entity::Column::Email.eq(email))
        })
        .await
        .map(|response| response.data.into_iter().next())
    }

    pub async fn create_user(
        &self,
        email: &str,
        password_hash: &str,
        role: &str,
    ) -> DaoResult<user::Model> {
        let model = user::ActiveModel {
            email: Set(email.to_string()),
            password_hash: Set(password_hash.to_string()),
            role: Set(role.to_string()),
            last_login_at: Set(None),
            ..Default::default()
        };
        self.create(model).await
    }

    pub async fn touch_updated_at(&self, id: &Uuid) -> DaoResult<()> {
        self.update(*id, |_| {}).await.map(|_| ())
    }

    pub async fn set_last_login(
        &self,
        id: &Uuid,
        at: &chrono::DateTime<chrono::FixedOffset>,
    ) -> DaoResult<()> {
        let at = *at;
        self.update(*id, move |active| {
            active.last_login_at = Set(Some(at));
        })
        .await
        .map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use chrono::{FixedOffset, TimeZone};
    use sea_orm::{DatabaseBackend, MockDatabase};
    use uuid::Uuid;

    use crate::db::entities::user;

    use super::UserDao;
    use crate::db::dao::{DaoBase, DaoLayerError};

    fn ts() -> chrono::DateTime<chrono::FixedOffset> {
        FixedOffset::east_opt(0)
            .expect("offset should be valid")
            .with_ymd_and_hms(2026, 1, 1, 0, 0, 0)
            .single()
            .expect("timestamp should be valid")
    }

    fn user_model(id: Uuid, email: &str) -> user::Model {
        let now = ts();
        user::Model {
            id,
            created_at: now,
            updated_at: now,
            email: email.to_string(),
            password_hash: "hash".to_string(),
            role: "user".to_string(),
            last_login_at: None,
        }
    }

    #[tokio::test]
    async fn find_by_email_returns_first_match() {
        let id = Uuid::new_v4();
        let db = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[user_model(id, "alice@example.com")]])
            .into_connection();
        let dao = UserDao::new(&db);

        let result = dao
            .find_by_email("alice@example.com")
            .await
            .expect("query should succeed");
        assert_eq!(result.map(|u| u.id), Some(id));
    }

    #[tokio::test]
    async fn find_by_email_returns_none_when_missing() {
        let db = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([Vec::<user::Model>::new()])
            .into_connection();
        let dao = UserDao::new(&db);

        let result = dao
            .find_by_email("missing@example.com")
            .await
            .expect("query should succeed");
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn set_last_login_propagates_not_found() {
        let missing_id = Uuid::new_v4();
        let db = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([Vec::<user::Model>::new()])
            .into_connection();
        let dao = UserDao::new(&db);

        let err = dao
            .set_last_login(&missing_id, &ts())
            .await
            .expect_err("update should fail");
        assert!(matches!(
            err,
            DaoLayerError::NotFound { id, .. } if id == missing_id
        ));
    }
}
