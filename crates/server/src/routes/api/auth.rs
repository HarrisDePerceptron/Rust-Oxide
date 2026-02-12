use std::sync::Arc;

use axum::{Json, Router, extract::State, routing::post};
use serde::Deserialize;

use crate::{
    auth::TokenBundle,
    routes::{ApiResult, JsonApiResponse},
    services::ServiceContext,
    state::AppState,
};

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
) -> ApiResult<TokenResponse> {
    let services = ServiceContext::from_state(state.as_ref());
    let service = services.auth(&state.auth_providers);
    let tokens = service.register(&body.email, &body.password).await?;
    JsonApiResponse::ok(tokens.into())
}

async fn login(
    State(state): State<Arc<AppState>>,
    Json(body): Json<LoginRequest>,
) -> ApiResult<TokenResponse> {
    let services = ServiceContext::from_state(state.as_ref());
    let service = services.auth(&state.auth_providers);
    let tokens = service.login(&body.email, &body.password).await?;
    JsonApiResponse::ok(tokens.into())
}

async fn refresh(
    State(state): State<Arc<AppState>>,
    Json(body): Json<RefreshRequest>,
) -> ApiResult<TokenResponse> {
    let services = ServiceContext::from_state(state.as_ref());
    let service = services.auth(&state.auth_providers);
    let tokens = service.refresh(&body.refresh_token).await?;
    JsonApiResponse::ok(tokens.into())
}

impl From<TokenBundle> for TokenResponse {
    fn from(bundle: TokenBundle) -> Self {
        Self {
            access_token: bundle.access_token,
            refresh_token: bundle.refresh_token,
            token_type: bundle.token_type,
            expires_in: bundle.expires_in,
        }
    }
}
