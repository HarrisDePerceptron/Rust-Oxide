use anyhow::{Context, Result};

use crate::auth::providers::AuthProviderId;

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub host: String,
    pub port: u16,
    pub jwt_secret: String,
    pub log_level: String,
    pub database_url: String,
    pub db_max_connections: u32,
    pub db_min_idle: u32,
    pub admin_email: String,
    pub admin_password: String,
    pub auth_provider: AuthProviderId,
}

impl AppConfig {
    pub fn from_env() -> Result<Self> {
        // Load .env from crate root (falls back to current dir if missing)
        let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let _ = dotenvy::from_filename(manifest_dir.join(".env")).or_else(|_| dotenvy::dotenv());

        let host = std::env::var("HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
        let port = std::env::var("PORT")
            .unwrap_or_else(|_| "3000".to_string())
            .parse::<u16>()
            .context("PORT must be a valid u16")?;
        let log_level =
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info,tower_http=info".to_string());

        let database_url = match std::env::var("DATABASE_URL") {
            Ok(val) => val,
            Err(_) if cfg!(debug_assertions) => {
                "postgres://postgres:postgres@localhost:5432/rust_oxide".to_string()
            }
            Err(err) => {
                Err(anyhow::anyhow!(err)).context("DATABASE_URL is required in release builds")?
            }
        };

        let db_max_connections = std::env::var("DB_MAX_CONNS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(10);

        let db_min_idle = std::env::var("DB_MIN_IDLE")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(2);

        let admin_email = match std::env::var("ADMIN_EMAIL") {
            Ok(val) => val,
            Err(_) if cfg!(debug_assertions) => "admin@example.com".to_string(),
            Err(err) => {
                Err(anyhow::anyhow!(err)).context("ADMIN_EMAIL is required in release builds")?
            }
        };

        let admin_password = match std::env::var("ADMIN_PASSWORD") {
            Ok(val) => val,
            Err(_) if cfg!(debug_assertions) => "adminpassword".to_string(),
            Err(err) => {
                Err(anyhow::anyhow!(err)).context("ADMIN_PASSWORD is required in release builds")?
            }
        };

        let jwt_secret = match std::env::var("JWT_SECRET") {
            Ok(val) => val,
            Err(_) if cfg!(debug_assertions) => "super-secret-change-me".to_string(),
            Err(err) => {
                Err(anyhow::anyhow!(err)).context("JWT_SECRET is required in release builds")?
            }
        };

        let auth_provider = std::env::var("AUTH_PROVIDER")
            .unwrap_or_else(|_| "local".to_string())
            .parse::<AuthProviderId>()
            .map_err(|err| anyhow::anyhow!(err))
            .context("AUTH_PROVIDER must be a supported provider")?;

        Ok(Self {
            host,
            port,
            jwt_secret,
            log_level,
            database_url,
            db_max_connections,
            db_min_idle,
            admin_email,
            admin_password,
            auth_provider,
        })
    }
}
