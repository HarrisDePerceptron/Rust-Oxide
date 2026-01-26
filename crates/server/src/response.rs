use axum::{http::StatusCode, response::{IntoResponse, Response}, Json};
use serde::Serialize;

use crate::error::AppError;

#[derive(Debug, Serialize)]
pub struct JsonApiResponse<T: Serialize> {
    pub status: u16,
    pub message: String,
    pub data: T,
}

impl<T: Serialize> JsonApiResponse<T> {
    pub fn ok(data: T) -> Self {
        Self {
            status: StatusCode::OK.as_u16(),
            message: "ok".to_string(),
            data,
        }
    }

    pub fn with_status(status: StatusCode, message: impl Into<String>, data: T) -> Self {
        Self {
            status: status.as_u16(),
            message: message.into(),
            data,
        }
    }

}

impl JsonApiResponse<serde_json::Value> {
    pub fn error(err: &AppError) -> Self {
        Self {
            status: err.status.as_u16(),
            message: err.message.to_string(),
            data: serde_json::Value::Null,
        }
    }
}

impl<T: Serialize> IntoResponse for JsonApiResponse<T> {
    fn into_response(self) -> Response {
        let status = StatusCode::from_u16(self.status)
            .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        (status, Json(self)).into_response()
    }
}
