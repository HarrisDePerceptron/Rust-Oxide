use std::{marker::PhantomData, sync::Arc};

use axum::{extract::FromRequestParts, http::header};

use crate::{
    auth::{Claims, RequiredRole},
    error::AppError,
    state::AppState,
};

// Auth guard: validate JWT and return claims.
impl FromRequestParts<Arc<AppState>> for Claims {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        if let Some(claims) = parts.extensions.get::<Claims>().cloned() {
            return Ok(claims);
        }

        let auth = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("");

        let token = auth
            .strip_prefix("Bearer ")
            .ok_or_else(|| AppError::unauthorized("Missing/invalid Authorization header"))?;

        let claims = state.auth_providers.active()?.verify(token).await?;
        parts.extensions.insert(claims.clone());
        Ok(claims)
    }
}

pub type AuthGuard = Claims;

pub struct AuthRoleGuard<R: RequiredRole> {
    pub claims: Claims,
    _marker: PhantomData<R>,
}

impl<R> FromRequestParts<Arc<AppState>> for AuthRoleGuard<R>
where
    R: RequiredRole,
{
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let claims = Claims::from_request_parts(parts, state).await?;

        if !claims.roles.iter().any(|role| role == &R::required()) {
            return Err(AppError::forbidden("Missing required role"));
        }

        Ok(Self {
            claims,
            _marker: PhantomData,
        })
    }
}
