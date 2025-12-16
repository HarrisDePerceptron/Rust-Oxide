use anyhow::{Context, Result};

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub host: String,
    pub port: u16,
    pub jwt_secret: String,
    pub log_level: String,
}

impl AppConfig {
    pub fn from_env() -> Result<Self> {
        // Load .env if present
        let _ = dotenvy::dotenv();

        let host = std::env::var("HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
        let port = std::env::var("PORT")
            .unwrap_or_else(|_| "3000".to_string())
            .parse::<u16>()
            .context("PORT must be a valid u16")?;
        let log_level =
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info,tower_http=info".to_string());

        let jwt_secret = match std::env::var("JWT_SECRET") {
            Ok(val) => val,
            Err(_) if cfg!(debug_assertions) => "super-secret-change-me".to_string(),
            Err(err) => {
                Err(anyhow::anyhow!(err)).context("JWT_SECRET is required in release builds")?
            }
        };

        Ok(Self {
            host,
            port,
            jwt_secret,
            log_level,
        })
    }
}
