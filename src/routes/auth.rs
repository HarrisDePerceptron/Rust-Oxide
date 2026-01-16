use std::sync::Arc;

use axum::{Json, Router, extract::State, routing::post};
use serde::Deserialize;

use crate::{error::AppError, services::auth_service, state::AppState};

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
    let tokens = auth_service::register(state.as_ref(), &body.email, &body.password).await?;
    Ok(Json(tokens.into()))
}

async fn login(
    State(state): State<Arc<AppState>>,
    Json(body): Json<LoginRequest>,
) -> Result<Json<TokenResponse>, AppError> {
    let tokens = auth_service::login(state.as_ref(), &body.email, &body.password).await?;
    Ok(Json(tokens.into()))
}

async fn refresh(
    State(state): State<Arc<AppState>>,
    Json(body): Json<RefreshRequest>,
) -> Result<Json<TokenResponse>, AppError> {
    let tokens = auth_service::refresh(state.as_ref(), &body.refresh_token).await?;
    Ok(Json(tokens.into()))
}

impl From<auth_service::TokenBundle> for TokenResponse {
    fn from(bundle: auth_service::TokenBundle) -> Self {
        Self {
            access_token: bundle.access_token,
            refresh_token: bundle.refresh_token,
            token_type: bundle.token_type,
            expires_in: bundle.expires_in,
        }
    }
}
