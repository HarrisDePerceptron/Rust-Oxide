use sea_orm::DatabaseConnection;
use tracing::info;

use crate::config::DatabaseConfig;
use crate::db::providers::default_registry;

pub async fn connect(cfg: &DatabaseConfig) -> anyhow::Result<DatabaseConnection> {
    let providers = default_registry()?;
    let provider = providers.provider_for_url(&cfg.url)?;

    info!(provider = provider.id().as_str(), "connecting to database");
    let db = provider.connect(cfg).await?;
    provider.post_connect(&db, cfg).await?;

    info!("syncing database schema from entities");
    db.get_schema_registry("rust_oxide::db::entities::*")
        .sync(&db)
        .await?;
    Ok(db)
}
