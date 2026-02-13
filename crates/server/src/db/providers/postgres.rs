use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use sea_orm::{ConnectOptions, Database, DatabaseConnection};

use super::registry::{DbProvider, DbProviderId};
use crate::config::DatabaseConfig;

pub struct PostgresDbProvider;

#[async_trait]
impl DbProvider for PostgresDbProvider {
    fn id(&self) -> DbProviderId {
        DbProviderId::Postgres
    }

    fn supports_url(&self, url: &str) -> bool {
        let normalized = url.trim().to_ascii_lowercase();
        normalized.starts_with("postgres://") || normalized.starts_with("postgresql://")
    }

    async fn connect(&self, cfg: &DatabaseConfig) -> Result<DatabaseConnection> {
        let mut options = ConnectOptions::new(cfg.url.clone());
        options
            .max_connections(cfg.max_connections)
            .min_connections(cfg.min_idle)
            .connect_timeout(Duration::from_secs(5))
            .sqlx_logging(false);

        let db = Database::connect(options).await?;
        Ok(db)
    }
}
