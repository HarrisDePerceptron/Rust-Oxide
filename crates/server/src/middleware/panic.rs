use std::any::Any;

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};
use tower_http::catch_panic::CatchPanicLayer;

use crate::response::JsonApiResponse;

pub fn catch_panic_layer() -> CatchPanicLayer<fn(Box<dyn Any + Send + 'static>) -> Response> {
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
