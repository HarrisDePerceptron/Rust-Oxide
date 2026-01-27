pub mod jwt;
pub mod password;

use std::{marker::PhantomData, sync::Arc};

use axum::{
    extract::FromRequestParts,
    http::{StatusCode, header},
};
use jsonwebtoken::{Algorithm, Validation, decode};
use serde::{Deserialize, Serialize};

use crate::{error::AppError, state::AppState};
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Admin,
}

impl Role {
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::User => "user",
            Role::Admin => "admin",
        }
    }
}

impl TryFrom<&str> for Role {
    type Error = ();

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "user" => Ok(Role::User),
            "admin" => Ok(Role::Admin),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    pub sub: String, // user id / email
    pub exp: usize,  // expiry (unix)
    pub iat: usize,  // issued at
    pub roles: Vec<Role>,
}

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

        let token = auth.strip_prefix("Bearer ").ok_or_else(|| {
            AppError::new(
                StatusCode::UNAUTHORIZED,
                "Missing/invalid Authorization header",
            )
        })?;

        let mut validation = Validation::new(Algorithm::HS256);
        validation.validate_exp = true;

        let data = decode::<Claims>(token, &state.jwt.dec, &validation).map_err(|err| {
            AppError::new(
                StatusCode::BAD_REQUEST,
                format!("Invalid or expired token: {err}"),
            )
        })?;

        parts.extensions.insert(data.claims.clone());
        Ok(data.claims)
    }
}

pub type AuthGuard = Claims;

pub trait RequiredRole {
    fn required() -> Role;
}

pub struct UserRole;

impl RequiredRole for UserRole {
    fn required() -> Role {
        Role::User
    }
}

pub struct AdminRole;

impl RequiredRole for AdminRole {
    fn required() -> Role {
        Role::Admin
    }
}

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
            return Err(AppError::new(
                StatusCode::FORBIDDEN,
                "Missing required role",
            ));
        }

        Ok(Self {
            claims,
            _marker: PhantomData,
        })
    }
}
