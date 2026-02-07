use sea_orm::DatabaseConnection;

use super::{DaoBase, RefreshTokenDao, TodoDao, UserDao};

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
