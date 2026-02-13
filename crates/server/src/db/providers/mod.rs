mod postgres;
mod registry;
mod sqlite;

use std::sync::Arc;

pub use registry::{DbProvider, DbProviderId, DbProviders};

use self::{postgres::PostgresDbProvider, sqlite::SqliteDbProvider};

pub fn default_registry() -> anyhow::Result<DbProviders> {
    DbProviders::new()
        .with_provider(Arc::new(PostgresDbProvider))?
        .with_provider(Arc::new(SqliteDbProvider))
}
