use std::sync::Arc;

use axum::{Router, routing::get};

use crate::{
    routes::{AdminRole, ApiResult, AuthRoleGuard, JsonApiResponse},
    state::AppState,
};

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/admin/stats", get(admin_stats))
        .with_state(state)
}

async fn admin_stats(
    AuthRoleGuard { claims, .. }: AuthRoleGuard<AdminRole>,
) -> ApiResult<serde_json::Value> {
    JsonApiResponse::ok(serde_json::json!({ "ok": true, "admin": claims.sub }))
}
