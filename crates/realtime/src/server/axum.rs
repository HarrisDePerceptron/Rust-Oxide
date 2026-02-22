use std::sync::Arc;

use axum::{
    Router,
    extract::{
        Query, State,
        ws::{WebSocketUpgrade, rejection::WebSocketUpgradeRejection},
    },
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use serde::Deserialize;

use super::{RealtimeError, SocketAppState};

#[derive(Debug, Clone)]
pub struct RealtimeRouteOptions {
    pub path: &'static str,
    pub allow_query_token: bool,
    pub strict_header_precedence: bool,
}

impl Default for RealtimeRouteOptions {
    fn default() -> Self {
        Self {
            path: "/realtime/socket",
            allow_query_token: true,
            strict_header_precedence: true,
        }
    }
}

struct SocketRouteState {
    socket_server_handle: Arc<SocketAppState>,
    options: RealtimeRouteOptions,
}

impl Clone for SocketRouteState {
    fn clone(&self) -> Self {
        Self {
            socket_server_handle: Arc::clone(&self.socket_server_handle),
            options: self.options.clone(),
        }
    }
}

#[derive(Debug, Deserialize, Default)]
struct SocketQuery {
    token: Option<String>,
}

#[derive(Debug)]
enum RealtimeHttpError {
    MissingToken,
    InvalidToken,
    UpgradeRequired,
    RealtimeDisabled,
    VerifyFailed(RealtimeError),
}

impl RealtimeHttpError {
    fn status(&self) -> StatusCode {
        match self {
            Self::MissingToken | Self::InvalidToken => StatusCode::UNAUTHORIZED,
            Self::UpgradeRequired => StatusCode::BAD_REQUEST,
            Self::RealtimeDisabled => StatusCode::NOT_FOUND,
            Self::VerifyFailed(err) => match err {
                RealtimeError::BadRequest(_) => StatusCode::BAD_REQUEST,
                RealtimeError::Unauthorized(_) => StatusCode::UNAUTHORIZED,
                RealtimeError::Forbidden(_) => StatusCode::FORBIDDEN,
                RealtimeError::NotFound(_) => StatusCode::NOT_FOUND,
                RealtimeError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
            },
        }
    }

    fn message(&self) -> String {
        match self {
            Self::MissingToken => {
                "Missing access token (use Authorization Bearer or token query param)".to_string()
            }
            Self::InvalidToken => "Missing/invalid Authorization header".to_string(),
            Self::UpgradeRequired => "WebSocket upgrade required".to_string(),
            Self::RealtimeDisabled => "Realtime is disabled".to_string(),
            Self::VerifyFailed(err) => err.message().to_string(),
        }
    }
}

impl IntoResponse for RealtimeHttpError {
    fn into_response(self) -> Response {
        (self.status(), self.message()).into_response()
    }
}

pub fn router(socket_server_handle: Arc<SocketAppState>) -> Router {
    router_with_options(socket_server_handle, RealtimeRouteOptions::default())
}

pub fn router_with_options(
    socket_server_handle: Arc<SocketAppState>,
    options: RealtimeRouteOptions,
) -> Router {
    let path = options.path;
    Router::new()
        .route(path, get(socket_handler))
        .with_state(SocketRouteState {
            socket_server_handle,
            options,
        })
}

async fn socket_handler(
    State(handler_state): State<SocketRouteState>,
    upgrade: Result<WebSocketUpgrade, WebSocketUpgradeRejection>,
    headers: HeaderMap,
    Query(query): Query<SocketQuery>,
) -> Response {
    let realtime = handler_state.socket_server_handle.handle.clone();

    if !realtime.is_enabled() {
        return RealtimeHttpError::RealtimeDisabled.into_response();
    }

    let upgrade = match upgrade {
        Ok(upgrade) => upgrade,
        Err(_) => return RealtimeHttpError::UpgradeRequired.into_response(),
    };

    let token = match extract_access_token(&headers, &query, &handler_state.options) {
        Ok(token) => token,
        Err(err) => return err.into_response(),
    };

    let auth = match handler_state
        .socket_server_handle
        .verifier
        .verify_token(&token)
        .await
    {
        Ok(auth) => auth,
        Err(err) => return RealtimeHttpError::VerifyFailed(err).into_response(),
    };

    upgrade
        .max_message_size(realtime.max_message_bytes())
        .max_frame_size(realtime.max_message_bytes())
        .on_upgrade(move |socket| async move {
            realtime.serve_socket(socket, auth).await;
        })
        .into_response()
}

fn extract_access_token(
    headers: &HeaderMap,
    query: &SocketQuery,
    options: &RealtimeRouteOptions,
) -> Result<String, RealtimeHttpError> {
    let auth_header = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok());

    if let Some(auth_header) = auth_header {
        let header_token = auth_header
            .strip_prefix("Bearer ")
            .map(str::trim)
            .filter(|value| !value.is_empty());

        if let Some(token) = header_token {
            return Ok(token.to_string());
        }

        if options.strict_header_precedence {
            return Err(RealtimeHttpError::InvalidToken);
        }
    }

    if options.allow_query_token
        && let Some(token) = query
            .token
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
    {
        return Ok(token.to_string());
    }

    Err(RealtimeHttpError::MissingToken)
}

#[cfg(test)]
mod tests {
    use axum::http::header;

    use super::*;

    #[test]
    fn extract_access_token_prefers_authorization_header() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            "Bearer header-token".parse().expect("valid header"),
        );
        let query = SocketQuery {
            token: Some("query-token".to_string()),
        };

        let token = extract_access_token(&headers, &query, &RealtimeRouteOptions::default())
            .expect("token should parse");
        assert_eq!(token, "header-token");
    }

    #[test]
    fn extract_access_token_falls_back_to_query_token() {
        let headers = HeaderMap::new();
        let query = SocketQuery {
            token: Some("query-token".to_string()),
        };

        let token = extract_access_token(&headers, &query, &RealtimeRouteOptions::default())
            .expect("token should parse");
        assert_eq!(token, "query-token");
    }

    #[test]
    fn extract_access_token_rejects_invalid_header_when_strict() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            "Token abc".parse().expect("valid header"),
        );
        let query = SocketQuery {
            token: Some("query-token".to_string()),
        };

        let err = extract_access_token(&headers, &query, &RealtimeRouteOptions::default())
            .expect_err("invalid header should fail");
        assert!(matches!(err, RealtimeHttpError::InvalidToken));
    }
}
