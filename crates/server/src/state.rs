use std::sync::Arc;

use async_trait::async_trait;
use sea_orm::DatabaseConnection;

use crate::{
    auth::providers::AuthProviders,
    config::AppConfig,
    error::AppError,
    realtime::{RealtimeHandle, SessionAuth},
};

#[derive(Clone)]
pub struct AppState {
    pub config: AppConfig,
    pub db: DatabaseConnection,
    pub auth_providers: AuthProviders,
    pub realtime: RealtimeHandle,
}

impl AppState {
    pub fn new(
        config: AppConfig,
        db: DatabaseConnection,
        auth_providers: AuthProviders,
        realtime: RealtimeHandle,
    ) -> Arc<Self> {
        Arc::new(Self {
            db,
            config,
            auth_providers,
            realtime,
        })
    }
}

#[async_trait]
impl realtime::server::axum::RealtimeAxumState for AppState {
    fn realtime_handle(&self) -> &RealtimeHandle {
        &self.realtime
    }

    async fn verify_realtime_token(
        &self,
        token: &str,
    ) -> Result<SessionAuth, realtime::server::RealtimeError> {
        let claims = self
            .auth_providers
            .active()
            .map_err(map_app_error)?
            .verify(token)
            .await
            .map_err(map_app_error)?;

        Ok(SessionAuth {
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
