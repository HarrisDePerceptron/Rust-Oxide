use std::sync::Arc;

use axum::{Json, Router, extract::State, http::StatusCode, routing::post};
use serde::Deserialize;

use crate::{
    auth::{
        Role,
        jwt::{encode_token, make_access_claims},
        password::{hash_password, verify_password},
    },
    db::{entities, refresh_token_repo, user_repo},
    error::AppError,
    state::AppState,
};

const ACCESS_TTL_SECS: usize = 15 * 60; // 15 minutes
const REFRESH_TTL_DAYS: i64 = 30;

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct RefreshRequest {
    pub refresh_token: String,
}

#[derive(Debug, serde::Serialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub token_type: &'static str,
    pub expires_in: usize,
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/login", post(login))
        .route("/register", post(register))
        .route("/refresh", post(refresh))
        .with_state(state)
}

async fn register(
    State(state): State<Arc<AppState>>,
    Json(body): Json<RegisterRequest>,
) -> Result<Json<TokenResponse>, AppError> {
    if body.email.trim().is_empty() {
        return Err(AppError::new(StatusCode::BAD_REQUEST, "Email required"));
    }

    if user_repo::find_by_email(&state.db, &body.email)
        .await
        .map_err(|_| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, "DB error"))?
        .is_some()
    {
        return Err(AppError::new(StatusCode::CONFLICT, "User already exists"));
    }

    let password_hash = hash_password(&body.password)?;
    let user = user_repo::create_user(&state.db, &body.email, &password_hash, Role::User.as_str())
        .await
        .map_err(|_| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, "Create user failed"))?;

    issue_tokens(&state, &user).await
}

async fn login(
    State(state): State<Arc<AppState>>,
    Json(body): Json<LoginRequest>,
) -> Result<Json<TokenResponse>, AppError> {
    let user = user_repo::find_by_email(&state.db, &body.email)
        .await
        .map_err(|_| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, "DB error"))?
        .ok_or_else(|| AppError::new(StatusCode::UNAUTHORIZED, "Invalid credentials"))?;

    let password_ok = verify_password(&body.password, &user.password_hash)?;
    if !password_ok {
        return Err(AppError::new(
            StatusCode::UNAUTHORIZED,
            "Invalid credentials",
        ));
    }

    let now = chrono::Utc::now().fixed_offset();
    user_repo::set_last_login(&state.db, &user.id, &now)
        .await
        .map_err(|_| {
            AppError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to update last login",
            )
        })?;

    issue_tokens(&state, &user).await
}

async fn refresh(
    State(state): State<Arc<AppState>>,
    Json(body): Json<RefreshRequest>,
) -> Result<Json<TokenResponse>, AppError> {
    let token = refresh_token_repo::find_active_by_token(&state.db, &body.refresh_token)
        .await
        .map_err(|_| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, "DB error"))?
        .ok_or_else(|| AppError::new(StatusCode::UNAUTHORIZED, "Invalid refresh token"))?;

    if token.expires_at < chrono::Utc::now().fixed_offset() || token.revoked {
        return Err(AppError::new(
            StatusCode::UNAUTHORIZED,
            "Refresh token expired",
        ));
    }

    let user = user_repo::find_by_id(&state.db, &token.user_id)
        .await
        .map_err(|_| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, "DB error"))?
        .ok_or_else(|| AppError::new(StatusCode::UNAUTHORIZED, "User missing"))?;

    // rotate refresh token
    refresh_token_repo::revoke_token(&state.db, &body.refresh_token)
        .await
        .map_err(|_| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, "Token revoke failed"))?;

    issue_tokens(&state, &user).await
}

async fn issue_tokens(
    state: &AppState,
    user: &entities::user::Model,
) -> Result<Json<TokenResponse>, AppError> {
    let primary_role = Role::try_from(user.role.as_str()).unwrap_or(Role::User);
    let mut roles = vec![primary_role.clone()];
    if matches!(primary_role, Role::Admin) {
        roles.push(Role::User);
    }
    let claims = make_access_claims(&user.id, roles, ACCESS_TTL_SECS);
    let access_token = encode_token(state, &claims)?;

    let refresh =
        refresh_token_repo::create_refresh_token(&state.db, &user.id, Some(REFRESH_TTL_DAYS))
            .await
            .map_err(|_| {
                AppError::new(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Refresh token issue failed",
                )
            })?;

    Ok(Json(TokenResponse {
        access_token,
        refresh_token: refresh.token,
        token_type: "Bearer",
        expires_in: ACCESS_TTL_SECS,
    }))
}
