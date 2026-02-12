use std::sync::Arc;

use crate::{config::AuthConfig, error::AppError, services::ServiceContext};

use super::{
    jwt::JwtKeys,
    providers::{AuthProviders, LocalAuthProvider},
};

pub fn build_providers(
    cfg: &AuthConfig,
    services: &ServiceContext,
) -> Result<AuthProviders, AppError> {
    let jwt = JwtKeys::from_secret(cfg.jwt_secret.as_bytes());
    let local_provider = LocalAuthProvider::new(services.user(), services.refresh_token_dao(), jwt);
    let mut providers = AuthProviders::new(cfg.provider).with_provider(Arc::new(local_provider))?;
    providers.set_active(cfg.provider)?;
    Ok(providers)
}

pub async fn init_providers(
    auth_cfg: &AuthConfig,
    services: &ServiceContext,
) -> anyhow::Result<AuthProviders> {
    let providers = build_providers(auth_cfg, services)?;
    services.auth(&providers).seed_admin(auth_cfg).await?;
    Ok(providers)
}
