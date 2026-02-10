use uuid::Uuid;

use crate::{
    db::dao::{DaoBase, DaoLayerError, UserDao},
    db::entities::user,
    error::AppError,
};

#[derive(Clone)]
pub struct UserService {
    user_dao: UserDao,
}

impl UserService {
    pub fn new(user_dao: UserDao) -> Self {
        Self { user_dao }
    }

    pub async fn find_by_id(&self, id: &Uuid) -> Result<Option<user::Model>, AppError> {
        match self.user_dao.find_by_id(*id).await {
            Ok(model) => Ok(Some(model)),
            Err(DaoLayerError::NotFound { .. }) => Ok(None),
            Err(DaoLayerError::Db(db_err)) => Err(AppError::internal_with_source(
                "database operation failed. Please check the logs for more details",
                db_err,
            )),
            Err(err) => Err(AppError::bad_request(err.to_string())),
        }
    }

    pub async fn find_by_email(&self, email: &str) -> Result<Option<user::Model>, AppError> {
        match self.user_dao.find_by_email(email).await {
            Ok(model) => Ok(model),
            Err(DaoLayerError::NotFound { .. }) => Ok(None),
            Err(DaoLayerError::Db(db_err)) => Err(AppError::internal_with_source(
                "database operation failed. Please check the logs for more details",
                db_err,
            )),
            Err(err) => Err(AppError::bad_request(err.to_string())),
        }
    }

    pub async fn create_user(
        &self,
        email: &str,
        password_hash: &str,
        role: &str,
    ) -> Result<user::Model, AppError> {
        Ok(self
            .user_dao
            .create_user(email, password_hash, role)
            .await?)
    }

    pub async fn set_last_login(
        &self,
        user_id: &Uuid,
        last_login: &chrono::DateTime<chrono::FixedOffset>,
    ) -> Result<(), AppError> {
        Ok(self.user_dao.set_last_login(user_id, last_login).await?)
    }
}

#[cfg(test)]
mod tests {
    use sea_orm::{DatabaseBackend, DbErr, MockDatabase};
    use uuid::Uuid;

    use super::UserService;
    use crate::db::dao::{DaoBase, UserDao};

    #[tokio::test]
    async fn find_by_id_returns_none_for_not_found() {
        let db = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([Vec::<crate::db::entities::user::Model>::new()])
            .into_connection();
        let service = UserService::new(UserDao::new(&db));

        let result = service
            .find_by_id(&Uuid::new_v4())
            .await
            .expect("query should succeed");
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn find_by_id_maps_db_error_to_internal() {
        let db = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_errors([DbErr::Custom("query failed".to_string())])
            .into_connection();
        let service = UserService::new(UserDao::new(&db));

        let err = service
            .find_by_id(&Uuid::new_v4())
            .await
            .expect_err("query should fail");
        assert_eq!(
            err.message(),
            "database operation failed. Please check the logs for more details"
        );
    }

    #[tokio::test]
    async fn find_by_email_maps_db_error_to_internal() {
        let db = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_errors([DbErr::Custom("query failed".to_string())])
            .into_connection();
        let service = UserService::new(UserDao::new(&db));

        let err = service
            .find_by_email("alice@example.com")
            .await
            .expect_err("query should fail");
        assert_eq!(
            err.message(),
            "database operation failed. Please check the logs for more details"
        );
    }
}
