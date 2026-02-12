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

    if errors.is_empty() {
        return Ok(());
    }

    bail!("invalid app config:\n- {}", errors.join("\n- "))
}
