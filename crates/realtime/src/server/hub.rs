use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
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
const INBOUND_QUEUE_SIZE: usize = 4096;

pub type SubscriptionId = u64;
type ChannelHandler = Arc<dyn Fn(Value) + Send + Sync>;
type GlobalHandler = Arc<dyn Fn(String, Value) + Send + Sync>;
type ChannelEventHandler = Arc<dyn Fn(String, Value) + Send + Sync>;
type GlobalEventHandler = Arc<dyn Fn(String, String, Value) + Send + Sync>;
type ChannelHandlers =
    Arc<std::sync::Mutex<HashMap<String, HashMap<SubscriptionId, ChannelHandler>>>>;
type GlobalHandlers = Arc<std::sync::Mutex<HashMap<SubscriptionId, GlobalHandler>>>;
type ChannelEventHandlers =
    Arc<std::sync::Mutex<HashMap<String, HashMap<SubscriptionId, ChannelEventHandler>>>>;
type GlobalEventHandlers = Arc<std::sync::Mutex<HashMap<SubscriptionId, GlobalEventHandler>>>;

#[derive(Clone)]
pub(crate) struct InboundMessage {
    pub channel: String,
    pub event: String,
    pub payload: Value,
}

#[derive(Clone)]
pub struct RealtimeHandle {
    config: RealtimeConfig,
    tx: Option<mpsc::Sender<HubCommand>>,
    channel_handlers: ChannelHandlers,
    global_handlers: GlobalHandlers,
    channel_event_handlers: ChannelEventHandlers,
    global_event_handlers: GlobalEventHandlers,
    next_subscription_id: Arc<AtomicU64>,
}

impl RealtimeHandle {
    pub fn spawn(config: RealtimeConfig) -> Self {
        Self::spawn_with_policy(config, Arc::new(DefaultChannelPolicy))
    }

    pub fn spawn_with_policy(config: RealtimeConfig, policy: Arc<dyn ChannelPolicy>) -> Self {
        let channel_handlers: ChannelHandlers = Arc::new(std::sync::Mutex::new(HashMap::new()));
        let global_handlers: GlobalHandlers = Arc::new(std::sync::Mutex::new(HashMap::new()));
        let channel_event_handlers: ChannelEventHandlers =
            Arc::new(std::sync::Mutex::new(HashMap::new()));
        let global_event_handlers: GlobalEventHandlers =
            Arc::new(std::sync::Mutex::new(HashMap::new()));
        let next_subscription_id = Arc::new(AtomicU64::new(1));

        if !config.enabled {
            return Self {
                config,
                tx: None,
                channel_handlers,
                global_handlers,
                channel_event_handlers,
                global_event_handlers,
                next_subscription_id,
            };
        }

        let (tx, rx) = mpsc::channel(HUB_QUEUE_SIZE);
        let (inbound_tx, inbound_rx) = mpsc::channel(INBOUND_QUEUE_SIZE);
        let mut hub = RealtimeHub::new(config.clone(), rx, policy, Some(inbound_tx));
        tokio::spawn(async move {
            hub.run().await;
        });
        spawn_inbound_dispatcher(
            inbound_rx,
            Arc::clone(&channel_handlers),
            Arc::clone(&global_handlers),
            Arc::clone(&channel_event_handlers),
            Arc::clone(&global_event_handlers),
        );

        Self {
            config,
            tx: Some(tx),
            channel_handlers,
            global_handlers,
            channel_event_handlers,
            global_event_handlers,
            next_subscription_id,
        }
    }

    pub fn disabled(config: RealtimeConfig) -> Self {
        Self {
            config,
            tx: None,
            channel_handlers: Arc::new(std::sync::Mutex::new(HashMap::new())),
            global_handlers: Arc::new(std::sync::Mutex::new(HashMap::new())),
            channel_event_handlers: Arc::new(std::sync::Mutex::new(HashMap::new())),
            global_event_handlers: Arc::new(std::sync::Mutex::new(HashMap::new())),
            next_subscription_id: Arc::new(AtomicU64::new(1)),
        }
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

    pub fn on_message<F>(&self, channel: &str, handler: F) -> SubscriptionId
    where
        F: Fn(Value) + Send + Sync + 'static,
    {
        let id = self.next_subscription_id.fetch_add(1, Ordering::Relaxed);
        let mut handlers = self
            .channel_handlers
            .lock()
            .expect("channel handler mutex poisoned");
        handlers
            .entry(channel.to_string())
            .or_default()
            .insert(id, Arc::new(handler));
        id
    }

    pub fn on_messages<F>(&self, handler: F) -> SubscriptionId
    where
        F: Fn(String, Value) + Send + Sync + 'static,
    {
        let id = self.next_subscription_id.fetch_add(1, Ordering::Relaxed);
        self.global_handlers
            .lock()
            .expect("global handler mutex poisoned")
            .insert(id, Arc::new(handler));
        id
    }

    pub fn on_channel_event<F>(&self, channel: &str, handler: F) -> SubscriptionId
    where
        F: Fn(String, Value) + Send + Sync + 'static,
    {
        let id = self.next_subscription_id.fetch_add(1, Ordering::Relaxed);
        let mut handlers = self
            .channel_event_handlers
            .lock()
            .expect("channel event handler mutex poisoned");
        handlers
            .entry(channel.to_string())
            .or_default()
            .insert(id, Arc::new(handler));
        id
    }

    pub fn on_events<F>(&self, handler: F) -> SubscriptionId
    where
        F: Fn(String, String, Value) + Send + Sync + 'static,
    {
        let id = self.next_subscription_id.fetch_add(1, Ordering::Relaxed);
        self.global_event_handlers
            .lock()
            .expect("global event handler mutex poisoned")
            .insert(id, Arc::new(handler));
        id
    }

    pub fn off(&self, id: SubscriptionId) -> bool {
        let mut removed = false;

        let mut global = self
            .global_handlers
            .lock()
            .expect("global handler mutex poisoned");
        if global.remove(&id).is_some() {
            removed = true;
        }
        drop(global);

        let mut channels = self
            .channel_handlers
            .lock()
            .expect("channel handler mutex poisoned");
        for handlers in channels.values_mut() {
            if handlers.remove(&id).is_some() {
                removed = true;
            }
        }

        let mut global_events = self
            .global_event_handlers
            .lock()
            .expect("global event handler mutex poisoned");
        if global_events.remove(&id).is_some() {
            removed = true;
        }
        drop(global_events);

        let mut channel_events = self
            .channel_event_handlers
            .lock()
            .expect("channel event handler mutex poisoned");
        for handlers in channel_events.values_mut() {
            if handlers.remove(&id).is_some() {
                removed = true;
            }
        }

        removed
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
    inbound_tx: Option<mpsc::Sender<InboundMessage>>,
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
        inbound_tx: Option<mpsc::Sender<InboundMessage>>,
    ) -> Self {
        Self {
            config,
            rx,
            policy,
            inbound_tx,
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

    fn unregister(&mut self, conn_id: ConnectionId, reason: DisconnectReason) {
        let Some(existing) = self.connections.remove(&conn_id) else {
            return;
        };

        tracing::debug!(
            conn_id = %conn_id,
            user_id = %existing.meta.user_id,
            reason = ?reason,
            "realtime connection disconnected"
        );

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
        tracing::debug!(
            conn_id = %conn_id,
            channel = %channel,
            req_id = %req_id,
            "realtime join requested"
        );

        if !self.check_join_rate(conn_id) {
            tracing::debug!(
                conn_id = %conn_id,
                channel = %channel,
                req_id = %req_id,
                "realtime join denied: rate limited"
            );
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
            tracing::debug!(
                conn_id = %conn_id,
                user_id = %meta.user_id,
                channel = %channel,
                req_id = %req_id,
                reason = %err,
                "realtime join denied by policy"
            );
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
            tracing::debug!(
                conn_id = %conn_id,
                channel = %channel,
                req_id = %req_id,
                "realtime join acknowledged: already joined"
            );
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
            tracing::debug!(
                conn_id = %conn_id,
                channel = %channel,
                req_id = %req_id,
                "realtime join denied: channel limit exceeded"
            );
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
            tracing::debug!(
                conn_id = %conn_id,
                channel = %channel,
                reason = ?reason,
                "realtime join caused disconnect"
            );
            self.unregister(conn_id, reason);
            return;
        }

        tracing::debug!(
            conn_id = %conn_id,
            user_id = %meta.user_id,
            channel = %channel,
            req_id = %req_id,
            "realtime join succeeded"
        );

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
        tracing::debug!(
            conn_id = %conn_id,
            channel = %channel,
            req_id = %req_id,
            "realtime leave requested"
        );
        let was_member = self
            .connection_channels
            .get(&conn_id)
            .is_some_and(|set| set.contains(&channel));
        if !was_member {
            tracing::debug!(
                conn_id = %conn_id,
                channel = %channel,
                req_id = %req_id,
                "realtime leave denied: not joined"
            );
            self.send_frame(
                conn_id,
                ServerFrame::ack_err(req_id, "channel_not_joined", "Not a member of channel"),
            );
            return;
        }

        self.leave_internal(conn_id, &channel);
        tracing::debug!(
            conn_id = %conn_id,
            channel = %channel,
            req_id = %req_id,
            "realtime leave succeeded"
        );
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
        self.publish_inbound(InboundMessage {
            channel: channel.to_string(),
            event: event.clone(),
            payload: payload.clone(),
        });
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

    fn publish_inbound(&mut self, message: InboundMessage) {
        let Some(tx) = &self.inbound_tx else {
            return;
        };

        match tx.try_send(message) {
            Ok(_) => {}
            Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                tracing::debug!("realtime inbound dispatch queue is full; dropping message");
            }
            Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                self.inbound_tx = None;
            }
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

fn spawn_inbound_dispatcher(
    mut inbound_rx: mpsc::Receiver<InboundMessage>,
    channel_handlers: ChannelHandlers,
    global_handlers: GlobalHandlers,
    channel_event_handlers: ChannelEventHandlers,
    global_event_handlers: GlobalEventHandlers,
) {
    tokio::spawn(async move {
        while let Some(message) = inbound_rx.recv().await {
            dispatch_channel_handlers(&channel_handlers, &message.channel, &message.payload);
            dispatch_global_handlers(&global_handlers, &message.channel, &message.payload);
            dispatch_channel_event_handlers(
                &channel_event_handlers,
                &message.channel,
                &message.event,
                &message.payload,
            );
            dispatch_global_event_handlers(
                &global_event_handlers,
                &message.channel,
                &message.event,
                &message.payload,
            );
        }
    });
}

fn dispatch_channel_handlers(handlers: &ChannelHandlers, channel: &str, message: &Value) {
    let callbacks: Vec<ChannelHandler> = {
        let guard = handlers.lock().expect("channel handler mutex poisoned");
        guard
            .get(channel)
            .map(|entries| entries.values().cloned().collect())
            .unwrap_or_default()
    };

    for callback in callbacks {
        callback(message.clone());
    }
}

fn dispatch_global_handlers(handlers: &GlobalHandlers, channel: &str, message: &Value) {
    let callbacks: Vec<GlobalHandler> = {
        let guard = handlers.lock().expect("global handler mutex poisoned");
        guard.values().cloned().collect()
    };

    for callback in callbacks {
        callback(channel.to_string(), message.clone());
    }
}

fn dispatch_channel_event_handlers(
    handlers: &ChannelEventHandlers,
    channel: &str,
    event: &str,
    message: &Value,
) {
    let callbacks: Vec<ChannelEventHandler> = {
        let guard = handlers
            .lock()
            .expect("channel event handler mutex poisoned");
        guard
            .get(channel)
            .map(|entries| entries.values().cloned().collect())
            .unwrap_or_default()
    };

    for callback in callbacks {
        callback(event.to_string(), message.clone());
    }
}

fn dispatch_global_event_handlers(
    handlers: &GlobalEventHandlers,
    channel: &str,
    event: &str,
    message: &Value,
) {
    let callbacks: Vec<GlobalEventHandler> = {
        let guard = handlers
            .lock()
            .expect("global event handler mutex poisoned");
        guard.values().cloned().collect()
    };

    for callback in callbacks {
        callback(channel.to_string(), event.to_string(), message.clone());
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use serde_json::json;

    use super::{
        ChannelEventHandlers, ChannelHandlers, GlobalEventHandlers, GlobalHandlers,
        dispatch_channel_event_handlers, dispatch_channel_handlers, dispatch_global_event_handlers,
        dispatch_global_handlers, should_echo_to_sender,
    };
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

    #[test]
    fn dispatch_channel_handlers_only_targets_matching_channel() {
        let handlers: ChannelHandlers = Arc::new(std::sync::Mutex::new(HashMap::new()));
        let count = Arc::new(AtomicUsize::new(0));
        let count_for_handler = Arc::clone(&count);
        handlers
            .lock()
            .expect("channel handlers lock")
            .entry("chat:room:1".to_string())
            .or_default()
            .insert(
                1,
                Arc::new(move |_| {
                    count_for_handler.fetch_add(1, Ordering::Relaxed);
                }),
            );

        dispatch_channel_handlers(&handlers, "chat:room:1", &json!({"text":"hello"}));
        dispatch_channel_handlers(&handlers, "chat:room:2", &json!({"text":"hello"}));

        assert_eq!(count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn dispatch_global_handlers_receives_channel_and_message() {
        let handlers: GlobalHandlers = Arc::new(std::sync::Mutex::new(HashMap::new()));
        let count = Arc::new(AtomicUsize::new(0));
        let count_for_handler = Arc::clone(&count);
        handlers.lock().expect("global handlers lock").insert(
            1,
            Arc::new(move |channel, payload| {
                assert_eq!(channel, "chat:room:1");
                assert_eq!(payload["text"], "hello");
                count_for_handler.fetch_add(1, Ordering::Relaxed);
            }),
        );

        dispatch_global_handlers(&handlers, "chat:room:1", &json!({"text":"hello"}));

        assert_eq!(count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn dispatch_channel_event_handlers_receives_event_name() {
        let handlers: ChannelEventHandlers = Arc::new(std::sync::Mutex::new(HashMap::new()));
        let count = Arc::new(AtomicUsize::new(0));
        let count_for_handler = Arc::clone(&count);
        handlers
            .lock()
            .expect("channel event handlers lock")
            .entry("chat:room:1".to_string())
            .or_default()
            .insert(
                1,
                Arc::new(move |event, payload| {
                    assert_eq!(event, "chat.typing");
                    assert_eq!(payload["typing"], true);
                    count_for_handler.fetch_add(1, Ordering::Relaxed);
                }),
            );

        dispatch_channel_event_handlers(
            &handlers,
            "chat:room:1",
            "chat.typing",
            &json!({"typing": true}),
        );

        assert_eq!(count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn dispatch_global_event_handlers_receives_channel_event_and_message() {
        let handlers: GlobalEventHandlers = Arc::new(std::sync::Mutex::new(HashMap::new()));
        let count = Arc::new(AtomicUsize::new(0));
        let count_for_handler = Arc::clone(&count);
        handlers.lock().expect("global event handlers lock").insert(
            1,
            Arc::new(move |channel, event, payload| {
                assert_eq!(channel, "chat:room:1");
                assert_eq!(event, "chat.message");
                assert_eq!(payload["text"], "hello");
                count_for_handler.fetch_add(1, Ordering::Relaxed);
            }),
        );

        dispatch_global_event_handlers(
            &handlers,
            "chat:room:1",
            "chat.message",
            &json!({"text":"hello"}),
        );

        assert_eq!(count.load(Ordering::Relaxed), 1);
    }
}
