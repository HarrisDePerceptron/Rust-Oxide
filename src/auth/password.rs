use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
};
use rand::thread_rng;

use crate::error::AppError;

const MIN_PASSWORD_LEN: usize = 8;

pub fn hash_password(password: &str) -> Result<String, AppError> {
    if password.len() < MIN_PASSWORD_LEN {
        return Err(AppError::new(
            axum::http::StatusCode::BAD_REQUEST,
            "Password too short",
        ));
    }

    let salt = SaltString::generate(&mut thread_rng());
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|_| {
            AppError::new(
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Password hashing failed",
            )
        })?
        .to_string();
    Ok(hash)
}

pub fn verify_password(password: &str, hash: &str) -> Result<bool, AppError> {
    let parsed = PasswordHash::new(hash).map_err(|_| {
        AppError::new(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "Invalid password hash",
        )
    })?;

    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok())
}
