use axum::{
    Json, Router,
    body::Body,
    http::{Request, StatusCode, Uri},
    response::Html,
    routing::get,
};
use chrono::Local;
use tokio::fs;
use tower::ServiceBuilder;
use tower_http::services::ServeDir;

use crate::error::AppError;

pub fn router() -> Router {
    Router::new()
        .route_service("/{*file}", ServeDir::new("public"))
        .route("/", get(index))
        .route("/public", get(handler))
}

async fn handler() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "ok": true, "route": "public" }))
}

async fn index() -> Result<Html<String>, AppError> {
    let contents = fs::read_to_string("views/index.html").await.map_err(|_| {
        AppError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to load index page",
        )
    })?;
    let now = Local::now().to_rfc3339();
    let rendered = contents.replace("{{now}}", &now);
    Ok(Html(rendered))
}
