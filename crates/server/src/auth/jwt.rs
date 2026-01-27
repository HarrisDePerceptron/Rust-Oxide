use std::time::{SystemTime, UNIX_EPOCH};

use jsonwebtoken::{Algorithm, Header, encode};

use super::{Claims, Role};
use crate::{error::AppError, state::JwtKeys};

pub fn now_unix() -> usize {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as usize
}

pub fn encode_token(keys: &JwtKeys, claims: &Claims) -> Result<String, AppError> {
    let mut header = Header::new(Algorithm::HS256);
    header.typ = Some("JWT".into());

    encode(&header, claims, &keys.enc).map_err(|err| {
        AppError::new(
            axum::http::StatusCode::BAD_REQUEST,
            format!("Token encoding failed: {err}"),
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
