use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use async_trait::async_trait;
use axum::{Json, Router, response::Html, routing::get};
use realtime::server::{
    RealtimeError, RealtimeTokenVerifier, SessionAuth, SocketAppState, SocketServerHandle,
};
use serde::Serialize;
use serde_json::{Value, json};

const DEFAULT_ADDR: &str = "127.0.0.1:4002";
const STROKE_EVENT: &str = "stroke.chunk";
const SYNC_REQUEST_EVENT: &str = "board.sync.request";
const SYNC_SNAPSHOT_EVENT: &str = "board.sync.snapshot";
const BOARD_CLEARED_EVENT: &str = "board.cleared";
const MAX_CHUNKS_PER_BOARD: usize = 12_000;

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

#[derive(Clone, Default)]
struct BoardStore {
    boards: Arc<std::sync::Mutex<HashMap<String, Vec<Value>>>>,
}

impl BoardStore {
    fn append_chunk(&self, board_channel: &str, payload: Value) {
        let mut boards = self.boards.lock().expect("board store mutex poisoned");
        let entries = boards.entry(board_channel.to_string()).or_default();
        entries.push(payload);
        if entries.len() > MAX_CHUNKS_PER_BOARD {
            let drain_len = entries.len().saturating_sub(MAX_CHUNKS_PER_BOARD);
            entries.drain(0..drain_len);
        }
    }

    fn snapshot(&self, board_channel: &str) -> Vec<Value> {
        self.boards
            .lock()
            .expect("board store mutex poisoned")
            .get(board_channel)
            .cloned()
            .unwrap_or_default()
    }

    fn clear_board(&self, board_channel: &str) {
        self.boards
            .lock()
            .expect("board store mutex poisoned")
            .remove(board_channel);
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let users = demo_users();
    let verifier = StaticTokenVerifier::new(&users);
    let socket_server_handle = SocketServerHandle::spawn(Default::default());
    let board_store = BoardStore::default();

    {
        let server = socket_server_handle.clone();
        let board_store = board_store.clone();
        socket_server_handle.on_events(move |channel, event, payload| {
            if !channel.starts_with("board:") {
                return;
            }

            match event.as_str() {
                STROKE_EVENT => {
                    board_store.append_chunk(&channel, payload);
                }
                BOARD_CLEARED_EVENT => {
                    board_store.clear_board(&channel);
                }
                SYNC_REQUEST_EVENT => {
                    let Some(requester_user_id) = payload
                        .get("requester_user_id")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(|value| value.to_string())
                    else {
                        return;
                    };

                    let chunks = board_store.snapshot(&channel);
                    let chunk_count = chunks.len();
                    let snapshot_payload = json!({
                        "board_channel": channel,
                        "snapshot_version": 1,
                        "chunk_count": chunk_count,
                        "chunks": chunks,
                    });

                    let server = server.clone();
                    tokio::spawn(async move {
                        if let Err(err) = server
                            .send_event_to_user(
                                requester_user_id,
                                SYNC_SNAPSHOT_EVENT,
                                snapshot_payload,
                            )
                            .await
                        {
                            eprintln!("failed to send board snapshot: {err}");
                        }
                    });
                }
                _ => {}
            }
        });
    }

    let socket_app_state = Arc::new(SocketAppState::new(socket_server_handle, verifier));

    let app = Router::new()
        .route("/", get(index))
        .route("/demo/users", get(demo_users_handler))
        .nest("/api/v1", realtime::server::axum::router(socket_app_state));

    let addr = demo_addr();
    println!("realtime drawing demo listening on http://{addr}");
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
    let raw =
        std::env::var("REALTIME_DRAWING_DEMO_ADDR").unwrap_or_else(|_| DEFAULT_ADDR.to_string());
    raw.parse()
        .unwrap_or_else(|_| panic!("invalid REALTIME_DRAWING_DEMO_ADDR: {raw}"))
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

const INDEX_HTML: &str = include_str!("views/drawing_demo.html");
