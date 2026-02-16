use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, RwLock},
};

use uuid::Uuid;

use crate::error::AppError;

const CHAT_ROOM_PREFIX: &str = "chat:room:";
const MAX_ROOM_NAME_LEN: usize = 64;

#[derive(Clone, Default)]
pub struct ChatRoomRegistry {
    inner: Arc<RwLock<ChatRoomsState>>,
}

#[derive(Default)]
struct ChatRoomsState {
    rooms_by_name: HashMap<String, RoomRecord>,
    channel_to_room: HashMap<String, String>,
    user_to_room: HashMap<String, String>,
}

struct RoomRecord {
    display_name: String,
    channel: String,
    members: HashSet<String>,
}

#[derive(Debug, Clone)]
pub struct ChatRoomJoin {
    pub room_name: String,
    pub channel: String,
    pub member_count: usize,
    pub switched_from: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ChatRoomLeave {
    pub room_name: String,
    pub channel: String,
    pub member_count: usize,
}

impl ChatRoomRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn join_room(&self, user_id: &str, room_name: &str) -> Result<ChatRoomJoin, AppError> {
        let (room_key, display_name) = normalize_room_name(room_name)?;
        let mut state = self
            .inner
            .write()
            .map_err(|_| AppError::internal("chat room registry lock poisoned"))?;

        let mut switched_from = None;
        if let Some(existing_room_key) = state.user_to_room.get(user_id).cloned() {
            if existing_room_key != room_key {
                if let Some(previous_room) = state.rooms_by_name.get_mut(&existing_room_key) {
                    previous_room.members.remove(user_id);
                    switched_from = Some(previous_room.display_name.clone());
                    if previous_room.members.is_empty() {
                        let removed = state.rooms_by_name.remove(&existing_room_key);
                        if let Some(removed) = removed {
                            state.channel_to_room.remove(&removed.channel);
                        }
                    }
                }
            } else if let Some(room) = state.rooms_by_name.get(&room_key) {
                if room.members.contains(user_id) {
                    return Ok(ChatRoomJoin {
                        room_name: room.display_name.clone(),
                        channel: room.channel.clone(),
                        member_count: room.members.len(),
                        switched_from: None,
                    });
                }
            }
        }

        let room = state
            .rooms_by_name
            .entry(room_key.clone())
            .or_insert_with(|| RoomRecord {
                display_name: display_name.clone(),
                channel: make_chat_room_channel(),
                members: HashSet::new(),
            });

        if !room.members.contains(user_id) && room.members.len() >= 2 {
            return Err(AppError::conflict("Room already has two participants"));
        }

        room.members.insert(user_id.to_string());
        let channel = room.channel.clone();
        let room_name = room.display_name.clone();
        let member_count = room.members.len();

        state
            .channel_to_room
            .insert(channel.clone(), room_key.clone());
        state.user_to_room.insert(user_id.to_string(), room_key);

        Ok(ChatRoomJoin {
            room_name,
            channel,
            member_count,
            switched_from,
        })
    }

    pub fn leave_room(&self, user_id: &str, room_name: &str) -> Result<ChatRoomLeave, AppError> {
        let (room_key, _) = normalize_room_name(room_name)?;
        let mut state = self
            .inner
            .write()
            .map_err(|_| AppError::internal("chat room registry lock poisoned"))?;

        let (room_name, channel, member_count) = {
            let room = state
                .rooms_by_name
                .get_mut(&room_key)
                .ok_or_else(|| AppError::not_found("Room not found"))?;

            if !room.members.remove(user_id) {
                return Err(AppError::not_found("User is not a member of this room"));
            }

            (
                room.display_name.clone(),
                room.channel.clone(),
                room.members.len(),
            )
        };

        if state
            .user_to_room
            .get(user_id)
            .is_some_and(|current| current == &room_key)
        {
            state.user_to_room.remove(user_id);
        }

        if member_count == 0 {
            state.rooms_by_name.remove(&room_key);
            state.channel_to_room.remove(&channel);
        }

        Ok(ChatRoomLeave {
            room_name,
            channel,
            member_count,
        })
    }

    pub fn user_can_access_channel(&self, user_id: &str, channel: &str) -> bool {
        if !is_chat_room_channel(channel) {
            return false;
        }

        let Ok(state) = self.inner.read() else {
            return false;
        };
        let Some(room_key) = state.channel_to_room.get(channel) else {
            return false;
        };
        let Some(room) = state.rooms_by_name.get(room_key) else {
            return false;
        };
        room.members.contains(user_id)
    }
}

pub struct AppChannelPolicy {
    chat_rooms: ChatRoomRegistry,
}

impl AppChannelPolicy {
    pub fn new(chat_rooms: ChatRoomRegistry) -> Self {
        Self { chat_rooms }
    }
}

impl Clone for AppChannelPolicy {
    fn clone(&self) -> Self {
        Self {
            chat_rooms: self.chat_rooms.clone(),
        }
    }
}

impl realtime::server::ChannelPolicy for AppChannelPolicy {
    fn can_join(
        &self,
        meta: &realtime::server::ConnectionMeta,
        channel: &realtime::server::ChannelName,
    ) -> Result<(), realtime::server::RealtimeError> {
        if is_chat_room_channel(channel.as_str()) {
            if self
                .chat_rooms
                .user_can_access_channel(&meta.user_id, channel.as_str())
            {
                return Ok(());
            }
            return Err(realtime::server::RealtimeError::forbidden(
                "Join room via /api/v1/chat/rooms/join before subscribing",
            ));
        }
        realtime::server::ChannelPolicy::can_join(
            &realtime::server::DefaultChannelPolicy,
            meta,
            channel,
        )
    }

    fn can_publish(
        &self,
        meta: &realtime::server::ConnectionMeta,
        channel: &realtime::server::ChannelName,
        event: &str,
    ) -> Result<(), realtime::server::RealtimeError> {
        if is_chat_room_channel(channel.as_str()) {
            if event.trim().is_empty() {
                return Err(realtime::server::RealtimeError::bad_request(
                    "Event name is required",
                ));
            }
            if self
                .chat_rooms
                .user_can_access_channel(&meta.user_id, channel.as_str())
            {
                return Ok(());
            }
            return Err(realtime::server::RealtimeError::forbidden(
                "Join room via /api/v1/chat/rooms/join before publishing",
            ));
        }
        realtime::server::ChannelPolicy::can_publish(
            &realtime::server::DefaultChannelPolicy,
            meta,
            channel,
            event,
        )
    }
}

fn normalize_room_name(raw: &str) -> Result<(String, String), AppError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(AppError::bad_request("room_name is required"));
    }
    if trimmed.len() > MAX_ROOM_NAME_LEN {
        return Err(AppError::bad_request("room_name is too long"));
    }
    if !trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '_' | '-' | '.'))
    {
        return Err(AppError::bad_request("room_name has invalid characters"));
    }

    let display_name = collapse_whitespace(trimmed);
    let room_key = display_name.to_ascii_lowercase();
    Ok((room_key, display_name))
}

fn collapse_whitespace(input: &str) -> String {
    input.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn make_chat_room_channel() -> String {
    format!("{CHAT_ROOM_PREFIX}{}", Uuid::new_v4().simple())
}

fn is_chat_room_channel(channel: &str) -> bool {
    channel.starts_with(CHAT_ROOM_PREFIX)
}

#[cfg(test)]
mod tests {
    use realtime::server::{ChannelName, ChannelPolicy, ConnectionId, ConnectionMeta};

    use super::*;

    fn meta(user_id: &str) -> ConnectionMeta {
        ConnectionMeta {
            id: ConnectionId::new(),
            user_id: user_id.to_string(),
            roles: vec!["user".to_string()],
            joined_at_unix: 0,
        }
    }

    #[test]
    fn room_allows_only_two_participants() {
        let rooms = ChatRoomRegistry::new();
        let join1 = rooms.join_room("u1", "Demo Room").expect("u1 joins");
        let join2 = rooms.join_room("u2", "Demo Room").expect("u2 joins");
        let err = rooms
            .join_room("u3", "Demo Room")
            .expect_err("third participant should be rejected");

        assert_eq!(join1.channel, join2.channel);
        assert_eq!(err.message(), "Room already has two participants");
    }

    #[test]
    fn policy_rejects_chat_room_join_without_registry_membership() {
        let rooms = ChatRoomRegistry::new();
        let policy = AppChannelPolicy::new(rooms);
        let channel = ChannelName::parse("chat:room:abc").expect("channel should parse");
        let err = policy
            .can_join(&meta("u1"), &channel)
            .expect_err("join should be forbidden");

        assert_eq!(
            err.message(),
            "Join room via /api/v1/chat/rooms/join before subscribing"
        );
    }

    #[test]
    fn policy_allows_join_after_registry_membership() {
        let rooms = ChatRoomRegistry::new();
        let joined = rooms
            .join_room("u1", "Demo Room")
            .expect("join should succeed");
        let policy = AppChannelPolicy::new(rooms);
        let channel = ChannelName::parse(&joined.channel).expect("channel should parse");

        policy
            .can_join(&meta("u1"), &channel)
            .expect("member should be allowed");
    }
}
