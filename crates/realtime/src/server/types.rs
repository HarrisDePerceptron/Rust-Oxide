use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::RealtimeError;

pub type UserId = String;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ConnectionId(pub Uuid);

impl ConnectionId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for ConnectionId {
    fn default() -> Self {
        Self::new()
    }
}

impl Display for ConnectionId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ChannelName(pub String);

impl ChannelName {
    pub const MAX_LEN: usize = 128;

    pub fn parse(raw: &str) -> Result<Self, RealtimeError> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(RealtimeError::bad_request("Channel name is required"));
        }
        if trimmed.len() > Self::MAX_LEN {
            return Err(RealtimeError::bad_request("Channel name is too long"));
        }
        if !trimmed
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, ':' | '_' | '-' | '.'))
        {
            return Err(RealtimeError::bad_request(
                "Channel name contains invalid characters",
            ));
        }
        Ok(Self(trimmed.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for ChannelName {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone)]
pub struct SessionAuth {
    pub user_id: UserId,
    pub roles: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ConnectionMeta {
    pub id: ConnectionId,
    pub user_id: UserId,
    pub roles: Vec<String>,
    pub joined_at_unix: i64,
}

#[derive(Debug, Clone, Copy)]
pub enum DisconnectReason {
    ClientClosed,
    SocketError,
    HubUnavailable,
    SlowConsumer,
    IdleTimeout,
    ProtocolError,
}

#[cfg(test)]
mod tests {
    use super::ChannelName;

    #[test]
    fn channel_name_parse_accepts_valid_symbols() {
        let channel =
            ChannelName::parse("todo:list:123_abc-xyz.test").expect("channel should parse");
        assert_eq!(channel.as_str(), "todo:list:123_abc-xyz.test");
    }

    #[test]
    fn channel_name_parse_rejects_empty_values() {
        let err = ChannelName::parse("   ").expect_err("empty channel should fail");
        assert_eq!(err.message(), "Channel name is required");
    }

    #[test]
    fn channel_name_parse_rejects_invalid_characters() {
        let err = ChannelName::parse("todo/list").expect_err("channel should fail");
        assert_eq!(err.message(), "Channel name contains invalid characters");
    }
}
