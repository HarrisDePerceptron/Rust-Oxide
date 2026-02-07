use std::sync::Arc;

use crate::{config::AppConfig, error::AppError, services::ServiceContext};

use super::{
    jwt::JwtKeys,
    providers::{AuthProviders, LocalAuthProvider},
};

pub fn build_providers(
    cfg: &AppConfig,
    services: &ServiceContext,
) -> Result<AuthProviders, AppError> {
    let jwt = JwtKeys::from_secret(cfg.jwt_secret.as_bytes());
    let local_provider = LocalAuthProvider::new(services.user(), services.refresh_token_dao(), jwt);
    let mut providers =
        AuthProviders::new(cfg.auth_provider).with_provider(Arc::new(local_provider))?;
    providers.set_active(cfg.auth_provider)?;
    Ok(providers)
}

pub async fn init_providers(
    cfg: &AppConfig,
    services: &ServiceContext,
) -> anyhow::Result<AuthProviders> {
    let providers = build_providers(cfg, services)?;
    services.auth(&providers).seed_admin(cfg).await?;
    Ok(providers)
}
