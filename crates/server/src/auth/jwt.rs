use std::time::{SystemTime, UNIX_EPOCH};

use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, encode};

use super::{Claims, Role};
use crate::error::AppError;

#[derive(Clone)]
pub struct JwtKeys {
    pub enc: EncodingKey,
    pub dec: DecodingKey,
}

impl JwtKeys {
    pub fn from_secret(secret: &[u8]) -> Self {
        Self {
            enc: EncodingKey::from_secret(secret),
            dec: DecodingKey::from_secret(secret),
        }
    }
}

pub fn now_unix() -> usize {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as usize
}

pub fn encode_token(keys: &JwtKeys, claims: &Claims) -> Result<String, AppError> {
    let mut header = Header::new(Algorithm::HS256);
    header.typ = Some("JWT".into());

    encode(&header, claims, &keys.enc)
        .map_err(|err| AppError::bad_request(format!("Token encoding failed: {err}")))
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

impl From<jsonwebtoken::errors::Error> for AppError {
    fn from(err: jsonwebtoken::errors::Error) -> Self {
        AppError::bad_request(format!("Invalid or expired token: {err}"))
    }
}
