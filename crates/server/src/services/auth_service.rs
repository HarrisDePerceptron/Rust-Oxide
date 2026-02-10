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

    use crate::auth::{Role, providers::AuthProviderId};

    use super::*;

    #[derive(Clone)]
    struct DelegatingProvider;

    #[async_trait]
    impl crate::auth::providers::AuthProvider for DelegatingProvider {
        fn id(&self) -> AuthProviderId {
            AuthProviderId::Local
        }

        async fn register(&self, email: &str, _password: &str) -> Result<TokenBundle, AppError> {
            Ok(TokenBundle {
                access_token: format!("register:{email}"),
                refresh_token: "refresh-register".to_string(),
                token_type: "Bearer",
                expires_in: 900,
            })
        }

        async fn login(&self, email: &str, _password: &str) -> Result<TokenBundle, AppError> {
            Ok(TokenBundle {
                access_token: format!("login:{email}"),
                refresh_token: "refresh-login".to_string(),
                token_type: "Bearer",
                expires_in: 900,
            })
        }

        async fn refresh(&self, refresh_token: &str) -> Result<TokenBundle, AppError> {
            Ok(TokenBundle {
                access_token: format!("refresh:{refresh_token}"),
                refresh_token: refresh_token.to_string(),
                token_type: "Bearer",
                expires_in: 900,
            })
        }

        async fn verify(&self, access_token: &str) -> Result<Claims, AppError> {
            Ok(Claims {
                sub: access_token.to_string(),
                exp: 100,
                iat: 10,
                roles: vec![Role::User],
            })
        }
    }

    #[tokio::test]
    async fn delegates_register_login_refresh_and_verify() {
        let providers = crate::auth::providers::AuthProviders::new(AuthProviderId::Local)
            .with_provider(Arc::new(DelegatingProvider))
            .expect("provider registration should succeed");
        let service = AuthService::new(&providers);

        let register = service
            .register("alice@example.com", "password123")
            .await
            .expect("register should succeed");
        assert_eq!(register.access_token, "register:alice@example.com");

        let login = service
            .login("alice@example.com", "password123")
            .await
            .expect("login should succeed");
        assert_eq!(login.access_token, "login:alice@example.com");

        let refreshed = service
            .refresh("refresh-token-1")
            .await
            .expect("refresh should succeed");
        assert_eq!(refreshed.access_token, "refresh:refresh-token-1");

        let claims = service
            .verify("subject-1")
            .await
            .expect("verify should succeed");
        assert_eq!(claims.sub, "subject-1");
        assert_eq!(claims.roles, vec![Role::User]);
    }
}
