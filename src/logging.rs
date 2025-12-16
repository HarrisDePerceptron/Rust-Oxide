use tracing_subscriber::{EnvFilter, fmt};

pub fn init_tracing(log_level: &str) {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(log_level));
    fmt().with_env_filter(filter).with_target(false).init();
}
