use std::time::Duration;

use axum::{
    body::{self, Body},
    http::{Request, StatusCode},
};
use sea_orm::{ConnectOptions, Database, DatabaseConnection};
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
    routes::{API_PREFIX, router},
    services::ServiceContext,
    state::AppState,
};

async fn app_state() -> std::sync::Arc<AppState> {
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

fn build_state(cfg: AppConfig, db: DatabaseConnection) -> std::sync::Arc<AppState> {
    let services = ServiceContext::new(&db);
    let providers = build_providers(
        cfg.auth.as_ref().expect("auth config should be present"),
        &services,
    )
    .expect("create auth providers");
    AppState::new(cfg, db, providers)
}

fn realtime_runtime_for_state(
    state: &std::sync::Arc<AppState>,
) -> std::sync::Arc<RealtimeRuntimeState> {
    let realtime = RealtimeHandle::spawn(state.config.realtime.clone());
    std::sync::Arc::new(RealtimeRuntimeState::new(
        realtime,
        std::sync::Arc::new(AppRealtimeVerifier::new(state.auth_providers.clone())),
    ))
}

async fn send(
    state: &std::sync::Arc<AppState>,
    request: Request<Body>,
) -> axum::response::Response {
    router(state.clone(), realtime_runtime_for_state(state))
        .oneshot(request)
        .await
        .unwrap()
}

async fn json_response(
    state: &std::sync::Arc<AppState>,
    request: Request<Body>,
) -> (StatusCode, serde_json::Value) {
    let response = send(state, request).await;
    let status = response.status();
    let body = body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    (status, json)
}

fn json_data(json: &serde_json::Value) -> &serde_json::Value {
    json.get("data").unwrap_or(json)
}

fn json_message(json: &serde_json::Value) -> Option<&str> {
    json.get("message")
        .and_then(|value| value.as_str())
        .or_else(|| json.get("error").and_then(|value| value.as_str()))
}

fn auth_header(state: &std::sync::Arc<AppState>) -> String {
    let user_id = Uuid::new_v4();
    let claims = make_access_claims(&user_id, vec![Role::User], 3600);
    let jwt = JwtKeys::from_secret(
        state
            .config
            .auth
            .as_ref()
            .expect("auth config should be present")
            .jwt_secret
            .as_bytes(),
    );
    let token = encode_token(&jwt, &claims).expect("encode token");
    format!("Bearer {token}")
}

fn test_auth_config(jwt_secret: String) -> AuthConfig {
    AuthConfig {
        provider: AuthProviderId::Local,
        jwt_secret,
        admin_email: "admin@example.com".to_string(),
        admin_password: "adminpassword".to_string(),
    }
}

fn api_path(path: &str) -> String {
    format!("{API_PREFIX}{path}")
}

async fn create_todo_list(
    state: &std::sync::Arc<AppState>,
    title: &str,
) -> (StatusCode, serde_json::Value) {
    json_response(
        state,
        Request::builder()
            .method("POST")
            .uri(api_path("/todo"))
            .header("content-type", "application/json")
            .body(Body::from(json!({ "title": title }).to_string()))
            .unwrap(),
    )
    .await
}

async fn create_todo_item(
    state: &std::sync::Arc<AppState>,
    list_id: &Uuid,
    description: &str,
) -> (StatusCode, serde_json::Value) {
    json_response(
        state,
        Request::builder()
            .method("POST")
            .uri(api_path(&format!("/todo/{}/items", list_id)))
            .header("content-type", "application/json")
            .body(Body::from(
                json!({ "description": description }).to_string(),
            ))
            .unwrap(),
    )
    .await
}

async fn create_todo_crud_list(
    state: &std::sync::Arc<AppState>,
    auth: &str,
    title: &str,
) -> (StatusCode, serde_json::Value) {
    json_response(
        state,
        Request::builder()
            .method("POST")
            .uri(api_path("/todo-crud"))
            .header("authorization", auth)
            .header("content-type", "application/json")
            .body(Body::from(json!({ "title": title }).to_string()))
            .unwrap(),
    )
    .await
}

#[tokio::test]
#[ignore = "requires Postgres database"]
async fn todo_create_list() {
    let state = app_state().await;
    let title = format!("Test List {}", Uuid::new_v4());

    let (status, list) = create_todo_list(&state, &title).await;
    let list = json_data(&list);

    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(list["title"].as_str(), Some(title.as_str()));
    assert!(list["id"].as_str().is_some());
}

#[tokio::test]
#[ignore = "requires Postgres database"]
async fn todo_list_lists() {
    let state = app_state().await;
    let title = format!("List {}", Uuid::new_v4());

    let (_, list) = create_todo_list(&state, &title).await;
    let list = json_data(&list);
    let list_id = list["id"].as_str().unwrap().to_string();

    let (status, lists) = json_response(
        &state,
        Request::builder()
            .uri(api_path("/todo"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let lists = json_data(&lists);
    assert!(
        lists
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["id"].as_str() == Some(list_id.as_str()))
    );
}

#[tokio::test]
#[ignore = "requires Postgres database"]
async fn todo_get_list() {
    let state = app_state().await;
    let title = format!("List {}", Uuid::new_v4());

    let (_, list) = create_todo_list(&state, &title).await;
    let list = json_data(&list);
    let list_id = list["id"].as_str().unwrap();

    let (status, response) = json_response(
        &state,
        Request::builder()
            .uri(api_path(&format!("/todo/{}", list_id)))
            .body(Body::empty())
            .unwrap(),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let response = json_data(&response);
    assert_eq!(response["list"]["id"].as_str(), Some(list_id));
    assert_eq!(response["list"]["title"].as_str(), Some(title.as_str()));
    assert_eq!(response["items"].as_array().unwrap().len(), 0);
}

#[tokio::test]
#[ignore = "requires Postgres database"]
async fn todo_update_list() {
    let state = app_state().await;
    let title = format!("List {}", Uuid::new_v4());

    let (_, list) = create_todo_list(&state, &title).await;
    let list = json_data(&list);
    let list_id = list["id"].as_str().unwrap();

    let new_title = format!("Updated {}", Uuid::new_v4());
    let (status, updated) = json_response(
        &state,
        Request::builder()
            .method("PATCH")
            .uri(api_path(&format!("/todo/{}", list_id)))
            .header("content-type", "application/json")
            .body(Body::from(json!({ "title": new_title }).to_string()))
            .unwrap(),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let updated = json_data(&updated);
    assert_eq!(updated["id"].as_str(), Some(list_id));
    assert_eq!(updated["title"].as_str(), Some(new_title.as_str()));
}

#[tokio::test]
#[ignore = "requires Postgres database"]
async fn todo_delete_list() {
    let state = app_state().await;
    let title = format!("List {}", Uuid::new_v4());

    let (_, list) = create_todo_list(&state, &title).await;
    let list = json_data(&list);
    let list_id = list["id"].as_str().unwrap();

    let response = send(
        &state,
        Request::builder()
            .method("DELETE")
            .uri(api_path(&format!("/todo/{}", list_id)))
            .body(Body::empty())
            .unwrap(),
    )
    .await;

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
#[ignore = "requires Postgres database"]
async fn todo_create_item() {
    let state = app_state().await;
    let title = format!("List {}", Uuid::new_v4());

    let (_, list) = create_todo_list(&state, &title).await;
    let list = json_data(&list);
    let list_id = Uuid::parse_str(list["id"].as_str().unwrap()).unwrap();
    let list_id_str = list_id.to_string();

    let (status, item) = create_todo_item(&state, &list_id, "First item").await;

    assert_eq!(status, StatusCode::CREATED);
    let item = json_data(&item);
    assert_eq!(item["list_id"].as_str(), Some(list_id_str.as_str()));
    assert_eq!(item["done"].as_bool(), Some(false));
}

#[tokio::test]
#[ignore = "requires Postgres database"]
async fn todo_list_items() {
    let state = app_state().await;
    let title = format!("List {}", Uuid::new_v4());

    let (_, list) = create_todo_list(&state, &title).await;
    let list = json_data(&list);
    let list_id = Uuid::parse_str(list["id"].as_str().unwrap()).unwrap();

    let (_, item) = create_todo_item(&state, &list_id, "First item").await;
    let item = json_data(&item);
    let item_id = item["id"].as_str().unwrap().to_string();

    let (status, items) = json_response(
        &state,
        Request::builder()
            .uri(api_path(&format!("/todo/{}/items", list_id)))
            .body(Body::empty())
            .unwrap(),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let items = json_data(&items);
    assert!(
        items
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["id"].as_str() == Some(item_id.as_str()))
    );
}

#[tokio::test]
#[ignore = "requires Postgres database"]
async fn todo_update_item() {
    let state = app_state().await;
    let title = format!("List {}", Uuid::new_v4());

    let (_, list) = create_todo_list(&state, &title).await;
    let list = json_data(&list);
    let list_id = Uuid::parse_str(list["id"].as_str().unwrap()).unwrap();

    let (_, item) = create_todo_item(&state, &list_id, "First item").await;
    let item = json_data(&item);
    let item_id = item["id"].as_str().unwrap();

    let (status, updated) = json_response(
        &state,
        Request::builder()
            .method("PATCH")
            .uri(api_path(&format!("/todo/{}/items/{}", list_id, item_id)))
            .header("content-type", "application/json")
            .body(Body::from(json!({ "done": true }).to_string()))
            .unwrap(),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let updated = json_data(&updated);
    assert_eq!(updated["done"].as_bool(), Some(true));
}

#[tokio::test]
#[ignore = "requires Postgres database"]
async fn todo_delete_item() {
    let state = app_state().await;
    let title = format!("List {}", Uuid::new_v4());

    let (_, list) = create_todo_list(&state, &title).await;
    let list = json_data(&list);
    let list_id = Uuid::parse_str(list["id"].as_str().unwrap()).unwrap();

    let (_, item) = create_todo_item(&state, &list_id, "First item").await;
    let item = json_data(&item);
    let item_id = item["id"].as_str().unwrap();

    let response = send(
        &state,
        Request::builder()
            .method("DELETE")
            .uri(api_path(&format!("/todo/{}/items/{}", list_id, item_id)))
            .body(Body::empty())
            .unwrap(),
    )
    .await;

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
#[ignore = "requires Postgres database"]
async fn todo_crud_create_list() {
    let state = app_state().await;
    let auth = auth_header(&state);
    let title = format!("CRUD List {}", Uuid::new_v4());

    let (status, list) = create_todo_crud_list(&state, &auth, &title).await;
    let list = json_data(&list);

    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(list["title"].as_str(), Some(title.as_str()));
    assert!(list["id"].as_str().is_some());
}

#[tokio::test]
#[ignore = "requires Postgres database"]
async fn todo_crud_list_lists() {
    let state = app_state().await;
    let auth = auth_header(&state);
    let title = format!("CRUD List {}", Uuid::new_v4());

    let (_, list) = create_todo_crud_list(&state, &auth, &title).await;
    let list = json_data(&list);
    let list_id = list["id"].as_str().unwrap().to_string();

    let (status, response) = json_response(
        &state,
        Request::builder()
            .uri(api_path("/todo-crud"))
            .header("authorization", auth)
            .body(Body::empty())
            .unwrap(),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let response = json_data(&response);
    assert!(
        response["data"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["id"].as_str() == Some(list_id.as_str()))
    );
}

#[tokio::test]
#[ignore = "requires Postgres database"]
async fn todo_crud_get_list() {
    let state = app_state().await;
    let auth = auth_header(&state);
    let title = format!("CRUD List {}", Uuid::new_v4());

    let (_, list) = create_todo_crud_list(&state, &auth, &title).await;
    let list = json_data(&list);
    let list_id = list["id"].as_str().unwrap();

    let (status, response) = json_response(
        &state,
        Request::builder()
            .uri(api_path(&format!("/todo-crud/{}", list_id)))
            .header("authorization", auth)
            .body(Body::empty())
            .unwrap(),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let response = json_data(&response);
    assert_eq!(response["id"].as_str(), Some(list_id));
    assert_eq!(response["title"].as_str(), Some(title.as_str()));
}

#[tokio::test]
#[ignore = "requires Postgres database"]
async fn todo_crud_update_list() {
    let state = app_state().await;
    let auth = auth_header(&state);
    let title = format!("CRUD List {}", Uuid::new_v4());

    let (_, list) = create_todo_crud_list(&state, &auth, &title).await;
    let list = json_data(&list);
    let list_id = list["id"].as_str().unwrap();

    let new_title = format!("Updated {}", Uuid::new_v4());
    let (status, updated) = json_response(
        &state,
        Request::builder()
            .method("PATCH")
            .uri(api_path(&format!("/todo-crud/{}", list_id)))
            .header("authorization", auth)
            .header("content-type", "application/json")
            .body(Body::from(json!({ "title": new_title }).to_string()))
            .unwrap(),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let updated = json_data(&updated);
    assert_eq!(updated["id"].as_str(), Some(list_id));
    assert_eq!(updated["title"].as_str(), Some(new_title.as_str()));
}

#[tokio::test]
#[ignore = "requires Postgres database"]
async fn todo_crud_delete_list() {
    let state = app_state().await;
    let auth = auth_header(&state);
    let title = format!("CRUD List {}", Uuid::new_v4());

    let (_, list) = create_todo_crud_list(&state, &auth, &title).await;
    let list = json_data(&list);
    let list_id = list["id"].as_str().unwrap();

    let response = send(
        &state,
        Request::builder()
            .method("DELETE")
            .uri(api_path(&format!("/todo-crud/{}", list_id)))
            .header("authorization", auth)
            .body(Body::empty())
            .unwrap(),
    )
    .await;

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
#[ignore = "requires Postgres database"]
async fn todo_crud_count_lists() {
    let state = app_state().await;
    let auth = auth_header(&state);

    let (status, count_before) = json_response(
        &state,
        Request::builder()
            .uri(api_path("/todo-crud/count"))
            .header("authorization", auth.clone())
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let _count_before = json_data(&count_before)["count"].as_u64().unwrap();

    let title = format!("Count List {}", Uuid::new_v4());
    let (status, created) = create_todo_crud_list(&state, &auth, &title).await;
    assert_eq!(status, StatusCode::CREATED);
    let created_id = json_data(&created)["id"]
        .as_str()
        .expect("created list id should be present");

    let (status, count_after) = json_response(
        &state,
        Request::builder()
            .uri(api_path("/todo-crud/count"))
            .header("authorization", &auth)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let count_after = json_data(&count_after)["count"].as_u64().unwrap();

    let (status, fetched) = json_response(
        &state,
        Request::builder()
            .uri(api_path(&format!("/todo-crud/{created_id}")))
            .header("authorization", &auth)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json_data(&fetched)["id"].as_str(), Some(created_id));
    assert!(count_after >= 1);
}

#[tokio::test]
#[ignore = "requires Postgres database"]
async fn todo_crud_count_items() {
    let state = app_state().await;
    let auth = auth_header(&state);
    let title = format!("Count List {}", Uuid::new_v4());

    let (status, list) = create_todo_crud_list(&state, &auth, &title).await;
    assert_eq!(status, StatusCode::CREATED);
    let list = json_data(&list);
    let list_id = list["id"].as_str().unwrap();

    let (status, response) = json_response(
        &state,
        Request::builder()
            .uri(api_path(&format!("/todo-crud/{}/items/count", list_id)))
            .header("authorization", auth)
            .body(Body::empty())
            .unwrap(),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json_data(&response)["count"].as_u64(), Some(0));
}

#[tokio::test]
#[ignore = "requires Postgres database"]
async fn todo_create_list_requires_title() {
    let state = app_state().await;

    let (status, response) = json_response(
        &state,
        Request::builder()
            .method("POST")
            .uri(api_path("/todo"))
            .header("content-type", "application/json")
            .body(Body::from(json!({ "title": "   " }).to_string()))
            .unwrap(),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json_message(&response), Some("Title required"));
}

#[tokio::test]
#[ignore = "requires Postgres database"]
async fn todo_update_list_requires_title() {
    let state = app_state().await;
    let title = format!("List {}", Uuid::new_v4());

    let (_, list) = create_todo_list(&state, &title).await;
    let list = json_data(&list);
    let list_id = list["id"].as_str().unwrap();

    let (status, response) = json_response(
        &state,
        Request::builder()
            .method("PATCH")
            .uri(api_path(&format!("/todo/{}", list_id)))
            .header("content-type", "application/json")
            .body(Body::from(json!({ "title": " " }).to_string()))
            .unwrap(),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json_message(&response), Some("Title required"));
}

#[tokio::test]
#[ignore = "requires Postgres database"]
async fn todo_create_item_requires_description() {
    let state = app_state().await;
    let title = format!("List {}", Uuid::new_v4());

    let (_, list) = create_todo_list(&state, &title).await;
    let list = json_data(&list);
    let list_id = list["id"].as_str().unwrap();

    let (status, response) = json_response(
        &state,
        Request::builder()
            .method("POST")
            .uri(api_path(&format!("/todo/{}/items", list_id)))
            .header("content-type", "application/json")
            .body(Body::from(json!({ "description": "   " }).to_string()))
            .unwrap(),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json_message(&response), Some("Description required"));
}

#[tokio::test]
#[ignore = "requires Postgres database"]
async fn todo_update_item_requires_payload() {
    let state = app_state().await;
    let title = format!("List {}", Uuid::new_v4());

    let (_, list) = create_todo_list(&state, &title).await;
    let list = json_data(&list);
    let list_id = Uuid::parse_str(list["id"].as_str().unwrap()).unwrap();

    let (_, item) = create_todo_item(&state, &list_id, "First item").await;
    let item = json_data(&item);
    let item_id = item["id"].as_str().unwrap();

    let (status, response) = json_response(
        &state,
        Request::builder()
            .method("PATCH")
            .uri(api_path(&format!("/todo/{}/items/{}", list_id, item_id)))
            .header("content-type", "application/json")
            .body(Body::from(json!({}).to_string()))
            .unwrap(),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(
        json_message(&response),
        Some("Description or done required")
    );
}

#[tokio::test]
#[ignore = "requires Postgres database"]
async fn todo_get_list_not_found() {
    let state = app_state().await;
    let missing_id = Uuid::new_v4();

    let (status, response) = json_response(
        &state,
        Request::builder()
            .uri(api_path(&format!("/todo/{}", missing_id)))
            .body(Body::empty())
            .unwrap(),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(json_message(&response), Some("Resource not found"));
}

#[tokio::test]
#[ignore = "requires Postgres database"]
async fn todo_update_item_not_found() {
    let state = app_state().await;
    let title = format!("List {}", Uuid::new_v4());

    let (_, list) = create_todo_list(&state, &title).await;
    let list = json_data(&list);
    let list_id = Uuid::parse_str(list["id"].as_str().unwrap()).unwrap();
    let missing_item = Uuid::new_v4();

    let (status, response) = json_response(
        &state,
        Request::builder()
            .method("PATCH")
            .uri(api_path(&format!(
                "/todo/{}/items/{}",
                list_id, missing_item
            )))
            .header("content-type", "application/json")
            .body(Body::from(json!({ "done": true }).to_string()))
            .unwrap(),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(json_message(&response), Some("Todo item not found"));
}

#[tokio::test]
#[ignore = "requires Postgres database"]
async fn todo_delete_item_not_found() {
    let state = app_state().await;
    let title = format!("List {}", Uuid::new_v4());

    let (_, list) = create_todo_list(&state, &title).await;
    let list = json_data(&list);
    let list_id = Uuid::parse_str(list["id"].as_str().unwrap()).unwrap();
    let missing_item = Uuid::new_v4();

    let (status, response) = json_response(
        &state,
        Request::builder()
            .method("DELETE")
            .uri(api_path(&format!(
                "/todo/{}/items/{}",
                list_id, missing_item
            )))
            .body(Body::empty())
            .unwrap(),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(json_message(&response), Some("Todo item not found"));
}
