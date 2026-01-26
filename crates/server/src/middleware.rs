use axum::{
    body::{Bytes, to_bytes},
    extract::Request,
    http::{HeaderMap, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::{error::AppError, response::JsonApiResponse};

const MAX_ERROR_BODY_BYTES: usize = 16 * 1024;

pub async fn json_error_middleware(req: Request, next: Next) -> Response {
    let wants_html = accepts_html(&req);
    let response = next.run(req).await;

    if !response.status().is_client_error() && !response.status().is_server_error() {
        return response;
    }

    if is_json_response(&response) || is_html_response(&response) || wants_html {
        return response;
    }

    let status = response.status();
    let (parts, body) = response.into_parts();
    let message = match to_bytes(body, MAX_ERROR_BODY_BYTES).await {
        Ok(bytes) => body_bytes_to_message(status, bytes),
        Err(_) => default_message(status),
    };
    let app_error = AppError::new(status, message);

    let mut new_response = JsonApiResponse::from_error(&app_error).into_response();
    copy_headers(&parts.headers, &mut new_response);
    new_response
}

fn accepts_html(req: &Request) -> bool {
    req.headers()
        .get(header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.to_ascii_lowercase().contains("text/html"))
        .unwrap_or(false)
}

fn is_json_response(response: &Response) -> bool {
    response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(|value| {
            let value = value.to_ascii_lowercase();
            value.contains("application/json") || value.contains("+json")
        })
        .unwrap_or(false)
}

fn is_html_response(response: &Response) -> bool {
    response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.to_ascii_lowercase().contains("text/html"))
        .unwrap_or(false)
}

fn body_bytes_to_message(status: StatusCode, bytes: Bytes) -> String {
    let message = String::from_utf8_lossy(&bytes).trim().to_string();
    if message.is_empty() {
        return default_message(status);
    }
    message
}

fn default_message(status: StatusCode) -> String {
    status
        .canonical_reason()
        .unwrap_or("Request failed")
        .to_string()
}

fn copy_headers(src: &HeaderMap, dest: &mut Response) {
    for (name, value) in src {
        if name == header::CONTENT_TYPE || name == header::CONTENT_LENGTH {
            continue;
        }
        dest.headers_mut().insert(name.clone(), value.clone());
    }
}
