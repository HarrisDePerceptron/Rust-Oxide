use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
};
use rand::thread_rng;

use crate::error::AppError;

const MIN_PASSWORD_LEN: usize = 8;

pub fn hash_password(password: &str) -> Result<String, AppError> {
    if password.len() < MIN_PASSWORD_LEN {
        return Err(AppError::bad_request("Password too short"));
    }

    let salt = SaltString::generate(&mut thread_rng());
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|err| AppError::bad_request(format!("Password hashing failed: {err}")))?
        .to_string();
    Ok(hash)
}

pub fn verify_password(password: &str, hash: &str) -> Result<bool, AppError> {
    let parsed = PasswordHash::new(hash)
        .map_err(|err| AppError::bad_request(format!("Invalid password hash: {err}")))?;

    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok())
}

#[cfg(test)]
mod tests {
    use super::{hash_password, verify_password};

    #[test]
    fn hash_password_rejects_password_shorter_than_min_len() {
        let err = hash_password("short").expect_err("password should be rejected");
        assert_eq!(err.message(), "Password too short");
    }

    #[test]
    fn hash_password_accepts_password_at_min_len() {
        let hash = hash_password("12345678").expect("min-length password should be accepted");
        assert!(!hash.is_empty());
    }

    #[test]
    fn verify_password_returns_true_for_matching_password() {
        let password = "correct-horse-battery-staple";
        let hash = hash_password(password).expect("hash should succeed");
        let verified = verify_password(password, &hash).expect("verification should succeed");
        assert!(verified);
    }

    #[test]
    fn verify_password_returns_false_for_non_matching_password() {
        let hash = hash_password("correct-horse-battery-staple").expect("hash should succeed");
        let verified =
            verify_password("wrong-password", &hash).expect("verification should succeed");
        assert!(!verified);
    }

    #[test]
    fn verify_password_returns_error_for_invalid_hash() {
        let err = verify_password("password123", "not-a-valid-hash")
            .expect_err("invalid hash should fail");
        assert!(
            err.message().starts_with("Invalid password hash:"),
            "unexpected message: {}",
            err.message()
        );
    }
}
