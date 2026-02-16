use anyhow::{Result, bail};

use super::AppConfig;

pub fn validate(cfg: &AppConfig) -> Result<()> {
    let mut errors: Vec<String> = Vec::new();

    if cfg.general.host.trim().is_empty() {
        errors.push("general.host must not be empty".to_string());
    }

    if let Some(database) = cfg.database.as_ref() {
        if database.url.trim().is_empty() {
            errors.push("database.url must not be empty".to_string());
        }

        if database.min_idle > database.max_connections {
            errors.push(format!(
                "database.min_idle ({}) must be <= database.max_connections ({})",
                database.min_idle, database.max_connections
            ));
        }
    }

    if let Some(auth) = cfg.auth.as_ref() {
        if auth.admin_email.trim().is_empty() {
            errors.push("auth.admin_email must not be empty".to_string());
        }

        if auth.admin_password.len() < 8 {
            errors.push("auth.admin_password must be at least 8 characters".to_string());
        }

        if auth.jwt_secret.trim().is_empty() {
            errors.push("auth.jwt_secret must not be empty".to_string());
        }
    }

    if cfg.realtime.max_connections == 0 {
        errors.push("realtime.max_connections must be > 0".to_string());
    }

    if cfg.realtime.max_channels_per_connection == 0 {
        errors.push("realtime.max_channels_per_connection must be > 0".to_string());
    }

    if cfg.realtime.max_message_bytes == 0 {
        errors.push("realtime.max_message_bytes must be > 0".to_string());
    }

    if cfg.realtime.heartbeat_interval_secs == 0 {
        errors.push("realtime.heartbeat_interval_secs must be > 0".to_string());
    }

    if cfg.realtime.idle_timeout_secs <= cfg.realtime.heartbeat_interval_secs {
        errors.push(
            "realtime.idle_timeout_secs must be greater than realtime.heartbeat_interval_secs"
                .to_string(),
        );
    }

    if cfg.realtime.outbound_queue_size == 0 {
        errors.push("realtime.outbound_queue_size must be > 0".to_string());
    }

    if cfg.realtime.emit_rate_per_sec == 0 {
        errors.push("realtime.emit_rate_per_sec must be > 0".to_string());
    }

    if cfg.realtime.join_rate_per_sec == 0 {
        errors.push("realtime.join_rate_per_sec must be > 0".to_string());
    }

    if errors.is_empty() {
        return Ok(());
    }

    bail!("invalid app config:\n- {}", errors.join("\n- "))
}
