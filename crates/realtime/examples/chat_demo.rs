use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use async_trait::async_trait;
use axum::{Json, Router, response::Html, routing::get};
use realtime::server::{
    RealtimeError, RealtimeHandle, RealtimeRuntimeState, RealtimeTokenVerifier, SessionAuth,
};
use serde::Serialize;

const DEFAULT_ADDR: &str = "127.0.0.1:4001";

#[derive(Clone)]
struct DemoUser {
    user_id: String,
    label: String,
    token: String,
    roles: Vec<String>,
}

#[derive(Serialize)]
struct DemoUserView {
    user_id: String,
    label: String,
    token: String,
    roles: Vec<String>,
}

#[derive(Clone)]
struct StaticTokenVerifier {
    sessions: Arc<HashMap<String, SessionAuth>>,
}

impl StaticTokenVerifier {
    fn new(users: &[DemoUser]) -> Self {
        let sessions = users
            .iter()
            .map(|user| {
                (
                    user.token.clone(),
                    SessionAuth {
                        user_id: user.user_id.clone(),
                        roles: user.roles.clone(),
                    },
                )
            })
            .collect();
        Self {
            sessions: Arc::new(sessions),
        }
    }
}

#[async_trait]
impl RealtimeTokenVerifier for StaticTokenVerifier {
    async fn verify_token(&self, token: &str) -> Result<SessionAuth, RealtimeError> {
        let token = token.trim();
        if token.is_empty() {
            return Err(RealtimeError::unauthorized("Missing token"));
        }
        self.sessions
            .get(token)
            .cloned()
            .ok_or_else(|| RealtimeError::unauthorized("Invalid demo token"))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let users = demo_users();
    let verifier = StaticTokenVerifier::new(&users);
    let runtime = Arc::new(RealtimeRuntimeState::new(
        RealtimeHandle::spawn(Default::default()),
        verifier,
    ));

    let app = Router::new()
        .route("/", get(index))
        .route("/demo/users", get(demo_users_handler))
        .nest("/api/v1", realtime::server::axum::router(runtime));

    let addr = demo_addr();
    println!("realtime demo listening on http://{addr}");
    println!("open http://{addr} in your browser");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

fn demo_users() -> Vec<DemoUser> {
    vec![
        DemoUser {
            user_id: "u-alice".to_string(),
            label: "Alice".to_string(),
            token: "demo-token-alice".to_string(),
            roles: vec!["user".to_string()],
        },
        DemoUser {
            user_id: "u-bob".to_string(),
            label: "Bob".to_string(),
            token: "demo-token-bob".to_string(),
            roles: vec!["user".to_string()],
        },
        DemoUser {
            user_id: "u-admin".to_string(),
            label: "Admin".to_string(),
            token: "demo-token-admin".to_string(),
            roles: vec!["admin".to_string(), "user".to_string()],
        },
    ]
}

fn demo_addr() -> SocketAddr {
    let raw = std::env::var("REALTIME_DEMO_ADDR").unwrap_or_else(|_| DEFAULT_ADDR.to_string());
    raw.parse()
        .unwrap_or_else(|_| panic!("invalid REALTIME_DEMO_ADDR: {raw}"))
}

async fn index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

async fn demo_users_handler() -> Json<Vec<DemoUserView>> {
    let users = demo_users()
        .iter()
        .map(|user| DemoUserView {
            user_id: user.user_id.clone(),
            label: user.label.clone(),
            token: user.token.clone(),
            roles: user.roles.clone(),
        })
        .collect();
    Json(users)
}

const INDEX_HTML: &str = include_str!("views/chat_demo.html");
