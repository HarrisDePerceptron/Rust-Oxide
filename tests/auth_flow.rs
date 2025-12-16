use axum::{
    body::{self, Body},
    http::{Request, StatusCode},
};
use serde_json::json;
use tower::ServiceExt; // for `oneshot`

use sample_server::{
    auth::{Claims, Role, jwt::now_unix},
    routes::router,
    state::AppState,
};

use jsonwebtoken::{Algorithm, Header, encode};

// Build a Router with shared state
fn app() -> axum::Router {
    let secret = b"test-secret";
    let state = AppState::new(secret);
    router(state)
}

#[tokio::test]
async fn public_route_works() {
    let app = app();

    let res = app
        .oneshot(
            Request::builder()
                .uri("/public")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["route"], "public");
}

#[tokio::test]
async fn login_returns_token() {
    let app = app();

    let payload = json!({"username": "admin", "password": "admin"});
    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/login")
                .header("content-type", "application/json")
                .body(Body::from(payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(json["access_token"].as_str().is_some());
}

#[tokio::test]
async fn me_without_token_is_rejected() {
    let app = app();

    let res = app
        .oneshot(Request::builder().uri("/me").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn me_with_token_succeeds() {
    let secret = b"test-secret";
    let state = AppState::new(secret);
    let app = router(state.clone());

    let token = login_token(secret, vec![Role::User]);

    let res = app
        .oneshot(
            Request::builder()
                .uri("/me")
                .header("authorization", format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn admin_requires_role() {
    let secret = b"test-secret";
    let app = router(AppState::new(secret));

    // token without Admin role
    let token = login_token(secret, vec![Role::User]);

    let res = app
        .oneshot(
            Request::builder()
                .uri("/admin/stats")
                .header("authorization", format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}

fn login_token(secret: &[u8], roles: Vec<Role>) -> String {
    let iat = now_unix();
    let claims = Claims {
        sub: "user".into(),
        roles,
        iat,
        exp: iat + 3600,
    };

    let mut header = Header::new(Algorithm::HS256);
    header.typ = Some("JWT".into());

    encode(
        &header,
        &claims,
        &jsonwebtoken::EncodingKey::from_secret(secret),
    )
    .unwrap()
}
