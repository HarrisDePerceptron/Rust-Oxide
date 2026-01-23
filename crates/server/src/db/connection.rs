use std::time::Duration;

use sea_orm::{ConnectOptions, Database, DatabaseConnection};
use tracing::info;

use crate::config::AppConfig;

pub async fn connect(cfg: &AppConfig) -> anyhow::Result<DatabaseConnection> {
    let mut options = ConnectOptions::new(cfg.database_url.clone());
    options
        .max_connections(cfg.db_max_connections)
        .min_connections(cfg.db_min_idle)
        .connect_timeout(Duration::from_secs(5))
        .sqlx_logging(false);

    let db = Database::connect(options).await?;
    info!("syncing database schema from entities");
    db.get_schema_registry("sample_server::db::entities::*")
        .sync(&db)
        .await?;
    Ok(db)
}
