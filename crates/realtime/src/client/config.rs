use std::time::Duration;

#[derive(Debug, Clone)]
pub struct ClientConfig {
    pub request_timeout: Duration,
    pub ping_interval: Duration,
    pub outbound_buffer: usize,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            request_timeout: Duration::from_secs(5),
            ping_interval: Duration::from_secs(20),
            outbound_buffer: 256,
        }
    }
}
