use axum::{Router, routing::get};

#[cfg(debug_assertions)]
use crate::routes::route_list::{RouteInfo, routes};
use crate::routes::{ApiResult, JsonApiResponse};

pub fn router() -> Router {
    let router = Router::new().route("/public", get(handler));
    #[cfg(debug_assertions)]
    let router = router.route("/routes.json", get(list_routes_json));
    router
}

async fn handler() -> ApiResult<serde_json::Value> {
    JsonApiResponse::ok(serde_json::json!({ "ok": true, "route": "public" }))
}

#[cfg(debug_assertions)]
async fn list_routes_json() -> ApiResult<&'static [RouteInfo]> {
    JsonApiResponse::ok(routes())
}
