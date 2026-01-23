use std::sync::Arc;

use jsonwebtoken::{DecodingKey, EncodingKey};
use sea_orm::DatabaseConnection;

use crate::config::AppConfig;

#[derive(Clone)]
pub struct AppState {
    pub config: AppConfig,
    pub jwt: JwtKeys,
    pub db: DatabaseConnection,
}

#[derive(Clone)]
pub struct JwtKeys {
    pub enc: EncodingKey,
    pub dec: DecodingKey,
}

impl JwtKeys {
    pub fn from_secret(secret: &[u8]) -> Self {
        Self {
            enc: EncodingKey::from_secret(secret),
            dec: DecodingKey::from_secret(secret),
        }
    }
}

impl AppState {
    pub fn new(config: AppConfig, db: DatabaseConnection) -> Arc<Self> {
        Arc::new(Self {
            jwt: JwtKeys::from_secret(config.jwt_secret.as_bytes()),
            db,
            config,
        })
    }
}
