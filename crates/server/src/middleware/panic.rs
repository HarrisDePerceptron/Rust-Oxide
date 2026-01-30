use std::any::Any;

use axum::{http::StatusCode, response::{IntoResponse, Response}};
use tower_http::catch_panic::CatchPanicLayer;

use crate::{
    error::AppError,
    response::{JsonApiResponse, log_app_error},
};

pub fn catch_panic_layer(
) -> CatchPanicLayer<fn(Box<dyn Any + Send + 'static>) -> Response> {
    CatchPanicLayer::custom(panic_to_json)
}

fn panic_to_json(panic: Box<dyn Any + Send + 'static>) -> Response {
    let details = if let Some(message) = panic.downcast_ref::<String>() {
        message.as_str()
    } else if let Some(message) = panic.downcast_ref::<&str>() {
        message
    } else {
        "unknown panic"
    };

    let app_error = AppError::internal_with_source(
        "internal server error",
        PanicSource::new(details),
    );
    log_app_error(&app_error, StatusCode::INTERNAL_SERVER_ERROR);

    let client_message = if cfg!(debug_assertions) {
        format!("internal server error: {}", details)
    } else {
        "internal server error".to_string()
    };

    JsonApiResponse {
        status: StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
        message: client_message,
        data: serde_json::Value::Null,
    }
    .into_response()
}

#[derive(Debug)]
struct PanicSource {
    message: String,
}

impl PanicSource {
    fn new(message: &str) -> Self {
        Self {
            message: message.to_string(),
        }
    }
}

impl std::fmt::Display for PanicSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for PanicSource {}
