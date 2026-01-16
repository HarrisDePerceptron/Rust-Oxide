use axum::http::StatusCode;
use sea_orm::DatabaseConnection;
use uuid::Uuid;

use crate::{
    db::dao::user_dao,
    db::entities::user,
    error::AppError,
};

pub async fn find_by_id(
    db: &DatabaseConnection,
    id: &Uuid,
) -> Result<Option<user::Model>, AppError> {
    user_dao::find_by_id(db, id)
        .await
        .map_err(|_| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, "DB error"))
}
