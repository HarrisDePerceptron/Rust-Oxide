use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use sea_orm::{ConnectOptions, ConnectionTrait, Database, DatabaseConnection};

use super::registry::{DbProvider, DbProviderId};
use crate::config::DatabaseConfig;

const SQLITE_BUSY_TIMEOUT_MS: u64 = 5_000;

pub struct SqliteDbProvider;

#[async_trait]
impl DbProvider for SqliteDbProvider {
    fn id(&self) -> DbProviderId {
        DbProviderId::Sqlite
    }

    fn supports_url(&self, url: &str) -> bool {
        url.trim().to_ascii_lowercase().starts_with("sqlite:")
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

    async fn post_connect(&self, db: &DatabaseConnection, _cfg: &DatabaseConfig) -> Result<()> {
        db.execute_unprepared("PRAGMA foreign_keys = ON").await?;
        db.execute_unprepared(&format!("PRAGMA busy_timeout = {SQLITE_BUSY_TIMEOUT_MS}"))
            .await?;
        Ok(())
    }
}
