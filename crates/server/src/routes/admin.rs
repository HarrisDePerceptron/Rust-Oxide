use std::sync::Arc;

use axum::{Router, middleware, routing::get};

use crate::{
    auth::{Claims, Role, jwt::jwt_auth, role_layer::RequireRoleLayer},
    response::JsonApiResponse,
    state::AppState,
};

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/admin/stats", get(admin_stats))
        .layer(RequireRoleLayer::new(Role::Admin))
        .route_layer(middleware::from_fn_with_state(state.clone(), jwt_auth))
        .with_state(state)
}

async fn admin_stats(claims: Claims) -> JsonApiResponse<serde_json::Value> {
    JsonApiResponse::ok(serde_json::json!({ "ok": true, "admin": claims.sub }))
}
