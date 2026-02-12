use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::{
    auth::{Claims, TokenBundle},
    config::AuthConfig,
    error::AppError,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
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

    async fn seed_admin(&self, _cfg: &AuthConfig) -> anyhow::Result<()> {
        Ok(())
    }
}

#[derive(Clone)]
pub struct AuthProviders {
    active_id: AuthProviderId,
    providers: HashMap<AuthProviderId, Arc<dyn AuthProvider>>,
}

impl AuthProviders {
    pub fn new(active_id: AuthProviderId) -> Self {
        Self {
            active_id,
            providers: HashMap::new(),
        }
    }

    pub fn with_provider(mut self, provider: Arc<dyn AuthProvider>) -> Result<Self, AppError> {
        self.add(provider)?;
        Ok(self)
    }

    pub fn add(&mut self, provider: Arc<dyn AuthProvider>) -> Result<(), AppError> {
        let id = provider.id();
        if self.providers.contains_key(&id) {
            return Err(AppError::conflict(format!(
                "Auth provider already registered: {}",
                id.as_str()
            )));
        }
        self.providers.insert(id, provider);
        Ok(())
    }

    pub fn set_active(&mut self, id: AuthProviderId) -> Result<(), AppError> {
        if self.providers.contains_key(&id) {
            self.active_id = id;
            Ok(())
        } else {
            Err(AppError::bad_request(format!(
                "Auth provider not configured: {}",
                id.as_str()
            )))
        }
    }

    pub fn active_id(&self) -> AuthProviderId {
        self.active_id
    }

    pub fn active(&self) -> Result<&dyn AuthProvider, AppError> {
        self.providers
            .get(&self.active_id)
            .map(|provider| provider.as_ref())
            .ok_or_else(|| {
                AppError::bad_request(format!(
                    "Auth provider not configured: {}",
                    self.active_id.as_str()
                ))
            })
    }

    pub fn get(&self, id: AuthProviderId) -> Option<&Arc<dyn AuthProvider>> {
        self.providers.get(&id)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use async_trait::async_trait;

    use crate::{
        auth::{Claims, TokenBundle},
        error::AppError,
    };

    use super::{AuthProvider, AuthProviderId, AuthProviders};

    #[derive(Clone)]
    struct TestProvider {
        id: AuthProviderId,
    }

    #[async_trait]
    impl AuthProvider for TestProvider {
        fn id(&self) -> AuthProviderId {
            self.id
        }

        async fn register(&self, _email: &str, _password: &str) -> Result<TokenBundle, AppError> {
            Err(AppError::unauthorized("not used"))
        }

        async fn login(&self, _email: &str, _password: &str) -> Result<TokenBundle, AppError> {
            Err(AppError::unauthorized("not used"))
        }

        async fn refresh(&self, _refresh_token: &str) -> Result<TokenBundle, AppError> {
            Err(AppError::unauthorized("not used"))
        }

        async fn verify(&self, _access_token: &str) -> Result<Claims, AppError> {
            Err(AppError::unauthorized("not used"))
        }
    }

    #[test]
    fn provider_id_parser_is_case_insensitive_for_supported_provider() {
        assert_eq!("local".parse::<AuthProviderId>(), Ok(AuthProviderId::Local));
        assert_eq!("LOCAL".parse::<AuthProviderId>(), Ok(AuthProviderId::Local));
        assert!("oauth".parse::<AuthProviderId>().is_err());
    }

    #[test]
    fn duplicate_provider_registration_is_rejected() {
        let provider = Arc::new(TestProvider {
            id: AuthProviderId::Local,
        });
        let mut providers = AuthProviders::new(AuthProviderId::Local);
        providers
            .add(provider.clone())
            .expect("first registration should succeed");

        let err = providers
            .add(provider)
            .expect_err("duplicate registration should fail");
        assert_eq!(err.message(), "Auth provider already registered: local");
    }

    #[test]
    fn active_provider_must_be_configured() {
        let providers = AuthProviders::new(AuthProviderId::Local);
        let err = match providers.active() {
            Ok(_) => panic!("missing provider should fail"),
            Err(err) => err,
        };
        assert_eq!(err.message(), "Auth provider not configured: local");
    }

    #[test]
    fn set_active_requires_existing_provider() {
        let mut providers = AuthProviders::new(AuthProviderId::Local);
        let provider = Arc::new(TestProvider {
            id: AuthProviderId::Local,
        });
        providers
            .add(provider)
            .expect("registration should succeed");

        providers
            .set_active(AuthProviderId::Local)
            .expect("active provider should be set");

        assert_eq!(providers.active_id(), AuthProviderId::Local);
        assert!(providers.get(AuthProviderId::Local).is_some());
    }
}
