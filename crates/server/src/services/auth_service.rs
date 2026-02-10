use crate::{
    auth::{Claims, TokenBundle, providers::AuthProviders},
    config::AppConfig,
    error::AppError,
};

#[derive(Clone, Copy)]
pub struct AuthService<'a> {
    providers: &'a AuthProviders,
}

impl<'a> AuthService<'a> {
    pub fn new(providers: &'a AuthProviders) -> Self {
        Self { providers }
    }

    pub async fn register(&self, email: &str, password: &str) -> Result<TokenBundle, AppError> {
        self.providers.active()?.register(email, password).await
    }

    pub async fn login(&self, email: &str, password: &str) -> Result<TokenBundle, AppError> {
        self.providers.active()?.login(email, password).await
    }

    pub async fn refresh(&self, refresh_token: &str) -> Result<TokenBundle, AppError> {
        self.providers.active()?.refresh(refresh_token).await
    }

    pub async fn verify(&self, access_token: &str) -> Result<Claims, AppError> {
        self.providers.active()?.verify(access_token).await
    }

    pub async fn seed_admin(&self, cfg: &AppConfig) -> anyhow::Result<()> {
        self.providers
            .active()
            .map_err(|err| anyhow::anyhow!(err.to_string()))?
            .seed_admin(cfg)
            .await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use async_trait::async_trait;

    use crate::{
        auth::{
            Role,
            providers::{AuthProvider, AuthProviderId, AuthProviders},
        },
        config::AppConfig,
    };

    use super::*;

    #[derive(Clone, Copy)]
    enum ProviderMode {
        Success,
        RegisterError,
        LoginError,
        RefreshError,
        VerifyError,
        SeedAdminError,
    }

    #[derive(Clone)]
    struct BoundaryProvider {
        mode: ProviderMode,
    }

    #[async_trait]
    impl AuthProvider for BoundaryProvider {
        fn id(&self) -> AuthProviderId {
            AuthProviderId::Local
        }

        async fn register(&self, _email: &str, _password: &str) -> Result<TokenBundle, AppError> {
            match self.mode {
                ProviderMode::RegisterError => Err(AppError::conflict("duplicate user")),
                _ => Ok(token_bundle("register:ok")),
            }
        }

        async fn login(&self, _email: &str, _password: &str) -> Result<TokenBundle, AppError> {
            match self.mode {
                ProviderMode::LoginError => Err(AppError::unauthorized("invalid credentials")),
                _ => Ok(token_bundle("login:ok")),
            }
        }

        async fn refresh(&self, _refresh_token: &str) -> Result<TokenBundle, AppError> {
            match self.mode {
                ProviderMode::RefreshError => Err(AppError::unauthorized("invalid refresh token")),
                _ => Ok(token_bundle("refresh:ok")),
            }
        }

        async fn verify(&self, _access_token: &str) -> Result<Claims, AppError> {
            match self.mode {
                ProviderMode::VerifyError => {
                    Err(AppError::unauthorized("invalid or expired token"))
                }
                _ => Ok(claims("subject-ok")),
            }
        }

        async fn seed_admin(&self, _cfg: &AppConfig) -> anyhow::Result<()> {
            match self.mode {
                ProviderMode::SeedAdminError => Err(anyhow::anyhow!("seed admin failed")),
                _ => Ok(()),
            }
        }
    }

    fn token_bundle(access_token: &str) -> TokenBundle {
        TokenBundle {
            access_token: access_token.to_string(),
            refresh_token: "refresh-token".to_string(),
            token_type: "Bearer",
            expires_in: 900,
        }
    }

    fn claims(subject: &str) -> Claims {
        Claims {
            sub: subject.to_string(),
            exp: 100,
            iat: 10,
            roles: vec![Role::User],
        }
    }

    fn test_config() -> AppConfig {
        AppConfig::default()
    }

    fn providers_with(mode: ProviderMode) -> AuthProviders {
        AuthProviders::new(AuthProviderId::Local)
            .with_provider(Arc::new(BoundaryProvider { mode }))
            .expect("provider registration should succeed")
    }

    fn providers_without_active_provider() -> AuthProviders {
        AuthProviders::new(AuthProviderId::Local)
    }

    #[tokio::test]
    async fn register_returns_provider_result_on_success() {
        let providers = providers_with(ProviderMode::Success);
        let service = AuthService::new(&providers);

        let result = service
            .register("alice@example.com", "password123")
            .await
            .expect("register should succeed");

        assert_eq!(result.access_token, "register:ok");
    }

    #[tokio::test]
    async fn register_propagates_provider_error() {
        let providers = providers_with(ProviderMode::RegisterError);
        let service = AuthService::new(&providers);

        let err = service
            .register("alice@example.com", "password123")
            .await
            .expect_err("register should fail");

        assert!(matches!(err, AppError::Conflict(_)));
    }

    #[tokio::test]
    async fn register_returns_bad_request_when_active_provider_missing() {
        let providers = providers_without_active_provider();
        let service = AuthService::new(&providers);

        let err = service
            .register("alice@example.com", "password123")
            .await
            .expect_err("register should fail");

        assert!(matches!(err, AppError::BadRequest(_)));
    }

    #[tokio::test]
    async fn login_returns_provider_result_on_success() {
        let providers = providers_with(ProviderMode::Success);
        let service = AuthService::new(&providers);

        let result = service
            .login("alice@example.com", "password123")
            .await
            .expect("login should succeed");

        assert_eq!(result.access_token, "login:ok");
    }

    #[tokio::test]
    async fn login_propagates_provider_error() {
        let providers = providers_with(ProviderMode::LoginError);
        let service = AuthService::new(&providers);

        let err = service
            .login("alice@example.com", "password123")
            .await
            .expect_err("login should fail");

        assert!(matches!(err, AppError::Unauthorized(_)));
    }

    #[tokio::test]
    async fn login_returns_bad_request_when_active_provider_missing() {
        let providers = providers_without_active_provider();
        let service = AuthService::new(&providers);

        let err = service
            .login("alice@example.com", "password123")
            .await
            .expect_err("login should fail");

        assert!(matches!(err, AppError::BadRequest(_)));
    }

    #[tokio::test]
    async fn refresh_returns_provider_result_on_success() {
        let providers = providers_with(ProviderMode::Success);
        let service = AuthService::new(&providers);

        let result = service
            .refresh("refresh-token-1")
            .await
            .expect("refresh should succeed");

        assert_eq!(result.access_token, "refresh:ok");
    }

    #[tokio::test]
    async fn refresh_propagates_provider_error() {
        let providers = providers_with(ProviderMode::RefreshError);
        let service = AuthService::new(&providers);

        let err = service
            .refresh("refresh-token-1")
            .await
            .expect_err("refresh should fail");

        assert!(matches!(err, AppError::Unauthorized(_)));
    }

    #[tokio::test]
    async fn refresh_returns_bad_request_when_active_provider_missing() {
        let providers = providers_without_active_provider();
        let service = AuthService::new(&providers);

        let err = service
            .refresh("refresh-token-1")
            .await
            .expect_err("refresh should fail");

        assert!(matches!(err, AppError::BadRequest(_)));
    }

    #[tokio::test]
    async fn verify_returns_provider_result_on_success() {
        let providers = providers_with(ProviderMode::Success);
        let service = AuthService::new(&providers);

        let result = service
            .verify("access-token")
            .await
            .expect("verify should succeed");

        assert_eq!(result.sub, "subject-ok");
    }

    #[tokio::test]
    async fn verify_propagates_provider_error() {
        let providers = providers_with(ProviderMode::VerifyError);
        let service = AuthService::new(&providers);

        let err = service
            .verify("access-token")
            .await
            .expect_err("verify should fail");

        assert!(matches!(err, AppError::Unauthorized(_)));
    }

    #[tokio::test]
    async fn verify_returns_bad_request_when_active_provider_missing() {
        let providers = providers_without_active_provider();
        let service = AuthService::new(&providers);

        let err = service
            .verify("access-token")
            .await
            .expect_err("verify should fail");

        assert!(matches!(err, AppError::BadRequest(_)));
    }

    #[tokio::test]
    async fn seed_admin_returns_ok_when_provider_succeeds() {
        let providers = providers_with(ProviderMode::Success);
        let service = AuthService::new(&providers);

        let result = service.seed_admin(&test_config()).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn seed_admin_propagates_provider_anyhow_error() {
        let providers = providers_with(ProviderMode::SeedAdminError);
        let service = AuthService::new(&providers);

        let err = service
            .seed_admin(&test_config())
            .await
            .expect_err("seed_admin should fail");

        assert_eq!(err.to_string(), "seed admin failed");
    }

    #[tokio::test]
    async fn seed_admin_maps_missing_active_provider_to_anyhow() {
        let providers = providers_without_active_provider();
        let service = AuthService::new(&providers);

        let err = service
            .seed_admin(&test_config())
            .await
            .expect_err("seed_admin should fail");

        assert_eq!(err.to_string(), "Auth provider not configured: local");
    }
}
