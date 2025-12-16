use axum::{
    body::Body,
    http::{Request, StatusCode},
    response::{IntoResponse, Response},
};
use futures_util::future::BoxFuture;
use std::task::{Context, Poll};
use tower::{Layer, Service};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Admin,
}
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    pub sub: String, // user id / email
    pub exp: usize,  // expiry (unix)
    pub iat: usize,  // issued at
    pub roles: Vec<Role>,
}

#[derive(Clone)]
pub struct RequireRoleLayer {
    required: Role,
}

impl RequireRoleLayer {
    pub fn new(required: Role) -> Self {
        Self { required }
    }
}

#[derive(Clone)]
pub struct RequireRole<S> {
    inner: S,
    required: Role,
}

// Helper extractor: get Claims from extensions in handler signature.
impl<S> axum::extract::FromRequestParts<S> for Claims
where
    S: Send + Sync,
{
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<Claims>()
            .cloned()
            .ok_or((StatusCode::UNAUTHORIZED, "No claims in request"))
    }
}

impl<S> Layer<S> for RequireRoleLayer {
    type Service = RequireRole<S>;

    fn layer(&self, inner: S) -> Self::Service {
        RequireRole {
            inner,
            required: self.required.clone(),
        }
    }
}

impl<S> Service<Request<Body>> for RequireRole<S>
where
    S: Service<Request<Body>, Response = Response> + Clone + Send + 'static,
    S::Future: Send + 'static,
    S::Error: Send + 'static,
{
    type Response = Response;
    type Error = S::Error;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        let required = self.required.clone();

        // tower Services are allowed to be called concurrently, so clone inner
        let mut inner = self.inner.clone();

        Box::pin(async move {
            let claims = match req.extensions().get::<Claims>() {
                Some(c) => c,
                None => return Ok((StatusCode::UNAUTHORIZED, "No JWT claims").into_response()),
            };

            if !claims.roles.iter().any(|r| r == &required) {
                return Ok((StatusCode::FORBIDDEN, "Missing required role").into_response());
            }

            inner.call(req).await
        })
    }
}
