use sea_orm::DbErr;
use uuid::Uuid;

#[derive(Debug)]
pub enum DaoLayerError {
    Db(DbErr),
    NotFound { entity: &'static str, id: Uuid },
    InvalidPagination { page: u64, page_size: u64 },
}

pub type DaoResult<T> = Result<T, DaoLayerError>;
