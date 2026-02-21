use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use futures_util::{
    SinkExt, StreamExt,
    stream::{SplitSink, SplitStream},
};
use serde_json::Value;
use tokio::net::TcpStream;
use tokio::{
    sync::{Mutex, mpsc, oneshot},
    time::{interval, timeout},
};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};
use uuid::Uuid;

use crate::protocol::{ClientFrame, DEFAULT_EVENT, ErrorPayload, ServerFrame};

use super::ClientConfig;

pub type ClientResult<T> = std::result::Result<T, String>;
pub type SubscriptionId = u64;
type ChannelHandler = Arc<dyn Fn(Value) + Send + Sync>;
type GlobalHandler = Arc<dyn Fn(String, Value) + Send + Sync>;
type ChannelEventHandler = Arc<dyn Fn(String, Value) + Send + Sync>;
type GlobalEventHandler = Arc<dyn Fn(String, String, Value) + Send + Sync>;
type PendingAcks = Arc<Mutex<HashMap<String, oneshot::Sender<ClientResult<()>>>>>;
type ChannelHandlers =
    Arc<std::sync::Mutex<HashMap<String, HashMap<SubscriptionId, ChannelHandler>>>>;
type GlobalHandlers = Arc<std::sync::Mutex<HashMap<SubscriptionId, GlobalHandler>>>;
type ChannelEventHandlers =
    Arc<std::sync::Mutex<HashMap<String, HashMap<SubscriptionId, ChannelEventHandler>>>>;
type GlobalEventHandlers = Arc<std::sync::Mutex<HashMap<SubscriptionId, GlobalEventHandler>>>;
type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;
type WsWriter = SplitSink<WsStream, Message>;
type WsReader = SplitStream<WsStream>;

#[derive(Clone)]
pub struct RealtimeClient {
    outbound_tx: mpsc::Sender<ClientFrame>,
    pending_acks: PendingAcks,
    channel_handlers: ChannelHandlers,
    global_handlers: GlobalHandlers,
    channel_event_handlers: ChannelEventHandlers,
    global_event_handlers: GlobalEventHandlers,
    next_subscription_id: Arc<AtomicU64>,
    cfg: ClientConfig,
}

impl RealtimeClient {
    pub async fn connect(base_url: &str, token: &str) -> ClientResult<Self> {
        Self::connect_with_config(base_url, token, ClientConfig::default()).await
    }

    pub async fn connect_with_config(
        base_url: &str,
        token: &str,
        cfg: ClientConfig,
    ) -> ClientResult<Self> {
        let ws = Self::open_socket(base_url, token).await?;
        let (write, read) = ws.split();
        let (outbound_tx, outbound_rx) = mpsc::channel::<ClientFrame>(cfg.outbound_buffer);

        let pending_acks: PendingAcks = Arc::new(Mutex::new(HashMap::new()));
        let channel_handlers: ChannelHandlers = Arc::new(std::sync::Mutex::new(HashMap::new()));
        let global_handlers: GlobalHandlers = Arc::new(std::sync::Mutex::new(HashMap::new()));
        let channel_event_handlers: ChannelEventHandlers =
            Arc::new(std::sync::Mutex::new(HashMap::new()));
        let global_event_handlers: GlobalEventHandlers =
            Arc::new(std::sync::Mutex::new(HashMap::new()));

        Self::spawn_writer_task(write, outbound_rx);
        Self::spawn_reader_task(
            read,
            Arc::clone(&pending_acks),
            Arc::clone(&channel_handlers),
            Arc::clone(&global_handlers),
            Arc::clone(&channel_event_handlers),
            Arc::clone(&global_event_handlers),
        );
        Self::spawn_ping_task(outbound_tx.clone(), cfg.ping_interval);

        Ok(Self {
            outbound_tx,
            pending_acks,
            channel_handlers,
            global_handlers,
            channel_event_handlers,
            global_event_handlers,
            next_subscription_id: Arc::new(AtomicU64::new(1)),
            cfg,
        })
    }

    pub async fn join(&self, channel: &str) -> ClientResult<()> {
        self.request_ack(
            ClientFrame::ChannelJoin {
                id: Uuid::new_v4().to_string(),
                channel: channel.to_string(),
                ts: None,
            },
            self.cfg.request_timeout,
        )
        .await
    }

    pub async fn leave(&self, channel: &str) -> ClientResult<()> {
        self.request_ack(
            ClientFrame::ChannelLeave {
                id: Uuid::new_v4().to_string(),
                channel: channel.to_string(),
                ts: None,
            },
            self.cfg.request_timeout,
        )
        .await
    }

    pub async fn send(&self, channel: &str, message: Value) -> ClientResult<()> {
        self.send_event(channel, DEFAULT_EVENT, message).await
    }

    pub async fn send_event(&self, channel: &str, event: &str, message: Value) -> ClientResult<()> {
        self.request_ack(
            ClientFrame::ChannelEmit {
                id: Uuid::new_v4().to_string(),
                channel: channel.to_string(),
                event: event.to_string(),
                data: message,
                ts: None,
            },
            self.cfg.request_timeout,
        )
        .await
    }

    pub fn on_message<F>(&self, channel: &str, handler: F) -> SubscriptionId
    where
        F: Fn(Value) + Send + Sync + 'static,
    {
        let id = self.next_subscription_id.fetch_add(1, Ordering::Relaxed);
        let mut guard = self
            .channel_handlers
            .lock()
            .expect("channel handler mutex poisoned");
        guard
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
        let mut guard = self
            .channel_event_handlers
            .lock()
            .expect("channel event handler mutex poisoned");
        guard
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

    async fn open_socket(base_url: &str, token: &str) -> ClientResult<WsStream> {
        let url = with_query_token(base_url, token);
        let (ws, _) = connect_async(&url)
            .await
            .map_err(|err| format!("failed to connect to {url}: {err}"))?;
        Ok(ws)
    }

    fn spawn_writer_task(mut write: WsWriter, mut outbound_rx: mpsc::Receiver<ClientFrame>) {
        tokio::spawn(async move {
            while let Some(frame) = outbound_rx.recv().await {
                let text = match serde_json::to_string(&frame) {
                    Ok(text) => text,
                    Err(err) => {
                        eprintln!("failed to serialize outbound frame: {err}");
                        continue;
                    }
                };

                if write.send(Message::Text(text.into())).await.is_err() {
                    break;
                }
            }
        });
    }

    fn spawn_reader_task(
        mut read: WsReader,
        pending_acks: PendingAcks,
        channel_handlers: ChannelHandlers,
        global_handlers: GlobalHandlers,
        channel_event_handlers: ChannelEventHandlers,
        global_event_handlers: GlobalEventHandlers,
    ) {
        tokio::spawn(async move {
            while let Some(next) = read.next().await {
                let msg = match next {
                    Ok(msg) => msg,
                    Err(err) => {
                        eprintln!("websocket read error: {err}");
                        break;
                    }
                };

                let keep_reading = Self::handle_incoming_message(
                    msg,
                    &pending_acks,
                    &channel_handlers,
                    &global_handlers,
                    &channel_event_handlers,
                    &global_event_handlers,
                )
                .await;
                if !keep_reading {
                    break;
                }
            }

            Self::fail_pending_acks(&pending_acks).await;
        });
    }

    async fn handle_incoming_message(
        msg: Message,
        pending_acks: &PendingAcks,
        channel_handlers: &ChannelHandlers,
        global_handlers: &GlobalHandlers,
        channel_event_handlers: &ChannelEventHandlers,
        global_event_handlers: &GlobalEventHandlers,
    ) -> bool {
        let text = match msg {
            Message::Text(text) => text,
            Message::Close(_) => return false,
            _ => return true,
        };

        let frame = match serde_json::from_str::<ServerFrame>(&text) {
            Ok(frame) => frame,
            Err(err) => {
                eprintln!("invalid server frame: {err}");
                return true;
            }
        };

        Self::handle_server_frame(
            frame,
            pending_acks,
            channel_handlers,
            global_handlers,
            channel_event_handlers,
            global_event_handlers,
        )
        .await;
        true
    }

    async fn handle_server_frame(
        frame: ServerFrame,
        pending_acks: &PendingAcks,
        channel_handlers: &ChannelHandlers,
        global_handlers: &GlobalHandlers,
        channel_event_handlers: &ChannelEventHandlers,
        global_event_handlers: &GlobalEventHandlers,
    ) {
        match frame {
            ServerFrame::Connected {
                conn_id, user_id, ..
            } => {
                println!("connected: conn_id={conn_id} user_id={user_id}");
            }
            ServerFrame::Joined { channel, .. } => {
                println!("joined channel={channel}");
            }
            ServerFrame::Left { channel, .. } => {
                println!("left channel={channel}");
            }
            ServerFrame::Event {
                channel,
                event,
                data,
                ..
            } => {
                dispatch_channel_handlers(channel_handlers, &channel, &data);
                dispatch_global_handlers(global_handlers, &channel, &data);
                dispatch_channel_event_handlers(channel_event_handlers, &channel, &event, &data);
                dispatch_global_event_handlers(global_event_handlers, &channel, &event, &data);
            }
            ServerFrame::Ack {
                for_id, ok, error, ..
            } => {
                Self::resolve_ack(pending_acks, for_id, ok, error).await;
            }
            ServerFrame::Pong { id, .. } => {
                println!("pong id={id}");
            }
            ServerFrame::Error { error, .. } => {
                eprintln!("server error {}: {}", error.code, error.message);
            }
        }
    }

    async fn resolve_ack(
        pending_acks: &PendingAcks,
        for_id: String,
        ok: bool,
        error: Option<ErrorPayload>,
    ) {
        let Some(tx) = pending_acks.lock().await.remove(&for_id) else {
            return;
        };

        let result = if ok {
            Ok(())
        } else {
            let message = error
                .map(|e| format!("{}: {}", e.code, e.message))
                .unwrap_or_else(|| "request rejected".to_string());
            Err(message)
        };
        let _ = tx.send(result);
    }

    async fn fail_pending_acks(pending_acks: &PendingAcks) {
        let mut pending = pending_acks.lock().await;
        for (_, tx) in pending.drain() {
            let _ = tx.send(Err("websocket connection closed".to_string()));
        }
    }

    fn spawn_ping_task(outbound_tx: mpsc::Sender<ClientFrame>, ping_interval: Duration) {
        tokio::spawn(async move {
            let mut ticker = interval(ping_interval);
            loop {
                ticker.tick().await;
                if outbound_tx
                    .send(ClientFrame::Ping {
                        id: Uuid::new_v4().to_string(),
                        ts: None,
                    })
                    .await
                    .is_err()
                {
                    break;
                }
            }
        });
    }

    async fn request_ack(&self, frame: ClientFrame, timeout_dur: Duration) -> ClientResult<()> {
        let req_id = frame_id(&frame).to_string();
        let (tx, rx) = oneshot::channel();
        self.pending_acks.lock().await.insert(req_id.clone(), tx);

        if let Err(err) = self.outbound_tx.send(frame).await {
            self.pending_acks.lock().await.remove(&req_id);
            return Err(format!("failed to send request: {err}"));
        }

        match timeout(timeout_dur, rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err("ack wait channel dropped".to_string()),
            Err(_) => {
                self.pending_acks.lock().await.remove(&req_id);
                Err(format!("ack timeout for request {req_id}"))
            }
        }
    }
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

fn frame_id(frame: &ClientFrame) -> &str {
    match frame {
        ClientFrame::ChannelJoin { id, .. } => id,
        ClientFrame::ChannelLeave { id, .. } => id,
        ClientFrame::ChannelEmit { id, .. } => id,
        ClientFrame::Ping { id, .. } => id,
    }
}

fn with_query_token(base_url: &str, token: &str) -> String {
    if base_url.contains('?') {
        format!("{base_url}&token={token}")
    } else {
        format!("{base_url}?token={token}")
    }
}
