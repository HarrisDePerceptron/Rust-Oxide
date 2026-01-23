use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    extract::{Request, State},
    http::header,
    middleware::Next,
    response::{IntoResponse, Response},
};
use jsonwebtoken::{Algorithm, Header, Validation, decode, encode};

use super::{Claims, Role};
use crate::{error::AppError, state::{AppState, JwtKeys}};

pub fn now_unix() -> usize {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as usize
}

pub fn encode_token(keys: &JwtKeys, claims: &Claims) -> Result<String, AppError> {
    let mut header = Header::new(Algorithm::HS256);
    header.typ = Some("JWT".into());

    encode(&header, claims, &keys.enc).map_err(|_| {
        AppError::new(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "Token encoding failed",
        )
    })
}

pub fn make_access_claims(user_id: &uuid::Uuid, roles: Vec<Role>, ttl_secs: usize) -> Claims {
    let iat = now_unix();
    let exp = iat + ttl_secs;
    Claims {
        sub: user_id.to_string(),
        roles,
        iat,
        exp,
    }
}

pub async fn jwt_auth(
    State(state): State<Arc<AppState>>,
    mut req: Request,
    next: Next,
) -> Result<Response, Response> {
    let auth = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let token = auth.strip_prefix("Bearer ").ok_or_else(|| {
        AppError::new(
            axum::http::StatusCode::UNAUTHORIZED,
            "Missing/invalid Authorization header",
        )
        .into_response()
    })?;

    let mut validation = Validation::new(Algorithm::HS256);
    validation.validate_exp = true;

    let data = decode::<Claims>(token, &state.jwt.dec, &validation).map_err(|_| {
        AppError::new(
            axum::http::StatusCode::UNAUTHORIZED,
            "Invalid or expired token",
        )
        .into_response()
    })?;

    req.extensions_mut().insert(data.claims);

    Ok(next.run(req).await)
}
