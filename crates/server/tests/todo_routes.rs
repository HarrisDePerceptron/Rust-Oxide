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
        jwt::{encode_token, make_access_claims},
        providers::{AuthProviders, LocalAuthProvider},
    },
    config::AppConfig,
    db::dao::DaoContext,
    routes::router,
    services::user_service,
    state::AppState,
    state::JwtKeys,
};

async fn app_state() -> std::sync::Arc<AppState> {
    let cfg = AppConfig::from_env().expect("load app config");
    let mut opt = ConnectOptions::new(cfg.database_url.clone());
    opt.max_connections(cfg.db_max_connections)
        .min_connections(cfg.db_min_idle)
        .connect_timeout(Duration::from_secs(5))
        .sqlx_logging(false);

    let db = Database::connect(opt).await.expect("connect to database");
    db.get_schema_registry("rust_oxide::db::entities::*")
        .sync(&db)
        .await
        .expect("sync schema");

    let mut cfg = cfg;
    cfg.jwt_secret = "test-secret".to_string();
    build_state(cfg, db)
}

fn build_state(cfg: AppConfig, db: DatabaseConnection) -> std::sync::Arc<AppState> {
    let jwt = JwtKeys::from_secret(cfg.jwt_secret.as_bytes());
    let daos = DaoContext::new(&db);
    let user_service = user_service::UserService::new(daos.user());
    let local_provider = LocalAuthProvider::new(
        user_service,
        daos.refresh_token(),
        jwt.clone(),
    );
    let mut providers = AuthProviders::new(cfg.auth_provider)
        .with_provider(std::sync::Arc::new(local_provider))
        .expect("create auth providers");
    providers
        .set_active(cfg.auth_provider)
        .expect("set active auth provider");
    AppState::new(cfg, db, jwt, providers)
}

async fn send(
    state: &std::sync::Arc<AppState>,
    request: Request<Body>,
) -> axum::response::Response {
    router(state.clone()).oneshot(request).await.unwrap()
}

async fn json_response(
    state: &std::sync::Arc<AppState>,
    request: Request<Body>,
) -> (StatusCode, serde_json::Value) {
    let response = send(state, request).await;
    let status = response.status();
    let body = body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    (status, json)
}

fn json_data<'a>(json: &'a serde_json::Value) -> &'a serde_json::Value {
    json.get("data").unwrap_or(json)
}

fn json_message<'a>(json: &'a serde_json::Value) -> Option<&'a str> {
    json.get("message")
        .and_then(|value| value.as_str())
        .or_else(|| json.get("error").and_then(|value| value.as_str()))
}

fn auth_header(state: &std::sync::Arc<AppState>) -> String {
    let user_id = Uuid::new_v4();
    let claims = make_access_claims(&user_id, vec![Role::User], 3600);
    let token = encode_token(&state.jwt, &claims).expect("encode token");
    format!("Bearer {token}")
}

async fn create_todo_list(
    state: &std::sync::Arc<AppState>,
    title: &str,
) -> (StatusCode, serde_json::Value) {
    json_response(
        state,
        Request::builder()
            .method("POST")
            .uri("/todo")
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
            .uri(format!("/todo/{}/items", list_id))
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
            .uri("/todo-crud")
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
        Request::builder().uri("/todo").body(Body::empty()).unwrap(),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let lists = json_data(&lists);
    assert!(lists
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["id"].as_str() == Some(list_id.as_str())));
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
            .uri(format!("/todo/{}", list_id))
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
            .uri(format!("/todo/{}", list_id))
            .header("content-type", "application/json")
            .body(Body::from(
                json!({ "title": new_title }).to_string(),
            ))
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
            .uri(format!("/todo/{}", list_id))
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
            .uri(format!("/todo/{}/items", list_id))
            .body(Body::empty())
            .unwrap(),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let items = json_data(&items);
    assert!(items
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["id"].as_str() == Some(item_id.as_str())));
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
            .uri(format!("/todo/{}/items/{}", list_id, item_id))
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
            .uri(format!("/todo/{}/items/{}", list_id, item_id))
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
            .uri("/todo-crud")
            .header("authorization", auth)
            .body(Body::empty())
            .unwrap(),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let response = json_data(&response);
    assert!(response["data"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["id"].as_str() == Some(list_id.as_str())));
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
            .uri(format!("/todo-crud/{}", list_id))
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
            .uri(format!("/todo-crud/{}", list_id))
            .header("authorization", auth)
            .header("content-type", "application/json")
            .body(Body::from(
                json!({ "title": new_title }).to_string(),
            ))
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
            .uri(format!("/todo-crud/{}", list_id))
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
            .uri("/todo-crud/count")
            .header("authorization", auth.clone())
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let count_before = json_data(&count_before)["count"].as_u64().unwrap();

    let title = format!("Count List {}", Uuid::new_v4());
    let (status, _) = create_todo_crud_list(&state, &auth, &title).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, count_after) = json_response(
        &state,
        Request::builder()
            .uri("/todo-crud/count")
            .header("authorization", auth)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let count_after = json_data(&count_after)["count"].as_u64().unwrap();
    assert!(count_after > count_before);
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
            .uri(format!("/todo-crud/{}/items/count", list_id))
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
            .uri("/todo")
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
            .uri(format!("/todo/{}", list_id))
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
            .uri(format!("/todo/{}/items", list_id))
            .header("content-type", "application/json")
            .body(Body::from(
                json!({ "description": "   " }).to_string(),
            ))
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
            .uri(format!("/todo/{}/items/{}", list_id, item_id))
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
            .uri(format!("/todo/{}", missing_id))
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
            .uri(format!("/todo/{}/items/{}", list_id, missing_item))
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
            .uri(format!("/todo/{}/items/{}", list_id, missing_item))
            .body(Body::empty())
            .unwrap(),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(json_message(&response), Some("Todo item not found"));
}
