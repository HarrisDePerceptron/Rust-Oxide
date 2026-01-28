use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use axum::http::StatusCode;

use crate::{
    auth::{Claims, TokenBundle},
    config::AppConfig,
    error::AppError,
};

pub mod local;
pub use local::LocalAuthProvider;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AuthProviderId {
    Local,
}

impl AuthProviderId {
    pub fn as_str(self) -> &'static str {
        match self {
            AuthProviderId::Local => "local",
        }
    }
}

impl std::str::FromStr for AuthProviderId {
    type Err = String;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        match raw.to_ascii_lowercase().as_str() {
            "local" => Ok(AuthProviderId::Local),
            other => Err(format!("unsupported auth provider: {}", other)),
        }
    }
}

#[async_trait]
pub trait AuthProvider: Send + Sync {
    fn id(&self) -> AuthProviderId;

    async fn register(&self, email: &str, password: &str) -> Result<TokenBundle, AppError>;
    async fn login(&self, email: &str, password: &str) -> Result<TokenBundle, AppError>;
    async fn refresh(&self, refresh_token: &str) -> Result<TokenBundle, AppError>;
    async fn verify(&self, access_token: &str) -> Result<Claims, AppError>;

    async fn seed_admin(&self, _cfg: &AppConfig) -> anyhow::Result<()> {
        Ok(())
    }
}

#[derive(Clone)]
pub struct AuthProviders {
    active_id: AuthProviderId,
    active: Arc<dyn AuthProvider>,
    providers: HashMap<AuthProviderId, Arc<dyn AuthProvider>>,
}

impl AuthProviders {
    pub fn new(
        active_id: AuthProviderId,
        providers: Vec<Arc<dyn AuthProvider>>,
    ) -> Result<Self, AppError> {
        let mut map = HashMap::new();
        for provider in providers {
            map.insert(provider.id(), provider);
        }

        let active = map
            .get(&active_id)
            .cloned()
            .ok_or_else(|| {
                AppError::new(
                    StatusCode::BAD_REQUEST,
                    format!("Auth provider not configured: {}", active_id.as_str()),
                )
            })?;

        Ok(Self {
            active_id,
            active,
            providers: map,
        })
    }

    pub fn active_id(&self) -> AuthProviderId {
        self.active_id
    }

    pub fn active(&self) -> &dyn AuthProvider {
        self.active.as_ref()
    }

    pub fn get(&self, id: AuthProviderId) -> Option<&Arc<dyn AuthProvider>> {
        self.providers.get(&id)
    }
}
