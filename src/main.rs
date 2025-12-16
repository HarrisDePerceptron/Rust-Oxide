use axum::{
    Json, Router,
    extract::{Request, State},
    http::{StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};
use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use tower_http::trace::TraceLayer;

use sample_server::role_layer::{Claims, RequireRoleLayer, Role};

#[derive(Clone)]
struct AppState {
    jwt: JwtKeys,
}

#[derive(Clone)]
struct JwtKeys {
    enc: EncodingKey,
    dec: DecodingKey,
}

#[derive(Debug, Deserialize)]
struct LoginRequest {
    username: String,
    password: String,
}

#[derive(Debug, Serialize)]
struct TokenResponse {
    access_token: String,
    token_type: &'static str,
}

fn now_unix() -> usize {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as usize
}

// --- Handlers ---

async fn public() -> impl IntoResponse {
    Json(serde_json::json!({ "ok": true, "route": "public" }))
}

async fn login(
    State(state): State<Arc<AppState>>,
    Json(body): Json<LoginRequest>,
) -> Result<Json<TokenResponse>, StatusCode> {
    // Demo auth: replace with DB lookup + password hashing

    if body.username != "admin" || body.password != "admin" {
        return Err(StatusCode::UNAUTHORIZED);
    }

    let iat = now_unix();
    let exp = iat + 60 * 60; // 1 hour

    let roles = if body.username == "admin" {
        vec![Role::User, Role::Admin]
    } else {
        vec![Role::User]
    };

    let claims = Claims {
        sub: body.username,
        roles,
        iat,
        exp,
    };

    let mut header = Header::new(Algorithm::HS256);
    header.typ = Some("JWT".into());

    let token =
        encode(&header, &claims, &state.jwt.enc).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(TokenResponse {
        access_token: token,
        token_type: "Bearer",
    }))
}

async fn me(claims: Claims) -> impl IntoResponse {
    Json(serde_json::json!({
        "ok": true,
        "sub": claims.sub,
        "iat": claims.iat,
        "exp": claims.exp
    }))
}

// --- JWT middleware ---

async fn jwt_auth(
    State(state): State<Arc<AppState>>,
    mut req: Request,
    next: Next,
) -> Result<Response, Response> {
    // Expect: Authorization: Bearer <token>
    let auth = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let token = auth.strip_prefix("Bearer ").ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            "Missing/invalid Authorization header",
        )
            .into_response()
    })?;

    let mut validation = Validation::new(Algorithm::HS256);
    validation.validate_exp = true;

    let data = decode::<Claims>(token, &state.jwt.dec, &validation)
        .map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid or expired token").into_response())?;

    // Put claims into request extensions for downstream handlers
    req.extensions_mut().insert(data.claims);

    Ok(next.run(req).await)
}

async fn admin_stats(claims: Claims) -> impl axum::response::IntoResponse {
    axum::Json(serde_json::json!({ "ok": true, "admin": claims.sub }))
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter("info,tower_http=info")
        .init();

    // In real apps: load from env (dotenv) and keep it secret
    let secret = b"super-secret-change-me";

    let state = Arc::new(AppState {
        jwt: JwtKeys {
            enc: EncodingKey::from_secret(secret),
            dec: DecodingKey::from_secret(secret),
        },
    });

    let protected = Router::new()
        .route("/me", get(me))
        .route_layer(middleware::from_fn_with_state(state.clone(), jwt_auth));

    let admin = Router::new()
        .route("/admin/stats", get(admin_stats))
        .layer(RequireRoleLayer::new(Role::Admin)) // role gate
        .layer(middleware::from_fn_with_state(state.clone(), jwt_auth)); // runs first

    let app = Router::new()
        .route("/public", get(public))
        .route("/login", post(login))
        .merge(protected)
        .merge(admin)
        .with_state(state)
        .layer(TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    tracing::info!("listening on http://localhost:3000");

    axum::serve(listener, app).await.unwrap();
}
