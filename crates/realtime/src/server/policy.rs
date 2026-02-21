use super::{ChannelName, ConnectionMeta, RealtimeError};

pub trait ChannelPolicy: Send + Sync {
    fn can_join(&self, meta: &ConnectionMeta, channel: &ChannelName) -> Result<(), RealtimeError>;
    fn can_publish(
        &self,
        meta: &ConnectionMeta,
        channel: &ChannelName,
        event: &str,
    ) -> Result<(), RealtimeError>;
}

#[derive(Debug, Default)]
pub struct DefaultChannelPolicy;

impl ChannelPolicy for DefaultChannelPolicy {
    fn can_join(&self, meta: &ConnectionMeta, channel: &ChannelName) -> Result<(), RealtimeError> {
        let name = channel.as_str();
        if let Some(user) = name.strip_prefix("user:") {
            if user == meta.user_id || is_admin(meta) {
                return Ok(());
            }
            return Err(RealtimeError::forbidden(
                "Cannot join another user's private channel",
            ));
        }
        if name.starts_with("admin:") && !is_admin(meta) {
            return Err(RealtimeError::forbidden(
                "Admin channel requires admin role",
            ));
        }
        Ok(())
    }

    fn can_publish(
        &self,
        meta: &ConnectionMeta,
        channel: &ChannelName,
        event: &str,
    ) -> Result<(), RealtimeError> {
        if event.trim().is_empty() {
            return Err(RealtimeError::bad_request("Event name is required"));
        }
        let name = channel.as_str();
        if let Some(user) = name.strip_prefix("user:") {
            if user == meta.user_id || is_admin(meta) {
                return Ok(());
            }
            return Err(RealtimeError::forbidden(
                "Cannot publish to another user's private channel",
            ));
        }
        if name.starts_with("admin:") && !is_admin(meta) {
            return Err(RealtimeError::forbidden(
                "Admin channel requires admin role",
            ));
        }
        Ok(())
    }
}

fn is_admin(meta: &ConnectionMeta) -> bool {
    meta.roles.iter().any(|role| role == "admin")
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;
    use crate::server::ConnectionId;

    fn connection_meta(user_id: &str, roles: Vec<String>) -> ConnectionMeta {
        ConnectionMeta {
            id: ConnectionId(Uuid::new_v4()),
            user_id: user_id.to_string(),
            roles,
            joined_at_unix: 0,
        }
    }

    #[test]
    fn user_cannot_join_another_private_channel() {
        let policy = DefaultChannelPolicy;
        let user_meta = connection_meta("u1", vec!["user".to_string()]);
        let channel = ChannelName::parse("user:u2").expect("channel should parse");

        let err = policy
            .can_join(&user_meta, &channel)
            .expect_err("join should be denied");
        assert_eq!(err.message(), "Cannot join another user's private channel");
    }

    #[test]
    fn admin_can_join_another_private_channel() {
        let policy = DefaultChannelPolicy;
        let admin_meta = connection_meta("admin", vec!["admin".to_string(), "user".to_string()]);
        let channel = ChannelName::parse("user:u2").expect("channel should parse");

        policy
            .can_join(&admin_meta, &channel)
            .expect("admin should be allowed");
    }

    #[test]
    fn user_publish_to_admin_channel_is_denied() {
        let policy = DefaultChannelPolicy;
        let user_meta = connection_meta("u1", vec!["user".to_string()]);
        let channel = ChannelName::parse("admin:ops").expect("channel should parse");

        let err = policy
            .can_publish(&user_meta, &channel, "status.updated")
            .expect_err("publish should be denied");
        assert_eq!(err.message(), "Admin channel requires admin role");
    }
}
