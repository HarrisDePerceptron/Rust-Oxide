use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::auth::providers::AuthProviderId;

use super::{defaults, envconfig::EnvConfig, validate};

#[cfg(feature = "realtime")]
pub use realtime::server::RealtimeConfig;

#[cfg(not(feature = "realtime"))]
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct RealtimeConfig {
    pub enabled: bool,
    pub max_connections: usize,
    pub max_channels_per_connection: usize,
    pub max_message_bytes: usize,
    pub heartbeat_interval_secs: u64,
    pub idle_timeout_secs: u64,
    pub outbound_queue_size: usize,
    pub emit_rate_per_sec: u32,
    pub join_rate_per_sec: u32,
}

#[cfg(not(feature = "realtime"))]
impl Default for RealtimeConfig {
    fn default() -> Self {
        Self {
            enabled: defaults::DEFAULT_REALTIME_ENABLED,
            max_connections: defaults::DEFAULT_REALTIME_MAX_CONNECTIONS,
            max_channels_per_connection: defaults::DEFAULT_REALTIME_MAX_CHANNELS_PER_CONNECTION,
            max_message_bytes: defaults::DEFAULT_REALTIME_MAX_MESSAGE_BYTES,
            heartbeat_interval_secs: defaults::DEFAULT_REALTIME_HEARTBEAT_INTERVAL_SECS,
            idle_timeout_secs: defaults::DEFAULT_REALTIME_IDLE_TIMEOUT_SECS,
            outbound_queue_size: defaults::DEFAULT_REALTIME_OUTBOUND_QUEUE_SIZE,
            emit_rate_per_sec: defaults::DEFAULT_REALTIME_EMIT_RATE_PER_SEC,
            join_rate_per_sec: defaults::DEFAULT_REALTIME_JOIN_RATE_PER_SEC,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct AppConfig {
    pub general: GeneralConfig,
    pub logging: LoggingConfig,
    pub database: Option<DatabaseConfig>,
    pub auth: Option<AuthConfig>,
    pub realtime: RealtimeConfig,
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
