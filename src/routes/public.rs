use axum::{Json, Router, routing::get};

pub fn router() -> Router {
    Router::new().route("/public", get(handler))
}

async fn handler() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "ok": true, "route": "public" }))
}
