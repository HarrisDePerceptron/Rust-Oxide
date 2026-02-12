use std::time::Duration;

use sea_orm::{ConnectOptions, Database, DatabaseConnection};
use tracing::info;

use crate::config::DatabaseConfig;

pub async fn connect(cfg: &DatabaseConfig) -> anyhow::Result<DatabaseConnection> {
    let mut options = ConnectOptions::new(cfg.url.clone());
    options
        .max_connections(cfg.max_connections)
        .min_connections(cfg.min_idle)
        .connect_timeout(Duration::from_secs(5))
        .sqlx_logging(false);

    let db = Database::connect(options).await?;
    info!("syncing database schema from entities");
    db.get_schema_registry("rust_oxide::db::entities::*")
        .sync(&db)
        .await?;
    Ok(db)
}
