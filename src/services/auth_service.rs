use axum::http::StatusCode;

use crate::{
    auth::{
        Role,
        jwt::{encode_token, make_access_claims},
        password::{hash_password, verify_password},
    },
    db::dao::{refresh_token_dao, user_dao},
    db::entities,
    error::AppError,
    state::AppState,
};

const ACCESS_TTL_SECS: usize = 15 * 60; // 15 minutes
const REFRESH_TTL_DAYS: i64 = 30;

#[derive(Debug)]
pub struct TokenBundle {
    pub access_token: String,
    pub refresh_token: String,
    pub token_type: &'static str,
    pub expires_in: usize,
}

pub async fn register(
    state: &AppState,
    email: &str,
    password: &str,
) -> Result<TokenBundle, AppError> {
    let email = email.trim();
    if email.is_empty() {
        return Err(AppError::new(StatusCode::BAD_REQUEST, "Email required"));
    }

    if user_dao::find_by_email(&state.db, email)
        .await
        .map_err(|_| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, "DB error"))?
        .is_some()
    {
        return Err(AppError::new(StatusCode::CONFLICT, "User already exists"));
    }

    let password_hash = hash_password(password)?;
    let user = user_dao::create_user(&state.db, email, &password_hash, Role::User.as_str())
        .await
        .map_err(|_| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, "Create user failed"))?;

    issue_tokens(state, &user).await
}

pub async fn login(
    state: &AppState,
    email: &str,
    password: &str,
) -> Result<TokenBundle, AppError> {
    let user = user_dao::find_by_email(&state.db, email)
        .await
        .map_err(|_| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, "DB error"))?
        .ok_or_else(|| AppError::new(StatusCode::UNAUTHORIZED, "Invalid credentials"))?;

    let password_ok = verify_password(password, &user.password_hash)?;
    if !password_ok {
        return Err(AppError::new(
            StatusCode::UNAUTHORIZED,
            "Invalid credentials",
        ));
    }

    let now = chrono::Utc::now().fixed_offset();
    user_dao::set_last_login(&state.db, &user.id, &now)
        .await
        .map_err(|_| {
            AppError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to update last login",
            )
        })?;

    issue_tokens(state, &user).await
}

pub async fn refresh(state: &AppState, refresh_token: &str) -> Result<TokenBundle, AppError> {
    let token = refresh_token_dao::find_active_by_token(&state.db, refresh_token)
        .await
        .map_err(|_| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, "DB error"))?
        .ok_or_else(|| AppError::new(StatusCode::UNAUTHORIZED, "Invalid refresh token"))?;

    if token.expires_at < chrono::Utc::now().fixed_offset() || token.revoked {
        return Err(AppError::new(
            StatusCode::UNAUTHORIZED,
            "Refresh token expired",
        ));
    }

    let user = user_dao::find_by_id(&state.db, &token.user_id)
        .await
        .map_err(|_| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, "DB error"))?
        .ok_or_else(|| AppError::new(StatusCode::UNAUTHORIZED, "User missing"))?;

    refresh_token_dao::revoke_token(&state.db, refresh_token)
        .await
        .map_err(|_| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, "Token revoke failed"))?;

    issue_tokens(state, &user).await
}

async fn issue_tokens(
    state: &AppState,
    user: &entities::user::Model,
) -> Result<TokenBundle, AppError> {
    let primary_role = Role::try_from(user.role.as_str()).unwrap_or(Role::User);
    let mut roles = vec![primary_role.clone()];
    if matches!(primary_role, Role::Admin) {
        roles.push(Role::User);
    }
    let claims = make_access_claims(&user.id, roles, ACCESS_TTL_SECS);
    let access_token = encode_token(state, &claims)?;

    let refresh =
        refresh_token_dao::create_refresh_token(&state.db, &user.id, Some(REFRESH_TTL_DAYS))
            .await
            .map_err(|_| {
                AppError::new(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Refresh token issue failed",
                )
            })?;

    Ok(TokenBundle {
        access_token,
        refresh_token: refresh.token,
        token_type: "Bearer",
        expires_in: ACCESS_TTL_SECS,
    })
}
