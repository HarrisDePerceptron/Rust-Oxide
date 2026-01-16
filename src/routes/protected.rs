use std::sync::Arc;

use axum::{Json, Router, extract::State, middleware, routing::get};

use crate::{
    auth::{Claims, jwt::jwt_auth},
    services::user_service,
    state::AppState,
};

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/me", get(me))
        .route_layer(middleware::from_fn_with_state(state.clone(), jwt_auth))
        .with_state(state)
}

async fn me(State(state): State<Arc<AppState>>, claims: Claims) -> Json<serde_json::Value> {
    let user = if let Ok(id) = claims.sub.parse() {
        user_service::find_by_id(&state.db, &id).await.ok().flatten()
    } else {
        None
    };

    let email = user.as_ref().map(|u| u.email.as_str()).unwrap_or("unknown");
    let role = user
        .as_ref()
        .map(|u| u.role.as_str())
        .unwrap_or("user")
        .to_string();

    Json(serde_json::json!({
        "ok": true,
        "sub": claims.sub,
        "email": email,
        "role": role,
        "iat": claims.iat,
        "exp": claims.exp
    }))
}
