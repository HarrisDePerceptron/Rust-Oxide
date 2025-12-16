use std::sync::Arc;

use axum::{Json, Router, extract::State, http::StatusCode, routing::post};
use jsonwebtoken::{Algorithm, Header, encode};
use serde::Deserialize;

use crate::auth::jwt::now_unix;
use crate::{
    auth::{Claims, Role},
    error::AppError,
    state::AppState,
};

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    username: String,
    password: String,
}

#[derive(Debug, serde::Serialize)]
pub struct TokenResponse {
    access_token: String,
    token_type: &'static str,
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new().route("/login", post(login)).with_state(state)
}

async fn login(
    State(state): State<Arc<AppState>>,
    Json(body): Json<LoginRequest>,
) -> Result<Json<TokenResponse>, AppError> {
    // Demo auth: replace with DB lookup + password hashing
    if body.username != "admin" || body.password != "admin" {
        return Err(AppError::new(
            StatusCode::UNAUTHORIZED,
            "Invalid credentials",
        ));
    }

    let iat = now_unix();
    let exp = iat + 60 * 60; // 1 hour

    let roles = if body.username == "admin" {
        vec![Role::User, Role::Admin]
    } else {
        vec![Role::User]
    };

    let claims = Claims {
        sub: body.username,
        roles,
        iat,
        exp,
    };

    let mut header = Header::new(Algorithm::HS256);
    header.typ = Some("JWT".into());

    let token = encode(&header, &claims, &state.jwt.enc)
        .map_err(|_| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, "Token encoding failed"))?;

    Ok(Json(TokenResponse {
        access_token: token,
        token_type: "Bearer",
    }))
}
