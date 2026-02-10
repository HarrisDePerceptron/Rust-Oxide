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

#[cfg(test)]
mod tests {
    use jsonwebtoken::{Algorithm, Validation, decode};
    use uuid::Uuid;

    use crate::auth::Claims;

    use super::{JwtKeys, Role, encode_token, make_access_claims};

    #[test]
    fn makes_claims_with_expected_subject_roles_and_ttl() {
        let user_id = Uuid::new_v4();
        let claims = make_access_claims(&user_id, vec![Role::User], 60);

        assert_eq!(claims.sub, user_id.to_string());
        assert_eq!(claims.roles, vec![Role::User]);
        assert_eq!(claims.exp.saturating_sub(claims.iat), 60);
    }

    #[test]
    fn encodes_token_that_can_be_decoded_with_same_secret() {
        let keys = JwtKeys::from_secret(b"unit-test-secret");
        let claims = make_access_claims(&Uuid::new_v4(), vec![Role::Admin, Role::User], 600);
        let token = encode_token(&keys, &claims).expect("token should encode");

        let mut validation = Validation::new(Algorithm::HS256);
        validation.validate_exp = false;
        let decoded =
            decode::<Claims>(&token, &keys.dec, &validation).expect("token should decode");

        assert_eq!(decoded.claims.sub, claims.sub);
        assert_eq!(decoded.claims.roles, claims.roles);
        assert_eq!(decoded.claims.iat, claims.iat);
        assert_eq!(decoded.claims.exp, claims.exp);
    }

    #[test]
    fn decode_error_maps_to_bad_request() {
        let err = crate::error::AppError::from(
            decode::<Claims>(
                "not-a-token",
                &JwtKeys::from_secret(b"unit-test-secret").dec,
                &Validation::new(Algorithm::HS256),
            )
            .expect_err("decode should fail"),
        );

        assert!(
            err.message().starts_with("Invalid or expired token:"),
            "unexpected message: {}",
            err.message()
        );
    }
}
