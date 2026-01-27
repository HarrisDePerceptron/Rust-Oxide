use std::{
    sync::Arc,
    task::{Context, Poll},
};

use axum::{
    body::Body,
    extract::{Request, State},
    http::{Request as HttpRequest, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use futures_util::future::BoxFuture;
use jsonwebtoken::{Algorithm, Validation, decode};
use tower::{Layer, Service};

use crate::{
    auth::{Claims, Role},
    error::AppError,
    state::AppState,
};

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

    let data = decode::<Claims>(token, &state.jwt.dec, &validation).map_err(|err| {
        AppError::new(
            axum::http::StatusCode::BAD_REQUEST,
            format!("Invalid or expired token: {err}"),
        )
        .into_response()
    })?;

    req.extensions_mut().insert(data.claims);

    Ok(next.run(req).await)
}

#[derive(Clone)]
pub struct AuthRolGuardLayer {
    required: Role,
}

impl AuthRolGuardLayer {
    pub fn new(required: Role) -> Self {
        Self { required }
    }
}

#[derive(Clone)]
pub struct RequireRole<S> {
    inner: S,
    required: Role,
}

impl<S> Layer<S> for AuthRolGuardLayer {
    type Service = RequireRole<S>;

    fn layer(&self, inner: S) -> Self::Service {
        RequireRole {
            inner,
            required: self.required.clone(),
        }
    }
}

impl<S> Service<HttpRequest<Body>> for RequireRole<S>
where
    S: Service<HttpRequest<Body>, Response = Response> + Clone + Send + 'static,
    S::Future: Send + 'static,
    S::Error: Send + 'static,
{
    type Response = Response;
    type Error = S::Error;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: HttpRequest<Body>) -> Self::Future {
        let required = self.required.clone();
        let mut inner = self.inner.clone();

        Box::pin(async move {
            let claims = match req.extensions().get::<Claims>() {
                Some(c) => c,
                None => {
                    return Ok(AppError::new(StatusCode::UNAUTHORIZED, "No JWT claims")
                        .into_response());
                }
            };

            if !claims.roles.iter().any(|r| r == &required) {
                return Ok(AppError::new(StatusCode::FORBIDDEN, "Missing required role")
                    .into_response());
            }

            inner.call(req).await
        })
    }
}
