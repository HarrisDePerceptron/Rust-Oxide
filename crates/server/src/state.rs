use std::sync::Arc;

use jsonwebtoken::{DecodingKey, EncodingKey};
use sea_orm::DatabaseConnection;

use crate::{auth::providers::AuthProviders, config::AppConfig};

#[derive(Clone)]
pub struct AppState {
    pub config: AppConfig,
    pub jwt: JwtKeys,
    pub db: DatabaseConnection,
    pub auth_providers: AuthProviders,
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
    pub fn new(
        config: AppConfig,
        db: DatabaseConnection,
        jwt: JwtKeys,
        auth_providers: AuthProviders,
    ) -> Arc<Self> {
        Arc::new(Self {
            db,
            config,
            jwt,
            auth_providers,
        })
    }
}
