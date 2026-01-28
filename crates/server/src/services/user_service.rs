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
            Err(err) => Err(AppError::bad_request(err.to_string())),
        }
    }

    pub async fn find_by_email(&self, email: &str) -> Result<Option<user::Model>, AppError> {
        match self.user_dao.find_by_email(email).await {
            Ok(model) => Ok(model),
            Err(DaoLayerError::NotFound { .. }) => Ok(None),
            Err(err) => Err(AppError::bad_request(err.to_string())),
        }
    }

    pub async fn create_user(
        &self,
        email: &str,
        password_hash: &str,
        role: &str,
    ) -> Result<user::Model, AppError> {
        Ok(self.user_dao.create_user(email, password_hash, role).await?)
    }

    pub async fn set_last_login(
        &self,
        user_id: &Uuid,
        last_login: &chrono::DateTime<chrono::FixedOffset>,
    ) -> Result<(), AppError> {
        Ok(self.user_dao.set_last_login(user_id, last_login).await?)
    }
}
