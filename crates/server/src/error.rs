use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};

use crate::response::JsonApiResponse;

#[derive(Debug)]
pub struct AppError {
    pub status: StatusCode,
    pub message: &'static str,
}

impl AppError {
    pub fn new(status: StatusCode, message: &'static str) -> Self {
        Self { status, message }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        JsonApiResponse::error(&self).into_response()
    }
}
