use std::time::Duration;

use axum::{
    body::{self, Body},
    http::{Request, StatusCode},
};
use sea_orm::{ConnectOptions, Database, DatabaseBackend, DatabaseConnection, MockDatabase};
use serde_json::json;
use tower::ServiceExt; // for `oneshot`
use uuid::Uuid;

use rust_oxide::{
    auth::{
        Claims, Role, bootstrap::build_providers, jwt::now_unix, password,
        providers::AuthProviderId,
    },
    config::{AppConfig, AuthConfig},
    db::dao::DaoContext,
    realtime::{AppRealtimeVerifier, RealtimeHandle, RealtimeRuntimeState},
    routes::{API_PREFIX, router},
    services::ServiceContext,
    state::AppState,
};

use jsonwebtoken::{Algorithm, Header, encode};

// Build a Router with shared state
fn app() -> axum::Router {
    let secret = b"test-secret";
    let db = MockDatabase::new(DatabaseBackend::Postgres).into_connection();
    let mut cfg = AppConfig::from_env().expect("load app config");
    cfg.auth = Some(test_auth_config(
        String::from_utf8_lossy(secret).into_owned(),
    ));
    let (state, realtime_runtime) = build_state(cfg, db);
    router(state, realtime_runtime)
}

fn api_path(path: &str) -> String {
    format!("{API_PREFIX}{path}")
}

async fn app_with_db() -> (
    std::sync::Arc<AppState>,
    std::sync::Arc<RealtimeRuntimeState>,
) {
    let cfg = AppConfig::from_env().expect("load app config");
    let db_cfg = cfg
        .database
        .as_ref()
        .expect("database config should be present in integration tests");
    let mut opt = ConnectOptions::new(db_cfg.url.clone());
    opt.max_connections(db_cfg.max_connections)
        .min_connections(db_cfg.min_idle)
        .connect_timeout(Duration::from_secs(5))
        .sqlx_logging(false);

    let db = Database::connect(opt).await.expect("connect to database");
    db.get_schema_registry("rust_oxide::db::entities::*")
        .sync(&db)
        .await
        .expect("sync schema");

    let mut cfg = cfg;
    cfg.auth = Some(test_auth_config("test-secret".to_string()));
    build_state(cfg, db)
}

fn build_state(
    cfg: AppConfig,
    db: DatabaseConnection,
) -> (
    std::sync::Arc<AppState>,
    std::sync::Arc<RealtimeRuntimeState>,
) {
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

#[tokio::test]
async fn public_route_works() {
    let app = app();

    let res = app
        .oneshot(
            Request::builder()
                .uri(api_path("/public"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let payload = json.get("data").unwrap_or(&json);
    assert_eq!(payload["route"], "public");
}

#[tokio::test]
#[ignore = "requires Postgres database"]
async fn login_returns_token() {
    let (state, realtime_runtime) = app_with_db().await;
    let email = format!("login-{}@example.com", Uuid::new_v4());
    let password_value = "password123";
    let hash = password::hash_password(password_value).unwrap();
    let user_dao = DaoContext::new(&state.db).user();
    user_dao
        .create_user(&email, &hash, Role::User.as_str())
        .await
        .unwrap();
    let app = router(state, realtime_runtime);

    let payload = json!({"email": email, "password": password_value});
    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(api_path("/login"))
                .header("content-type", "application/json")
                .body(Body::from(payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let payload = json.get("data").unwrap_or(&json);
    assert!(payload["access_token"].as_str().is_some());
}

#[tokio::test]
async fn me_without_token_is_rejected() {
    let app = app();

    let res = app
        .oneshot(
            Request::builder()
                .uri(api_path("/me"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
#[ignore = "requires DB with seeded user"]
async fn me_with_token_succeeds() {
    let secret = b"test-secret";
    let db = MockDatabase::new(DatabaseBackend::Postgres).into_connection();
    let mut cfg = AppConfig::from_env().expect("load app config");
    cfg.auth = Some(test_auth_config(
        String::from_utf8_lossy(secret).into_owned(),
    ));
    let (state, realtime_runtime) = build_state(cfg, db);
    let app = router(state.clone(), realtime_runtime);

    let token = login_token(secret, vec![Role::User]);

    let res = app
        .oneshot(
            Request::builder()
                .uri(api_path("/me"))
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
    let mut cfg = AppConfig::from_env().expect("load app config");
    cfg.auth = Some(test_auth_config(
        String::from_utf8_lossy(secret).into_owned(),
    ));
    let (state, realtime_runtime) = build_state(cfg, db);
    let app = router(state, realtime_runtime);

    // token without Admin role
    let token = login_token(secret, vec![Role::User]);

    let res = app
        .oneshot(
            Request::builder()
                .uri(api_path("/admin/stats"))
                .header("authorization", format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}

fn test_auth_config(jwt_secret: String) -> AuthConfig {
    AuthConfig {
        provider: AuthProviderId::Local,
        jwt_secret,
        admin_email: "admin@example.com".to_string(),
        admin_password: "adminpassword".to_string(),
    }
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
