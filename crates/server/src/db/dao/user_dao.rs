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
