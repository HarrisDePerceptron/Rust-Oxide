use std::sync::Arc;

use sea_orm::DatabaseConnection;

use crate::{auth::providers::AuthProviders, config::AppConfig};

#[derive(Clone)]
pub struct AppState {
    pub config: AppConfig,
    pub db: DatabaseConnection,
    pub auth_providers: AuthProviders,
}

impl AppState {
    pub fn new(
        config: AppConfig,
        db: DatabaseConnection,
        auth_providers: AuthProviders,
    ) -> Arc<Self> {
        Arc::new(Self {
            db,
            config,
            auth_providers,
        })
    }
}
