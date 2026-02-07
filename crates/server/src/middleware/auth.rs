use std::{
    sync::Arc,
    task::{Context, Poll},
};

use axum::{
    body::Body,
    extract::{Request, State},
    http::{Request as HttpRequest, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use futures_util::future::BoxFuture;
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
        AppError::unauthorized("Missing/invalid Authorization header").into_response()
    })?;

    let claims = state
        .auth_providers
        .active()
        .map_err(IntoResponse::into_response)?
        .verify(token)
        .await
        .map_err(IntoResponse::into_response)?;

    req.extensions_mut().insert(claims);

    Ok(next.run(req).await)
}

#[derive(Clone)]
pub struct AuthRolGuardLayer {
    required: Role,
    state: Arc<AppState>,
}

impl AuthRolGuardLayer {
    pub fn new(state: Arc<AppState>, required: Role) -> Self {
        Self { required, state }
    }
}

#[derive(Clone)]
pub struct RequireRole<S> {
    inner: S,
    required: Role,
    state: Arc<AppState>,
}

impl<S> Layer<S> for AuthRolGuardLayer {
    type Service = RequireRole<S>;

    fn layer(&self, inner: S) -> Self::Service {
        RequireRole {
            inner,
            required: self.required.clone(),
            state: Arc::clone(&self.state),
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
        let state = Arc::clone(&self.state);

        Box::pin(async move {
            let mut req = req;
            let claims = if let Some(claims) = req.extensions().get::<Claims>() {
                claims.clone()
            } else {
                let auth = req
                    .headers()
                    .get(header::AUTHORIZATION)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("");

                let token = match auth.strip_prefix("Bearer ") {
                    Some(token) => token,
                    None => {
                        return Ok(
                            AppError::unauthorized("Missing/invalid Authorization header")
                                .into_response(),
                        );
                    }
                };

                match state.auth_providers.active() {
                    Ok(provider) => match provider.verify(token).await {
                        Ok(claims) => claims,
                        Err(err) => return Ok(err.into_response()),
                    },
                    Err(err) => return Ok(err.into_response()),
                }
            };

            req.extensions_mut().insert(claims.clone());

            if !claims.roles.iter().any(|r| r == &required) {
                return Ok(AppError::forbidden("Missing required role").into_response());
            }

            inner.call(req).await
        })
    }
}
