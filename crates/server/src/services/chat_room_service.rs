use serde_json::json;

use crate::{
    error::AppError,
    realtime::{ChatRoomJoin, ChatRoomLeave, ChatRoomRegistry, RealtimeHandle},
};

#[derive(Clone)]
pub struct ChatRoomService {
    rooms: ChatRoomRegistry,
    realtime: RealtimeHandle,
}

impl ChatRoomService {
    pub fn new(rooms: ChatRoomRegistry, realtime: RealtimeHandle) -> Self {
        Self { rooms, realtime }
    }

    pub async fn join_room(
        &self,
        user_id: &str,
        room_name: &str,
    ) -> Result<ChatRoomJoin, AppError> {
        let joined = self.rooms.join_room(user_id, room_name)?;

        self.realtime
            .send_event(
                joined.channel.clone(),
                "chat.presence",
                json!({
                    "room_name": joined.room_name,
                    "user_id": user_id,
                    "action": "joined",
                    "member_count": joined.member_count,
                }),
            )
            .await?;

        Ok(joined)
    }

    pub async fn leave_room(
        &self,
        user_id: &str,
        room_name: &str,
    ) -> Result<ChatRoomLeave, AppError> {
        let left = self.rooms.leave_room(user_id, room_name)?;

        self.realtime
            .send_event(
                left.channel.clone(),
                "chat.presence",
                json!({
                    "room_name": left.room_name,
                    "user_id": user_id,
                    "action": "left",
                    "member_count": left.member_count,
                }),
            )
            .await?;

        Ok(left)
    }
}
