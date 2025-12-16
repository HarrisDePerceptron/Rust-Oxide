use axum::{
    body::Body,
    http::{Request, StatusCode},
    response::{IntoResponse, Response},
};
use futures_util::future::BoxFuture;
use std::task::{Context, Poll};
use tower::{Layer, Service};

use super::Claims;
use crate::auth::Role;

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
