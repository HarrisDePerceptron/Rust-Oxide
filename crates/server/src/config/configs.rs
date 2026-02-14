use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::auth::providers::AuthProviderId;

use super::{defaults, envconfig::EnvConfig, validate};

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct AppConfig {
    pub general: GeneralConfig,
    pub logging: LoggingConfig,
    pub database: Option<DatabaseConfig>,
    pub auth: Option<AuthConfig>,
}

impl AppConfig {
    pub fn from_env() -> Result<Self> {
        <Self as EnvConfig>::from_env()
    }
}

impl EnvConfig for AppConfig {
    fn validate(&self) -> Result<()> {
        validate::validate(self)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct GeneralConfig {
    pub host: String,
    pub port: u16,
    pub enable_docs_in_release: bool,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            host: defaults::DEFAULT_HOST.to_string(),
            port: defaults::DEFAULT_PORT as u16,
            enable_docs_in_release: defaults::DEFAULT_ENABLE_DOCS_IN_RELEASE,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct LoggingConfig {
    pub rust_log: String,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            rust_log: defaults::DEFAULT_RUST_LOG.to_string(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DatabaseConfig {
    pub url: String,
    #[serde(default = "default_db_max_connections")]
    pub max_connections: u32,
    #[serde(default = "default_db_min_idle")]
    pub min_idle: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AuthConfig {
    #[serde(default = "default_auth_provider")]
    pub provider: AuthProviderId,
    pub jwt_secret: String,
    pub admin_email: String,
    pub admin_password: String,
}

fn default_db_max_connections() -> u32 {
    defaults::DEFAULT_DB_MAX_CONNECTIONS as u32
}

fn default_db_min_idle() -> u32 {
    defaults::DEFAULT_DB_MIN_IDLE as u32
}

fn default_auth_provider() -> AuthProviderId {
    AuthProviderId::Local
}
