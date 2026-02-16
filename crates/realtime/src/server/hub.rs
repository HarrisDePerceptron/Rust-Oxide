use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Instant,
};

use chrono::Utc;
use serde_json::Value;
use tokio::sync::mpsc;

use crate::protocol::{DEFAULT_EVENT, ServerFrame};

use super::{
    ChannelName, ConnectionId, ConnectionMeta, DisconnectReason, RealtimeConfig, RealtimeError,
    SessionAuth,
    policy::{ChannelPolicy, DefaultChannelPolicy},
    session,
};

const HUB_QUEUE_SIZE: usize = 4096;

#[derive(Clone)]
pub struct RealtimeHandle {
    config: RealtimeConfig,
    tx: Option<mpsc::Sender<HubCommand>>,
}

impl RealtimeHandle {
    pub fn spawn(config: RealtimeConfig) -> Self {
        if !config.enabled {
            return Self { config, tx: None };
        }

        let (tx, rx) = mpsc::channel(HUB_QUEUE_SIZE);
        let mut hub = RealtimeHub::new(config.clone(), rx, Arc::new(DefaultChannelPolicy));
        tokio::spawn(async move {
            hub.run().await;
        });

        Self {
            config,
            tx: Some(tx),
        }
    }

    pub fn disabled(config: RealtimeConfig) -> Self {
        Self { config, tx: None }
    }

    pub fn is_enabled(&self) -> bool {
        self.config.enabled && self.tx.is_some()
    }

    pub fn max_message_bytes(&self) -> usize {
        self.config.max_message_bytes
    }

    pub async fn serve_socket(&self, socket: axum::extract::ws::WebSocket, auth: SessionAuth) {
        let Some(hub_tx) = self.tx.clone() else {
            return;
        };
        session::run_socket_session(socket, auth, hub_tx, self.config.clone()).await;
    }

    pub async fn send(
        &self,
        channel_name: impl Into<String>,
        message: Value,
    ) -> Result<(), RealtimeError> {
        self.send_event(channel_name, DEFAULT_EVENT, message).await
    }

    pub async fn send_to_user(
        &self,
        user_id: impl Into<String>,
        message: Value,
    ) -> Result<(), RealtimeError> {
        self.send_event_to_user(user_id, DEFAULT_EVENT, message)
            .await
    }

    pub async fn send_event(
        &self,
        channel_name: impl Into<String>,
        event: impl Into<String>,
        payload: Value,
    ) -> Result<(), RealtimeError> {
        let Some(tx) = &self.tx else {
            return Ok(());
        };
        let channel_name = channel_name.into();
        let channel = ChannelName::parse(&channel_name)?;
        tx.send(HubCommand::SendToChannel {
            channel,
            event: event.into(),
            payload,
        })
        .await
        .map_err(|_| RealtimeError::internal("realtime hub is unavailable"))
    }

    pub async fn send_event_to_user(
        &self,
        user_id: impl Into<String>,
        event: impl Into<String>,
        payload: Value,
    ) -> Result<(), RealtimeError> {
        let Some(tx) = &self.tx else {
            return Ok(());
        };
        tx.send(HubCommand::SendToUser {
            user_id: user_id.into(),
            event: event.into(),
            payload,
        })
        .await
        .map_err(|_| RealtimeError::internal("realtime hub is unavailable"))
    }

    pub async fn emit_to_user(
        &self,
        user_id: impl Into<String>,
        event: impl Into<String>,
        payload: Value,
    ) -> Result<(), RealtimeError> {
        self.send_event_to_user(user_id, event, payload).await
    }
}

pub(crate) enum HubCommand {
    Register {
        meta: ConnectionMeta,
        outbound_tx: mpsc::Sender<ServerFrame>,
    },
    Unregister {
        conn_id: ConnectionId,
        reason: DisconnectReason,
    },
    Join {
        conn_id: ConnectionId,
        channel: ChannelName,
        req_id: String,
    },
    Leave {
        conn_id: ConnectionId,
        channel: ChannelName,
        req_id: String,
    },
    Emit {
        conn_id: ConnectionId,
        channel: ChannelName,
        event: String,
        payload: Value,
        req_id: String,
    },
    Ping {
        conn_id: ConnectionId,
        req_id: String,
    },
    SendToChannel {
        channel: ChannelName,
        event: String,
        payload: Value,
    },
    SendToUser {
        user_id: String,
        event: String,
        payload: Value,
    },
}

struct RealtimeHub {
    config: RealtimeConfig,
    rx: mpsc::Receiver<HubCommand>,
    policy: Arc<dyn ChannelPolicy>,
    connections: HashMap<ConnectionId, ConnectionState>,
    users: HashMap<String, HashSet<ConnectionId>>,
    channels: HashMap<ChannelName, HashSet<ConnectionId>>,
    connection_channels: HashMap<ConnectionId, HashSet<ChannelName>>,
}

struct ConnectionState {
    meta: ConnectionMeta,
    outbound_tx: mpsc::Sender<ServerFrame>,
    rate: ConnectionRateState,
}

struct ConnectionRateState {
    join_window_started_at: Instant,
    joins_in_window: u32,
    emit_window_started_at: Instant,
    emits_in_window: u32,
}

impl RealtimeHub {
    fn new(
        config: RealtimeConfig,
        rx: mpsc::Receiver<HubCommand>,
        policy: Arc<dyn ChannelPolicy>,
    ) -> Self {
        Self {
            config,
            rx,
            policy,
            connections: HashMap::new(),
            users: HashMap::new(),
            channels: HashMap::new(),
            connection_channels: HashMap::new(),
        }
    }

    async fn run(&mut self) {
        while let Some(command) = self.rx.recv().await {
            self.handle_command(command);
        }
    }

    fn handle_command(&mut self, command: HubCommand) {
        match command {
            HubCommand::Register { meta, outbound_tx } => self.register(meta, outbound_tx),
            HubCommand::Unregister { conn_id, reason } => self.unregister(conn_id, reason),
            HubCommand::Join {
                conn_id,
                channel,
                req_id,
            } => self.handle_join(conn_id, channel, req_id),
            HubCommand::Leave {
                conn_id,
                channel,
                req_id,
            } => self.handle_leave(conn_id, channel, req_id),
            HubCommand::Emit {
                conn_id,
                channel,
                event,
                payload,
                req_id,
            } => self.handle_emit(conn_id, channel, event, payload, req_id),
            HubCommand::Ping { conn_id, req_id } => self.handle_ping(conn_id, req_id),
            HubCommand::SendToChannel {
                channel,
                event,
                payload,
            } => self.handle_send_to_channel(channel, event, payload),
            HubCommand::SendToUser {
                user_id,
                event,
                payload,
            } => self.handle_send_to_user(user_id, event, payload),
        }
    }

    fn register(&mut self, meta: ConnectionMeta, outbound_tx: mpsc::Sender<ServerFrame>) {
        if self.connections.len() >= self.config.max_connections {
            let _ = outbound_tx.try_send(ServerFrame::error(
                "capacity_exceeded",
                "Realtime server is at capacity",
            ));
            return;
        }

        let conn_id = meta.id;
        let user_id = meta.user_id.clone();
        let now = Instant::now();

        self.connections.insert(
            conn_id,
            ConnectionState {
                meta,
                outbound_tx: outbound_tx.clone(),
                rate: ConnectionRateState {
                    join_window_started_at: now,
                    joins_in_window: 0,
                    emit_window_started_at: now,
                    emits_in_window: 0,
                },
            },
        );

        self.users
            .entry(user_id.clone())
            .or_default()
            .insert(conn_id);

        let _ = outbound_tx.try_send(ServerFrame::connected(conn_id.to_string(), user_id.clone()));

        let private_channel = ChannelName(format!("user:{user_id}"));
        if let Some(reason) = self.join_internal(conn_id, private_channel.clone()) {
            self.unregister(conn_id, reason);
            return;
        }
        self.send_frame(
            conn_id,
            ServerFrame::Joined {
                id: uuid::Uuid::new_v4().to_string(),
                channel: private_channel.to_string(),
                ts: Utc::now().timestamp(),
            },
        );
    }

    fn unregister(&mut self, conn_id: ConnectionId, _reason: DisconnectReason) {
        let Some(existing) = self.connections.remove(&conn_id) else {
            return;
        };

        if let Some(user_set) = self.users.get_mut(&existing.meta.user_id) {
            user_set.remove(&conn_id);
            if user_set.is_empty() {
                self.users.remove(&existing.meta.user_id);
            }
        }

        if let Some(joined) = self.connection_channels.remove(&conn_id) {
            for channel in joined {
                if let Some(member_set) = self.channels.get_mut(&channel) {
                    member_set.remove(&conn_id);
                    if member_set.is_empty() {
                        self.channels.remove(&channel);
                    }
                }
            }
        }
    }

    fn handle_join(&mut self, conn_id: ConnectionId, channel: ChannelName, req_id: String) {
        if !self.check_join_rate(conn_id) {
            self.send_frame(
                conn_id,
                ServerFrame::ack_err(req_id, "rate_limited", "Join rate limit exceeded"),
            );
            return;
        }

        let Some(meta) = self.connections.get(&conn_id).map(|conn| conn.meta.clone()) else {
            return;
        };

        if let Err(err) = self.policy.can_join(&meta, &channel) {
            self.send_frame(
                conn_id,
                ServerFrame::ack_err(req_id, "forbidden_channel", err.message()),
            );
            return;
        }

        if self
            .connection_channels
            .get(&conn_id)
            .is_some_and(|set| set.contains(&channel))
        {
            self.send_frame(conn_id, ServerFrame::ack_ok(req_id));
            return;
        }

        if self
            .connection_channels
            .get(&conn_id)
            .map(|set| set.len())
            .unwrap_or(0)
            >= self.config.max_channels_per_connection
        {
            self.send_frame(
                conn_id,
                ServerFrame::ack_err(
                    req_id,
                    "channel_limit_exceeded",
                    "Maximum channels per connection reached",
                ),
            );
            return;
        }

        if let Some(reason) = self.join_internal(conn_id, channel.clone()) {
            self.unregister(conn_id, reason);
            return;
        }

        self.send_frame(conn_id, ServerFrame::ack_ok(req_id));
        self.send_frame(
            conn_id,
            ServerFrame::Joined {
                id: uuid::Uuid::new_v4().to_string(),
                channel: channel.to_string(),
                ts: Utc::now().timestamp(),
            },
        );
    }

    fn handle_leave(&mut self, conn_id: ConnectionId, channel: ChannelName, req_id: String) {
        let was_member = self
            .connection_channels
            .get(&conn_id)
            .is_some_and(|set| set.contains(&channel));
        if !was_member {
            self.send_frame(
                conn_id,
                ServerFrame::ack_err(req_id, "channel_not_joined", "Not a member of channel"),
            );
            return;
        }

        self.leave_internal(conn_id, &channel);
        self.send_frame(conn_id, ServerFrame::ack_ok(req_id));
        self.send_frame(
            conn_id,
            ServerFrame::Left {
                id: uuid::Uuid::new_v4().to_string(),
                channel: channel.to_string(),
                ts: Utc::now().timestamp(),
            },
        );
    }

    fn handle_emit(
        &mut self,
        conn_id: ConnectionId,
        channel: ChannelName,
        event: String,
        payload: Value,
        req_id: String,
    ) {
        if !self.check_emit_rate(conn_id) {
            self.send_frame(
                conn_id,
                ServerFrame::ack_err(req_id, "rate_limited", "Emit rate limit exceeded"),
            );
            return;
        }

        let Some(meta) = self.connections.get(&conn_id).map(|conn| conn.meta.clone()) else {
            return;
        };

        if let Err(err) = self.policy.can_publish(&meta, &channel, &event) {
            self.send_frame(
                conn_id,
                ServerFrame::ack_err(req_id, "forbidden_channel", err.message()),
            );
            return;
        }

        let sender_is_member = self
            .connection_channels
            .get(&conn_id)
            .is_some_and(|set| set.contains(&channel));
        if !sender_is_member {
            self.send_frame(
                conn_id,
                ServerFrame::ack_err(req_id, "channel_not_joined", "Join channel before emitting"),
            );
            return;
        }

        let recipients = self.channels.get(&channel).cloned().unwrap_or_default();
        let include_sender = should_echo_to_sender(&channel);
        let event_frame = ServerFrame::event(
            channel.to_string(),
            event,
            payload,
            Some(meta.user_id.clone()),
        );
        for recipient_id in recipients {
            if recipient_id == conn_id && !include_sender {
                continue;
            }
            self.send_frame(recipient_id, event_frame.clone());
        }

        self.send_frame(conn_id, ServerFrame::ack_ok(req_id));
    }

    fn handle_ping(&mut self, conn_id: ConnectionId, req_id: String) {
        self.send_frame(conn_id, ServerFrame::pong(req_id));
    }

    fn handle_send_to_channel(&mut self, channel: ChannelName, event: String, payload: Value) {
        let Some(conn_ids) = self.channels.get(&channel).cloned() else {
            return;
        };

        let frame = ServerFrame::event(channel.to_string(), event, payload, None);
        for conn_id in conn_ids {
            self.send_frame(conn_id, frame.clone());
        }
    }

    fn handle_send_to_user(&mut self, user_id: String, event: String, payload: Value) {
        let Some(conn_ids) = self.users.get(&user_id).cloned() else {
            return;
        };

        let channel = format!("user:{user_id}");
        let frame = ServerFrame::event(channel, event, payload, None);
        for conn_id in conn_ids {
            self.send_frame(conn_id, frame.clone());
        }
    }

    fn check_join_rate(&mut self, conn_id: ConnectionId) -> bool {
        let Some(state) = self.connections.get_mut(&conn_id) else {
            return false;
        };
        allow_within_window(
            &mut state.rate.join_window_started_at,
            &mut state.rate.joins_in_window,
            self.config.join_rate_per_sec,
        )
    }

    fn check_emit_rate(&mut self, conn_id: ConnectionId) -> bool {
        let Some(state) = self.connections.get_mut(&conn_id) else {
            return false;
        };
        allow_within_window(
            &mut state.rate.emit_window_started_at,
            &mut state.rate.emits_in_window,
            self.config.emit_rate_per_sec,
        )
    }

    fn join_internal(
        &mut self,
        conn_id: ConnectionId,
        channel: ChannelName,
    ) -> Option<DisconnectReason> {
        self.connection_channels
            .entry(conn_id)
            .or_default()
            .insert(channel.clone());
        self.channels.entry(channel).or_default().insert(conn_id);
        None
    }

    fn leave_internal(&mut self, conn_id: ConnectionId, channel: &ChannelName) {
        if let Some(set) = self.connection_channels.get_mut(&conn_id) {
            set.remove(channel);
            if set.is_empty() {
                self.connection_channels.remove(&conn_id);
            }
        }

        if let Some(set) = self.channels.get_mut(channel) {
            set.remove(&conn_id);
            if set.is_empty() {
                self.channels.remove(channel);
            }
        }
    }

    fn send_frame(&mut self, conn_id: ConnectionId, frame: ServerFrame) {
        let Some(outbound_tx) = self
            .connections
            .get(&conn_id)
            .map(|connection| connection.outbound_tx.clone())
        else {
            return;
        };

        match outbound_tx.try_send(frame) {
            Ok(_) => {}
            Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                self.unregister(conn_id, DisconnectReason::SlowConsumer);
            }
            Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                self.unregister(conn_id, DisconnectReason::SocketError);
            }
        }
    }
}

fn allow_within_window(start: &mut Instant, count: &mut u32, max_per_sec: u32) -> bool {
    let now = Instant::now();
    if now.duration_since(*start).as_secs() >= 1 {
        *start = now;
        *count = 0;
    }
    if *count >= max_per_sec {
        return false;
    }
    *count += 1;
    true
}

fn should_echo_to_sender(channel: &ChannelName) -> bool {
    channel.as_str().starts_with("echo:")
}

#[cfg(test)]
mod tests {
    use super::should_echo_to_sender;
    use crate::server::ChannelName;

    #[test]
    fn echo_channel_includes_sender() {
        let channel = ChannelName::parse("echo:room").expect("channel should parse");
        assert!(should_echo_to_sender(&channel));
    }

    #[test]
    fn non_echo_channel_excludes_sender() {
        let channel = ChannelName::parse("public:lobby").expect("channel should parse");
        assert!(!should_echo_to_sender(&channel));
    }
}
