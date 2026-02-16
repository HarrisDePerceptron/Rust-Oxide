use std::sync::Arc;

use axum::{Extension, Json, Router, routing::post};
use serde::{Deserialize, Serialize};

use crate::{
    realtime::{ChatRoomRegistry, RealtimeHandle},
    routes::{ApiResult, AuthGuard, JsonApiResponse},
    services::chat_room_service::ChatRoomService,
    state::AppState,
};

#[derive(Debug, Deserialize)]
pub struct JoinRoomRequest {
    pub room_name: String,
}

#[derive(Debug, Serialize)]
pub struct JoinRoomResponse {
    pub room_name: String,
    pub channel: String,
    pub member_count: usize,
    pub switched_from: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct LeaveRoomRequest {
    pub room_name: String,
}

#[derive(Debug, Serialize)]
pub struct LeaveRoomResponse {
    pub room_name: String,
    pub channel: String,
    pub member_count: usize,
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/chat/rooms/join", post(join_room))
        .route("/chat/rooms/leave", post(leave_room))
        .with_state(state)
}

async fn join_room(
    claims: AuthGuard,
    Extension(chat_rooms): Extension<ChatRoomRegistry>,
    Extension(realtime): Extension<RealtimeHandle>,
    Json(body): Json<JoinRoomRequest>,
) -> ApiResult<JoinRoomResponse> {
    let service = ChatRoomService::new(chat_rooms, realtime);
    let joined = service.join_room(&claims.sub, &body.room_name).await?;

    JsonApiResponse::ok(JoinRoomResponse {
        room_name: joined.room_name,
        channel: joined.channel,
        member_count: joined.member_count,
        switched_from: joined.switched_from,
    })
}

async fn leave_room(
    claims: AuthGuard,
    Extension(chat_rooms): Extension<ChatRoomRegistry>,
    Extension(realtime): Extension<RealtimeHandle>,
    Json(body): Json<LeaveRoomRequest>,
) -> ApiResult<LeaveRoomResponse> {
    let service = ChatRoomService::new(chat_rooms, realtime);
    let left = service.leave_room(&claims.sub, &body.room_name).await?;

    JsonApiResponse::ok(LeaveRoomResponse {
        room_name: left.room_name,
        channel: left.channel,
        member_count: left.member_count,
    })
}
