use std::sync::Arc;

use axum::{Json, Router, middleware, routing::get};

use crate::{
    auth::{Claims, jwt::jwt_auth},
    state::AppState,
};

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/me", get(me))
        .route_layer(middleware::from_fn_with_state(state.clone(), jwt_auth))
        .with_state(state)
}

async fn me(claims: Claims) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "ok": true,
        "sub": claims.sub,
        "iat": claims.iat,
        "exp": claims.exp
    }))
}
