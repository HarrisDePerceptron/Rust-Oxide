use std::{collections::HashMap, sync::Arc};

use anyhow::{Result, bail};
use async_trait::async_trait;
use sea_orm::DatabaseConnection;

use crate::config::DatabaseConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DbProviderId {
    Postgres,
    Sqlite,
}

impl DbProviderId {
    pub fn as_str(self) -> &'static str {
        match self {
            DbProviderId::Postgres => "postgres",
            DbProviderId::Sqlite => "sqlite",
        }
    }
}

#[async_trait]
pub trait DbProvider: Send + Sync {
    fn id(&self) -> DbProviderId;
    fn supports_url(&self, url: &str) -> bool;
    async fn connect(&self, cfg: &DatabaseConfig) -> Result<DatabaseConnection>;
    async fn post_connect(&self, _db: &DatabaseConnection, _cfg: &DatabaseConfig) -> Result<()> {
        Ok(())
    }
}

pub struct DbProviders {
    providers: HashMap<DbProviderId, Arc<dyn DbProvider>>,
}

impl DbProviders {
    pub fn new() -> Self {
        Self {
            providers: HashMap::new(),
        }
    }

    pub fn with_provider(mut self, provider: Arc<dyn DbProvider>) -> Result<Self> {
        self.add(provider)?;
        Ok(self)
    }

    pub fn add(&mut self, provider: Arc<dyn DbProvider>) -> Result<()> {
        let id = provider.id();
        if self.providers.contains_key(&id) {
            bail!("database provider already registered: {}", id.as_str());
        }
        self.providers.insert(id, provider);
        Ok(())
    }

    pub fn provider_for_url(&self, url: &str) -> Result<Arc<dyn DbProvider>> {
        self.providers
            .values()
            .find(|provider| provider.supports_url(url))
            .cloned()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "unsupported database url '{}'; expected scheme postgres://, postgresql://, or sqlite://",
                    redact_url(url)
                )
            })
    }
}

impl Default for DbProviders {
    fn default() -> Self {
        Self::new()
    }
}

fn redact_url(url: &str) -> String {
    let trimmed = url.trim();
    if let Some((scheme, _)) = trimmed.split_once("://") {
        format!("{scheme}://<redacted>")
    } else if let Some((scheme, _)) = trimmed.split_once(':') {
        format!("{scheme}:<redacted>")
    } else {
        "<invalid-url>".to_string()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use anyhow::Result;
    use async_trait::async_trait;
    use sea_orm::{DatabaseConnection, MockDatabase};

    use super::{DbProvider, DbProviderId, DbProviders};
    use crate::config::DatabaseConfig;

    struct TestProvider {
        id: DbProviderId,
        accepted_prefix: &'static str,
    }

    #[async_trait]
    impl DbProvider for TestProvider {
        fn id(&self) -> DbProviderId {
            self.id
        }

        fn supports_url(&self, url: &str) -> bool {
            url.starts_with(self.accepted_prefix)
        }

        async fn connect(&self, _cfg: &DatabaseConfig) -> Result<DatabaseConnection> {
            Ok(MockDatabase::new(sea_orm::DatabaseBackend::Sqlite).into_connection())
        }
    }

    #[test]
    fn rejects_duplicate_provider_registration() {
        let provider = Arc::new(TestProvider {
            id: DbProviderId::Sqlite,
            accepted_prefix: "sqlite://",
        });
        let mut providers = DbProviders::new();

        providers
            .add(provider.clone())
            .expect("first provider registration should succeed");
        let err = providers
            .add(provider)
            .expect_err("duplicate provider registration should fail");

        assert!(err.to_string().contains("already registered"));
    }

    #[test]
    fn resolves_provider_by_url() {
        let providers = DbProviders::new()
            .with_provider(Arc::new(TestProvider {
                id: DbProviderId::Postgres,
                accepted_prefix: "postgres://",
            }))
            .expect("postgres provider should register")
            .with_provider(Arc::new(TestProvider {
                id: DbProviderId::Sqlite,
                accepted_prefix: "sqlite://",
            }))
            .expect("sqlite provider should register");

        let sqlite = providers
            .provider_for_url("sqlite://./app.db")
            .expect("sqlite provider should resolve");
        let postgres = providers
            .provider_for_url("postgres://localhost/db")
            .expect("postgres provider should resolve");

        assert_eq!(sqlite.id(), DbProviderId::Sqlite);
        assert_eq!(postgres.id(), DbProviderId::Postgres);
    }

    #[test]
    fn returns_error_for_unsupported_scheme() {
        let providers = DbProviders::new();
        let err = match providers.provider_for_url("mysql://localhost/db") {
            Ok(_) => panic!("unsupported url should fail"),
            Err(err) => err,
        };

        assert!(err.to_string().contains("unsupported database url"));
    }
}
