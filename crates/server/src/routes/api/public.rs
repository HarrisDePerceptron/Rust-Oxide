use axum::{Router, routing::get};

use crate::response::{ApiResult, JsonApiResponse};
use crate::routes::route_list::{RouteInfo, routes};

pub fn router() -> Router {
    Router::new()
        .route("/public", get(handler))
        .route("/routes.json", get(list_routes_json))
}

async fn handler() -> ApiResult<serde_json::Value> {
    JsonApiResponse::ok(serde_json::json!({ "ok": true, "route": "public" }))
}

async fn list_routes_json() -> ApiResult<&'static [RouteInfo]> {
    JsonApiResponse::ok(routes())
}
