use axum::http::StatusCode;
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
            Err(err) => Err(AppError::new(StatusCode::BAD_REQUEST, err.to_string())),
        }
    }
}
