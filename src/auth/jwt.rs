use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    extract::{Request, State},
    http::header,
    middleware::Next,
    response::{IntoResponse, Response},
};
use jsonwebtoken::{Algorithm, Validation, decode};

use super::Claims;
use crate::{error::AppError, state::AppState};

pub fn now_unix() -> usize {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as usize
}

pub async fn jwt_auth(
    State(state): State<Arc<AppState>>,
    mut req: Request,
    next: Next,
) -> Result<Response, Response> {
    let auth = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let token = auth.strip_prefix("Bearer ").ok_or_else(|| {
        AppError::new(
            axum::http::StatusCode::UNAUTHORIZED,
            "Missing/invalid Authorization header",
        )
        .into_response()
    })?;

    let mut validation = Validation::new(Algorithm::HS256);
    validation.validate_exp = true;

    let data = decode::<Claims>(token, &state.jwt.dec, &validation).map_err(|_| {
        AppError::new(
            axum::http::StatusCode::UNAUTHORIZED,
            "Invalid or expired token",
        )
        .into_response()
    })?;

    req.extensions_mut().insert(data.claims);

    Ok(next.run(req).await)
}
