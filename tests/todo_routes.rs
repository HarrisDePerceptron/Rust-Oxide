use std::time::Duration;

use axum::{
    body::{self, Body},
    http::{Request, StatusCode},
};
use sea_orm::{ConnectOptions, Database};
use serde_json::json;
use tower::ServiceExt;
use uuid::Uuid;

use sample_server::{config::AppConfig, db::dao::DaoContext, routes::router, state::AppState};

async fn app_state() -> std::sync::Arc<AppState> {
    let cfg = AppConfig::from_env().expect("load app config");
    let mut opt = ConnectOptions::new(cfg.database_url.clone());
    opt.max_connections(cfg.db_max_connections)
        .min_connections(cfg.db_min_idle)
        .connect_timeout(Duration::from_secs(5))
        .sqlx_logging(false);

    let db = Database::connect(opt).await.expect("connect to database");
    db.get_schema_registry("sample_server::db::entities::*")
        .sync(&db)
        .await
        .expect("sync schema");

    let mut cfg = cfg;
    cfg.jwt_secret = "test-secret".to_string();
    AppState::new(cfg, db)
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

#[tokio::test]
#[ignore = "requires Postgres database"]
async fn todo_crud_flow() {
    let state = app_state().await;
    let title = format!("Test List {}", Uuid::new_v4());

    let (status, list) = json_response(
        &state,
        Request::builder()
            .method("POST")
            .uri("/todo")
            .header("content-type", "application/json")
            .body(Body::from(json!({ "title": title }).to_string()))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let list_id = Uuid::parse_str(list["id"].as_str().unwrap()).unwrap();
    let list_id_str = list_id.to_string();

    let (status, lists) = json_response(
        &state,
        Request::builder()
            .uri("/todo")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(lists
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["id"].as_str() == Some(list_id_str.as_str())));

    let new_title = format!("Updated {}", Uuid::new_v4());
    let new_title_payload = new_title.clone();
    let (status, updated) = json_response(
        &state,
        Request::builder()
            .method("PATCH")
            .uri(format!("/todo/{}", list_id))
            .header("content-type", "application/json")
            .body(Body::from(json!({ "title": new_title_payload }).to_string()))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["id"].as_str().unwrap(), list_id.to_string());
    assert_eq!(updated["title"].as_str().unwrap(), new_title);

    let (status, item) = json_response(
        &state,
        Request::builder()
            .method("POST")
            .uri(format!("/todo/{}/items", list_id))
            .header("content-type", "application/json")
            .body(Body::from(
                json!({ "description": "First item" }).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let item_id = Uuid::parse_str(item["id"].as_str().unwrap()).unwrap();
    assert_eq!(item["done"].as_bool(), Some(false));

    let (status, items) = json_response(
        &state,
        Request::builder()
            .uri(format!("/todo/{}/items", list_id))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(items.as_array().unwrap().len(), 1);

    let (status, updated_item) = json_response(
        &state,
        Request::builder()
            .method("PATCH")
            .uri(format!("/todo/{}/items/{}", list_id, item_id))
            .header("content-type", "application/json")
            .body(Body::from(
                json!({ "description": "Updated item", "done": true }).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated_item["description"].as_str(), Some("Updated item"));
    assert_eq!(updated_item["done"].as_bool(), Some(true));

    let (status, updated_item) = json_response(
        &state,
        Request::builder()
            .method("PATCH")
            .uri(format!("/todo/{}/items/{}", list_id, item_id))
            .header("content-type", "application/json")
            .body(Body::from(json!({ "done": false }).to_string()))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated_item["done"].as_bool(), Some(false));

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

    let (status, _) = json_response(
        &state,
        Request::builder()
            .method("POST")
            .uri(format!("/todo/{}/items", list_id))
            .header("content-type", "application/json")
            .body(Body::from(
                json!({ "description": "Cascade item" }).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

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

    let response = send(
        &state,
        Request::builder()
            .uri(format!("/todo/{}", list_id))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let todo_dao = DaoContext::new(&state.db).todo();
    let remaining = todo_dao
        .count_items_by_list(&list_id)
        .await
        .expect("count items");
    assert_eq!(remaining, 0);
}
