use axum::{Json, Router, http::StatusCode, response::Html, routing::get};
use askama::Template;
use chrono::Local;
use tower_http::services::ServeDir;

use crate::error::AppError;

#[derive(Template)]
#[template(path = "index.html")]
struct IndexTemplate {
    now: String,
}

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
    let now = Local::now().to_rfc3339();
    let rendered = IndexTemplate { now }
        .render()
        .map_err(|_| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, "failed to render index"))?;
    Ok(Html(rendered))
}
