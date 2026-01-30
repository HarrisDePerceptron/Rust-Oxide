use std::backtrace::Backtrace;

use tracing_subscriber::{EnvFilter, fmt};

pub fn init_tracing(log_level: &str) {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(log_level));
    fmt().with_env_filter(filter).with_target(false).init();
    set_panic_hook();
}

fn set_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let message = if let Some(message) = info.payload().downcast_ref::<&str>() {
            *message
        } else if let Some(message) = info.payload().downcast_ref::<String>() {
            message.as_str()
        } else {
            "unknown panic"
        };

        let backtrace = Backtrace::capture();

        if let Some(location) = info.location() {
            tracing::error!(
                panic = %message,
                location = %location,
                backtrace = %backtrace,
                "panic"
            );
        } else {
            tracing::error!(panic = %message, backtrace = %backtrace, "panic");
        }
    }));
}
