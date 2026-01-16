use std::time::Duration;

use axum::{
    body::{self, Body},
    http::{Request, StatusCode},
};
use sea_orm::{ConnectOptions, Database, DatabaseBackend, MockDatabase};
use serde_json::json;
use tower::ServiceExt; // for `oneshot`
use uuid::Uuid;

use sample_server::{
    auth::{Claims, Role, jwt::now_unix, password},
    config::AppConfig,
    db::dao::user_dao,
    routes::router,
    state::AppState,
};

use jsonwebtoken::{Algorithm, Header, encode};

// Build a Router with shared state
fn app() -> axum::Router {
    let secret = b"test-secret";
    let db = MockDatabase::new(DatabaseBackend::Postgres).into_connection();
    let state = AppState::new(secret, db);
    router(state)
}

async fn app_with_db() -> std::sync::Arc<AppState> {
    let cfg = AppConfig::from_env().expect("load app config");
    let mut opt = ConnectOptions::new(cfg.database_url);
    opt.max_connections(cfg.db_max_connections)
        .min_connections(cfg.db_min_idle)
        .connect_timeout(Duration::from_secs(5))
        .sqlx_logging(false);

    let db = Database::connect(opt).await.expect("connect to database");
    db.get_schema_registry("sample_server::db::entities::*")
        .sync(&db)
        .await
        .expect("sync schema");

    AppState::new(b"test-secret", db)
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
#[ignore = "requires Postgres database"]
async fn login_returns_token() {
    let state = app_with_db().await;
    let email = format!("login-{}@example.com", Uuid::new_v4());
    let password_value = "password123";
    let hash = password::hash_password(password_value).unwrap();
    user_dao::create_user(&state.db, &email, &hash, Role::User.as_str())
        .await
        .unwrap();
    let app = router(state);

    let payload = json!({"email": email, "password": password_value});
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
#[ignore = "requires DB with seeded user"]
async fn me_with_token_succeeds() {
    let secret = b"test-secret";
    let db = MockDatabase::new(DatabaseBackend::Postgres).into_connection();
    let state = AppState::new(secret, db);
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
#[ignore = "requires DB with seeded user"]
async fn admin_requires_role() {
    let secret = b"test-secret";
    let db = MockDatabase::new(DatabaseBackend::Postgres).into_connection();
    let app = router(AppState::new(secret, db));

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
