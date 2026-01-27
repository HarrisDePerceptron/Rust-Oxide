use sea_orm::DbErr;
use uuid::Uuid;
use std::fmt;

#[derive(Debug)]
pub enum DaoLayerError {
    Db(DbErr),
    NotFound { entity: &'static str, id: Uuid },
    InvalidPagination { page: u64, page_size: u64 },
}

pub type DaoResult<T> = Result<T, DaoLayerError>;

impl fmt::Display for DaoLayerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DaoLayerError::Db(err) => write!(f, "Database error: {err}"),
            DaoLayerError::NotFound { entity, id } => {
                write!(f, "{entity} not found (id={id})")
            }
            DaoLayerError::InvalidPagination { page, page_size } => write!(
                f,
                "Invalid pagination: page={page} page_size={page_size}"
            ),
        }
    }
}

impl std::error::Error for DaoLayerError {}
