use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

pub const DEFAULT_EVENT: &str = "message";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorPayload {
    pub code: String,
    pub message: String,
}

impl ErrorPayload {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum ClientFrame {
    ChannelJoin {
        id: String,
        channel: String,
        #[serde(default)]
        ts: Option<i64>,
    },
    ChannelLeave {
        id: String,
        channel: String,
        #[serde(default)]
        ts: Option<i64>,
    },
    ChannelEmit {
        id: String,
        channel: String,
        event: String,
        #[serde(default)]
        data: Value,
        #[serde(default)]
        ts: Option<i64>,
    },
    Ping {
        id: String,
        #[serde(default)]
        ts: Option<i64>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum ServerFrame {
    Connected {
        id: String,
        conn_id: String,
        user_id: String,
        ts: i64,
    },
    Joined {
        id: String,
        channel: String,
        ts: i64,
    },
    Left {
        id: String,
        channel: String,
        ts: i64,
    },
    Event {
        id: String,
        channel: String,
        event: String,
        data: Value,
        from_user: Option<String>,
        ts: i64,
    },
    Ack {
        id: String,
        for_id: String,
        ok: bool,
        error: Option<ErrorPayload>,
        ts: i64,
    },
    Pong {
        id: String,
        ts: i64,
    },
    Error {
        id: String,
        error: ErrorPayload,
        ts: i64,
    },
}

impl ServerFrame {
    pub fn connected(conn_id: impl Into<String>, user_id: impl Into<String>) -> Self {
        Self::Connected {
            id: random_id(),
            conn_id: conn_id.into(),
            user_id: user_id.into(),
            ts: now_unix_i64(),
        }
    }

    pub fn event(
        channel: impl Into<String>,
        event: impl Into<String>,
        data: Value,
        from_user: Option<String>,
    ) -> Self {
        Self::Event {
            id: random_id(),
            channel: channel.into(),
            event: event.into(),
            data,
            from_user,
            ts: now_unix_i64(),
        }
    }

    pub fn ack_ok(for_id: impl Into<String>) -> Self {
        Self::Ack {
            id: random_id(),
            for_id: for_id.into(),
            ok: true,
            error: None,
            ts: now_unix_i64(),
        }
    }

    pub fn ack_err(for_id: impl Into<String>, code: &str, message: &str) -> Self {
        Self::Ack {
            id: random_id(),
            for_id: for_id.into(),
            ok: false,
            error: Some(ErrorPayload::new(code, message)),
            ts: now_unix_i64(),
        }
    }

    pub fn pong(for_id: impl Into<String>) -> Self {
        Self::Pong {
            id: for_id.into(),
            ts: now_unix_i64(),
        }
    }

    pub fn error(code: &str, message: &str) -> Self {
        Self::Error {
            id: random_id(),
            error: ErrorPayload::new(code, message),
            ts: now_unix_i64(),
        }
    }
}

fn random_id() -> String {
    Uuid::new_v4().to_string()
}

fn now_unix_i64() -> i64 {
    Utc::now().timestamp()
}
