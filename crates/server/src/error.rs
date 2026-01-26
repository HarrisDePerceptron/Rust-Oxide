use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};

use crate::response::JsonApiResponse;

#[derive(Debug)]
pub struct AppError {
    pub status: StatusCode,
    pub message: String,
}

impl AppError {
    pub fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        JsonApiResponse::from_error(&self).into_response()
    }
}
