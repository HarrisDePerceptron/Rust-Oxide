use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use super::{RealtimeError, RealtimeHandle, SessionAuth, SubscriptionId};

#[async_trait]
pub trait RealtimeTokenVerifier: Send + Sync + 'static {
    async fn verify_token(&self, token: &str) -> Result<SessionAuth, RealtimeError>;
}

#[derive(Clone)]
pub struct RealtimeRuntimeState {
    pub handle: RealtimeHandle,
    pub verifier: Arc<dyn RealtimeTokenVerifier>,
}

impl RealtimeRuntimeState {
    pub fn new(handle: RealtimeHandle, verifier: Arc<dyn RealtimeTokenVerifier>) -> Self {
        Self { handle, verifier }
    }

    pub async fn send(
        &self,
        channel_name: impl Into<String>,
        message: Value,
    ) -> Result<(), RealtimeError> {
        self.handle.send(channel_name, message).await
    }

    pub async fn send_to_user(
        &self,
        user_id: impl Into<String>,
        message: Value,
    ) -> Result<(), RealtimeError> {
        self.handle.send_to_user(user_id, message).await
    }

    pub fn on_message<F>(&self, channel: &str, handler: F) -> SubscriptionId
    where
        F: Fn(Value) + Send + Sync + 'static,
    {
        self.handle.on_message(channel, handler)
    }

    pub fn on_messages<F>(&self, handler: F) -> SubscriptionId
    where
        F: Fn(String, Value) + Send + Sync + 'static,
    {
        self.handle.on_messages(handler)
    }

    pub fn off(&self, id: SubscriptionId) -> bool {
        self.handle.off(id)
    }
}
