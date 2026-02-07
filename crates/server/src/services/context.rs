use sea_orm::DatabaseConnection;

use crate::{
    auth::providers::AuthProviders,
    db::dao::{DaoContext, RefreshTokenDao},
    services::{auth_service::AuthService, todo_service::TodoService, user_service::UserService},
    state::AppState,
};

#[derive(Clone)]
pub struct ServiceContext {
    daos: DaoContext,
}

impl ServiceContext {
    pub fn new(db: &DatabaseConnection) -> Self {
        Self {
            daos: DaoContext::new(db),
        }
    }

    pub fn from_state(state: &AppState) -> Self {
        Self::new(&state.db)
    }

    pub fn user(&self) -> UserService {
        UserService::new(self.daos.user())
    }

    pub fn todo(&self) -> TodoService {
        TodoService::new(self.daos.todo())
    }

    pub fn auth<'a>(&self, providers: &'a AuthProviders) -> AuthService<'a> {
        AuthService::new(providers)
    }

    pub fn refresh_token_dao(&self) -> RefreshTokenDao {
        self.daos.refresh_token()
    }
}
