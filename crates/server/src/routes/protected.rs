use std::sync::Arc;

use axum::{Router, extract::State, middleware, routing::get};

use crate::{
    auth::{Claims, jwt::jwt_auth},
    db::dao::DaoContext,
    response::{ApiResult, JsonApiResponse},
    services::user_service,
    state::AppState,
};

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/me", get(me))
        .route_layer(middleware::from_fn_with_state(state.clone(), jwt_auth))
        .with_state(state)
}

async fn me(
    State(state): State<Arc<AppState>>,
    claims: Claims,
) -> ApiResult<serde_json::Value> {
    let service = user_service_from_state(state.as_ref());
    let user = if let Ok(id) = claims.sub.parse() {
        service.find_by_id(&id).await.ok().flatten()
    } else {
        None
    };

    let email = user.as_ref().map(|u| u.email.as_str()).unwrap_or("unknown");
    let role = user
        .as_ref()
        .map(|u| u.role.as_str())
        .unwrap_or("user")
        .to_string();

    JsonApiResponse::ok(serde_json::json!({
        "ok": true,
        "sub": claims.sub,
        "email": email,
        "role": role,
        "iat": claims.iat,
        "exp": claims.exp
    }))
}

fn user_service_from_state(state: &AppState) -> user_service::UserService {
    let daos = DaoContext::new(&state.db);
    user_service::UserService::new(daos.user())
}
