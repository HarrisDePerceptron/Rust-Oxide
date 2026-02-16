use serde::{Deserialize, Serialize};

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

impl Default for RealtimeConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_connections: 10_000,
            max_channels_per_connection: 100,
            max_message_bytes: 64 * 1024,
            heartbeat_interval_secs: 20,
            idle_timeout_secs: 60,
            outbound_queue_size: 256,
            emit_rate_per_sec: 100,
            join_rate_per_sec: 50,
        }
    }
}
