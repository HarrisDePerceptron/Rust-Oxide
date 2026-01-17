use sea_orm::DatabaseConnection;

pub mod base;
pub mod base_traits;
pub mod error;
pub mod refresh_token_dao;
pub mod todo_dao;
pub mod user_dao;

pub use base::DaoBase;
pub use base_traits::{HasIdActiveModel, TimestampedActiveModel};
pub use error::{DaoLayerError, DaoResult};
pub use refresh_token_dao::RefreshTokenDao;
pub use todo_dao::TodoDao;
pub use user_dao::UserDao;

#[derive(Clone)]
pub struct DaoContext {
    db: DatabaseConnection,
}

impl DaoContext {
    pub fn new(db: &DatabaseConnection) -> Self {
        Self { db: db.clone() }
    }

    pub fn user(&self) -> UserDao {
        DaoBase::new(&self.db)
    }

    pub fn refresh_token(&self) -> RefreshTokenDao {
        DaoBase::new(&self.db)
    }

    pub fn todo(&self) -> TodoDao {
        DaoBase::new(&self.db)
    }
}
