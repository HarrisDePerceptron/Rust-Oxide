use std::sync::Arc;

use axum::{Router, extract::State, routing::get};

use crate::{
    middleware::AuthGuard,
    response::{ApiResult, JsonApiResponse},
    services::{ServiceContext, user_service},
    state::AppState,
};

pub fn router(state: Arc<AppState>) -> Router {
    Router::new().route("/me", get(me)).with_state(state)
}

async fn me(State(state): State<Arc<AppState>>, claims: AuthGuard) -> ApiResult<serde_json::Value> {
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
    ServiceContext::from_state(state).user()
}
