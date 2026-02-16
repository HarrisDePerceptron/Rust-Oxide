use std::time::Duration;

use axum::extract::ws::{Message, WebSocket};
use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio::time::{Instant, MissedTickBehavior, interval};

use crate::protocol::{ClientFrame, ServerFrame};

use super::{
    RealtimeConfig, SessionAuth,
    hub::HubCommand,
    types::{ChannelName, ConnectionId, ConnectionMeta, DisconnectReason},
};

pub async fn run_socket_session(
    socket: WebSocket,
    auth: SessionAuth,
    hub_tx: mpsc::Sender<HubCommand>,
    cfg: RealtimeConfig,
) {
    let conn_id = ConnectionId::new();
    let (outbound_tx, mut outbound_rx) = mpsc::channel(cfg.outbound_queue_size);

    let meta = ConnectionMeta {
        id: conn_id,
        user_id: auth.user_id,
        roles: auth.roles,
        joined_at_unix: Utc::now().timestamp(),
    };

    if hub_tx
        .send(HubCommand::Register { meta, outbound_tx })
        .await
        .is_err()
    {
        return;
    }

    let (mut ws_sender, mut ws_receiver) = socket.split();
    let mut heartbeat = interval(Duration::from_secs(cfg.heartbeat_interval_secs));
    heartbeat.set_missed_tick_behavior(MissedTickBehavior::Delay);

    let idle_timeout = Duration::from_secs(cfg.idle_timeout_secs);
    let mut last_activity = Instant::now();

    let disconnect_reason = loop {
        tokio::select! {
            outbound = outbound_rx.recv() => {
                let Some(frame) = outbound else {
                    break DisconnectReason::HubUnavailable;
                };

                let Ok(payload) = serde_json::to_string(&frame) else {
                    break DisconnectReason::ProtocolError;
                };

                if ws_sender.send(Message::Text(payload.into())).await.is_err() {
                    break DisconnectReason::SocketError;
                }
            }
            incoming = ws_receiver.next() => {
                let Some(incoming) = incoming else {
                    break DisconnectReason::ClientClosed;
                };

                match incoming {
                    Ok(Message::Text(text)) => {
                        last_activity = Instant::now();
                        if text.len() > cfg.max_message_bytes {
                            let _ = send_direct_error(
                                &mut ws_sender,
                                "message_too_large",
                                "Message exceeds realtime.max_message_bytes",
                            ).await;
                            continue;
                        }

                        let frame = match serde_json::from_str::<ClientFrame>(&text) {
                            Ok(frame) => frame,
                            Err(_) => {
                                let _ = send_direct_error(
                                    &mut ws_sender,
                                    "invalid_payload",
                                    "Invalid websocket payload",
                                )
                                .await;
                                continue;
                            }
                        };

                        if dispatch_client_frame(conn_id, frame, &hub_tx, &mut ws_sender)
                            .await
                            .is_err()
                        {
                            break DisconnectReason::HubUnavailable;
                        }
                    }
                    Ok(Message::Binary(_)) => {
                        let _ = send_direct_error(
                            &mut ws_sender,
                            "invalid_payload",
                            "Binary websocket payloads are not supported",
                        )
                        .await;
                    }
                    Ok(Message::Ping(payload)) => {
                        last_activity = Instant::now();
                        if ws_sender.send(Message::Pong(payload)).await.is_err() {
                            break DisconnectReason::SocketError;
                        }
                    }
                    Ok(Message::Pong(_)) => {
                        last_activity = Instant::now();
                    }
                    Ok(Message::Close(_)) => {
                        break DisconnectReason::ClientClosed;
                    }
                    Err(_) => {
                        break DisconnectReason::SocketError;
                    }
                }
            }
            _ = heartbeat.tick() => {
                if last_activity.elapsed() > idle_timeout {
                    break DisconnectReason::IdleTimeout;
                }

                if ws_sender
                    .send(Message::Ping(Vec::new().into()))
                    .await
                    .is_err()
                {
                    break DisconnectReason::SocketError;
                }
            }
        }
    };

    let _ = hub_tx
        .send(HubCommand::Unregister {
            conn_id,
            reason: disconnect_reason,
        })
        .await;
}

async fn dispatch_client_frame(
    conn_id: ConnectionId,
    frame: ClientFrame,
    hub_tx: &mpsc::Sender<HubCommand>,
    ws_sender: &mut futures_util::stream::SplitSink<WebSocket, Message>,
) -> Result<(), ()> {
    let command = match frame {
        ClientFrame::ChannelJoin { id, channel, .. } => {
            let channel = match ChannelName::parse(&channel) {
                Ok(channel) => channel,
                Err(err) => {
                    let message = err.message().to_string();
                    let _ = send_direct_error(ws_sender, "invalid_channel", &message).await;
                    return Ok(());
                }
            };
            HubCommand::Join {
                conn_id,
                channel,
                req_id: id,
            }
        }
        ClientFrame::ChannelLeave { id, channel, .. } => {
            let channel = match ChannelName::parse(&channel) {
                Ok(channel) => channel,
                Err(err) => {
                    let message = err.message().to_string();
                    let _ = send_direct_error(ws_sender, "invalid_channel", &message).await;
                    return Ok(());
                }
            };
            HubCommand::Leave {
                conn_id,
                channel,
                req_id: id,
            }
        }
        ClientFrame::ChannelEmit {
            id,
            channel,
            event,
            data,
            ..
        } => {
            let channel = match ChannelName::parse(&channel) {
                Ok(channel) => channel,
                Err(err) => {
                    let message = err.message().to_string();
                    let _ = send_direct_error(ws_sender, "invalid_channel", &message).await;
                    return Ok(());
                }
            };
            HubCommand::Emit {
                conn_id,
                channel,
                event,
                payload: data,
                req_id: id,
            }
        }
        ClientFrame::Ping { id, .. } => HubCommand::Ping {
            conn_id,
            req_id: id,
        },
    };

    hub_tx.send(command).await.map_err(|_| ())
}

async fn send_direct_error(
    ws_sender: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    code: &str,
    message: &str,
) -> Result<(), ()> {
    let frame = ServerFrame::error(code, message);
    let payload = serde_json::to_string(&frame).map_err(|_| ())?;
    ws_sender
        .send(Message::Text(payload.into()))
        .await
        .map_err(|_| ())
}
