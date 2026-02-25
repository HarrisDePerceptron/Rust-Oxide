#[cfg(feature = "auth-local")]
use std::sync::Arc;

#[cfg(feature = "auth-local")]
use anyhow::Context;

use crate::{config::AuthConfig, error::AppError, services::ServiceContext};

use super::providers::AuthProviders;
#[cfg(feature = "auth-local")]
use super::{jwt::JwtKeys, providers::LocalAuthProvider};

#[cfg(feature = "auth-local")]
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

#[cfg(not(feature = "auth-local"))]
pub fn build_providers(
    cfg: &AuthConfig,
    services: &ServiceContext,
) -> Result<AuthProviders, AppError> {
    let _ = (cfg, services);
    Ok(AuthProviders::new(
        crate::auth::providers::AuthProviderId::Local,
    ))
}

#[cfg(feature = "auth-local")]
pub async fn init_providers(
    auth_cfg: Option<&AuthConfig>,
    services: &ServiceContext,
) -> anyhow::Result<AuthProviders> {
    let auth_cfg = auth_cfg.context(
        "auth config missing; set APP_AUTH__JWT_SECRET, APP_AUTH__ADMIN_EMAIL, APP_AUTH__ADMIN_PASSWORD",
    )?;
    let providers = build_providers(auth_cfg, services)?;
    services.auth(&providers).seed_admin(auth_cfg).await?;
    Ok(providers)
}

#[cfg(not(feature = "auth-local"))]
pub async fn init_providers(
    auth_cfg: Option<&AuthConfig>,
    services: &ServiceContext,
) -> anyhow::Result<AuthProviders> {
    let _ = (auth_cfg, services);
    Ok(AuthProviders::new(
        crate::auth::providers::AuthProviderId::Local,
    ))
}
