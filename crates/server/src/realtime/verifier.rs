use async_trait::async_trait;

use crate::{auth::providers::AuthProviders, error::AppError};

#[derive(Clone)]
pub struct AppRealtimeVerifier {
    auth_providers: AuthProviders,
}

impl AppRealtimeVerifier {
    pub fn new(auth_providers: AuthProviders) -> Self {
        Self { auth_providers }
    }
}

#[async_trait]
impl realtime::server::RealtimeTokenVerifier for AppRealtimeVerifier {
    async fn verify_token(
        &self,
        token: &str,
    ) -> Result<realtime::server::SessionAuth, realtime::server::RealtimeError> {
        let claims = self
            .auth_providers
            .active()
            .map_err(map_app_error)?
            .verify(token)
            .await
            .map_err(map_app_error)?;

        Ok(realtime::server::SessionAuth {
            user_id: claims.sub,
            roles: claims
                .roles
                .into_iter()
                .map(|role| role.as_str().to_string())
                .collect(),
        })
    }
}

fn map_app_error(err: AppError) -> realtime::server::RealtimeError {
    match err {
        AppError::BadRequest(message) | AppError::Conflict(message) => {
            realtime::server::RealtimeError::bad_request(message)
        }
        AppError::Unauthorized(message) => realtime::server::RealtimeError::unauthorized(message),
        AppError::Forbidden(message) => realtime::server::RealtimeError::forbidden(message),
        AppError::NotFound(message) => realtime::server::RealtimeError::not_found(message),
        AppError::Internal(_) => realtime::server::RealtimeError::internal("internal server error"),
    }
}
