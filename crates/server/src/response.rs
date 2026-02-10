use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;

use crate::error::AppError;

pub type ApiResult<T> = Result<JsonApiResponse<T>, AppError>;

#[derive(Debug, Serialize)]
pub struct JsonApiResponse<T: Serialize> {
    pub status: u16,
    pub message: String,
    pub data: T,
}

impl<T: Serialize> JsonApiResponse<T> {
    pub fn ok(data: T) -> ApiResult<T> {
        Ok(Self {
            status: StatusCode::OK.as_u16(),
            message: "ok".to_string(),
            data,
        })
    }

    pub fn with_status(status: StatusCode, message: impl Into<String>, data: T) -> ApiResult<T> {
        Ok(Self {
            status: status.as_u16(),
            message: message.into(),
            data,
        })
    }
}

impl JsonApiResponse<serde_json::Value> {
    pub fn error(err: AppError) -> ApiResult<serde_json::Value> {
        Err(err)
    }

    pub(crate) fn from_error(err: &AppError) -> Self {
        let status = status_for(err);
        Self {
            status: status.as_u16(),
            message: err.message().to_string(),
            data: serde_json::Value::Null,
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = status_for(&self);
        log_app_error(&self, status);
        JsonApiResponse::from_error(&self).into_response()
    }
}

impl<T: Serialize> IntoResponse for JsonApiResponse<T> {
    fn into_response(self) -> Response {
        let status = StatusCode::from_u16(self.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        (status, Json(self)).into_response()
    }
}

fn status_for(err: &AppError) -> StatusCode {
    match err {
        AppError::BadRequest(_) => StatusCode::BAD_REQUEST,
        AppError::Unauthorized(_) => StatusCode::UNAUTHORIZED,
        AppError::Forbidden(_) => StatusCode::FORBIDDEN,
        AppError::NotFound(_) => StatusCode::NOT_FOUND,
        AppError::Conflict(_) => StatusCode::CONFLICT,
        AppError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

pub(crate) fn log_app_error(err: &AppError, status: StatusCode) {
    let kind = error_kind(err);
    let message = err.message();

    if status.is_server_error() {
        if let Some(source) = err.source() {
            tracing::error!(
                status = status.as_u16(),
                error_kind = %kind,
                message = %message,
                error = ?source
            );
        } else {
            tracing::error!(
                status = status.as_u16(),
                error_kind = %kind,
                message = %message
            );
        }
    } else if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
        tracing::debug!(status = status.as_u16(), error_kind = %kind, message = %message);
    } else {
        tracing::warn!(status = status.as_u16(), error_kind = %kind, message = %message);
    }
}

fn error_kind(err: &AppError) -> &'static str {
    match err {
        AppError::BadRequest(_) => "bad_request",
        AppError::Unauthorized(_) => "unauthorized",
        AppError::Forbidden(_) => "forbidden",
        AppError::NotFound(_) => "not_found",
        AppError::Conflict(_) => "conflict",
        AppError::Internal(_) => "internal",
    }
}

#[cfg(test)]
mod tests {
    use axum::{
        body::to_bytes,
        http::StatusCode,
        response::IntoResponse,
    };
    use serde_json::Value;

    use super::JsonApiResponse;
    use crate::error::AppError;

    #[tokio::test]
    async fn app_error_into_response_uses_expected_status_and_message() {
        let response = AppError::forbidden("missing role").into_response();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body should read");
        let json: Value = serde_json::from_slice(&body).expect("body should be json");
        assert_eq!(json["status"], StatusCode::FORBIDDEN.as_u16());
        assert_eq!(json["message"], "missing role");
        assert!(json["data"].is_null());
    }

    #[tokio::test]
    async fn json_api_response_into_response_respects_status_code() {
        let response = JsonApiResponse::with_status(
            StatusCode::CREATED,
            "created",
            serde_json::json!({ "id": 1 }),
        )
        .expect("response should build")
        .into_response();
        assert_eq!(response.status(), StatusCode::CREATED);

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body should read");
        let json: Value = serde_json::from_slice(&body).expect("body should be json");
        assert_eq!(json["status"], StatusCode::CREATED.as_u16());
        assert_eq!(json["message"], "created");
        assert_eq!(json["data"]["id"], 1);
    }
}
