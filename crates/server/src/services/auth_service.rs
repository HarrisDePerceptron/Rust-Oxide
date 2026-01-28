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
