use axum::{
    Router,
    body::{self, Body},
    http::{Request, StatusCode},
    middleware,
};
use sea_orm::{DatabaseBackend, MockDatabase};
use serde_json::json;
use tower::ServiceExt;
use uuid::Uuid;

use rust_oxide::{
    auth::{
        Role,
        bootstrap::build_providers,
        jwt::{JwtKeys, encode_token, make_access_claims},
        providers::AuthProviderId,
    },
    config::{AppConfig, AuthConfig},
    realtime::{AppRealtimeVerifier, RealtimeHandle, RealtimeRuntimeState},
    routes::{
        API_PREFIX,
        middleware::{catch_panic_layer, json_error_middleware},
        router,
    },
    services::ServiceContext,
    state::AppState,
};

fn api_path(path: &str) -> String {
    format!("{API_PREFIX}{path}")
}

fn build_state(
    secret: &[u8],
) -> (
    std::sync::Arc<AppState>,
    std::sync::Arc<RealtimeRuntimeState>,
) {
    let db = MockDatabase::new(DatabaseBackend::Postgres).into_connection();
    let mut cfg = AppConfig::from_env().expect("load app config");
    cfg.auth = Some(AuthConfig {
        provider: AuthProviderId::Local,
        jwt_secret: String::from_utf8_lossy(secret).into_owned(),
        admin_email: "admin@example.com".to_string(),
        admin_password: "adminpassword".to_string(),
    });
    let services = ServiceContext::new(&db);
    let providers = build_providers(
        cfg.auth.as_ref().expect("auth config should be present"),
        &services,
    )
    .expect("create auth providers");
    let realtime = RealtimeHandle::spawn(cfg.realtime.clone());
    let realtime_runtime = std::sync::Arc::new(RealtimeRuntimeState::new(
        realtime,
        AppRealtimeVerifier::new(providers.clone()),
    ));
    let state = AppState::new(cfg, db, providers);
    (state, realtime_runtime)
}

fn app(secret: &[u8]) -> Router {
    let (state, realtime_runtime) = build_state(secret);
    router(state, realtime_runtime)
        .layer(middleware::from_fn(json_error_middleware))
        .layer(catch_panic_layer())
}

fn auth_header(secret: &[u8], roles: Vec<Role>) -> String {
    let claims = make_access_claims(&Uuid::new_v4(), roles, 3600);
    let jwt = JwtKeys::from_secret(secret);
    let token = encode_token(&jwt, &claims).expect("encode token");
    format!("Bearer {token}")
}

async fn json_response(app: Router, request: Request<Body>) -> (StatusCode, serde_json::Value) {
    let response = app.oneshot(request).await.expect("request should succeed");
    let status = response.status();
    let body = body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body should read");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("body should be json");
    (status, json)
}

#[tokio::test]
async fn todo_crud_create_requires_auth_header() {
    let secret = b"mock-routes-secret";
    let (status, json) = json_response(
        app(secret),
        Request::builder()
            .method("POST")
            .uri(api_path("/todo-crud"))
            .header("content-type", "application/json")
            .body(Body::from(json!({ "title": "Test" }).to_string()))
            .unwrap(),
    )
    .await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(json["message"], "Missing/invalid Authorization header");
}

#[tokio::test]
async fn todo_crud_create_rejects_non_user_role() {
    let secret = b"mock-routes-secret";
    let auth = auth_header(secret, vec![Role::Admin]);

    let (status, json) = json_response(
        app(secret),
        Request::builder()
            .method("POST")
            .uri(api_path("/todo-crud"))
            .header("authorization", auth)
            .header("content-type", "application/json")
            .body(Body::from(json!({ "title": "Test" }).to_string()))
            .unwrap(),
    )
    .await;

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(json["message"], "Missing required role");
}

#[tokio::test]
async fn todo_crud_list_rejects_invalid_pagination_without_touching_db() {
    let secret = b"mock-routes-secret";
    let (status, json) = json_response(
        app(secret),
        Request::builder()
            .method("GET")
            .uri(api_path("/todo-crud?page=0&page_size=25"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json["message"], "Invalid pagination: page=0 page_size=25");
}

#[tokio::test]
async fn todo_crud_list_rejects_invalid_filter_value_shape() {
    let secret = b"mock-routes-secret";
    let (status, json) = json_response(
        app(secret),
        Request::builder()
            .method("GET")
            .uri(api_path("/todo-crud?title=a*b"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json["message"], "Invalid filter value");
}

#[tokio::test]
async fn admin_route_allows_admin_token() {
    let secret = b"mock-routes-secret";
    let auth = auth_header(secret, vec![Role::Admin]);

    let (status, json) = json_response(
        app(secret),
        Request::builder()
            .method("GET")
            .uri(api_path("/admin/stats"))
            .header("authorization", auth)
            .body(Body::empty())
            .unwrap(),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["data"]["ok"], true);
}

#[tokio::test]
async fn admin_route_rejects_user_token() {
    let secret = b"mock-routes-secret";
    let auth = auth_header(secret, vec![Role::User]);

    let (status, json) = json_response(
        app(secret),
        Request::builder()
            .method("GET")
            .uri(api_path("/admin/stats"))
            .header("authorization", auth)
            .body(Body::empty())
            .unwrap(),
    )
    .await;

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(json["message"], "Missing required role");
}

#[tokio::test]
async fn unknown_route_is_normalized_to_json_error() {
    let secret = b"mock-routes-secret";
    let (status, json) = json_response(
        app(secret),
        Request::builder()
            .method("GET")
            .uri(api_path("/unknown-route"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(json["status"], StatusCode::NOT_FOUND.as_u16());
    assert!(!json["message"].as_str().unwrap_or("").is_empty());
}

#[tokio::test]
async fn panic_route_is_caught_and_returned_as_json() {
    let secret = b"mock-routes-secret";
    let (status, json) = json_response(
        app(secret),
        Request::builder()
            .method("GET")
            .uri(api_path("/todo/panic"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(json["status"], StatusCode::INTERNAL_SERVER_ERROR.as_u16());
    assert!(
        json["message"]
            .as_str()
            .unwrap_or("")
            .starts_with("internal server error"),
        "unexpected message: {}",
        json["message"]
    );
}
