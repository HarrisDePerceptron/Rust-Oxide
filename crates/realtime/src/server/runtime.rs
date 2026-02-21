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
    pub fn new<V>(handle: RealtimeHandle, verifier: V) -> Self
    where
        V: RealtimeTokenVerifier,
    {
        Self {
            handle,
            verifier: Arc::new(verifier),
        }
    }

    pub fn new_with_shared_verifier(
        handle: RealtimeHandle,
        verifier: Arc<dyn RealtimeTokenVerifier>,
    ) -> Self {
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

    pub fn on_channel_event<F>(&self, channel: &str, handler: F) -> SubscriptionId
    where
        F: Fn(String, Value) + Send + Sync + 'static,
    {
        self.handle.on_channel_event(channel, handler)
    }

    pub fn on_events<F>(&self, handler: F) -> SubscriptionId
    where
        F: Fn(String, String, Value) + Send + Sync + 'static,
    {
        self.handle.on_events(handler)
    }

    pub fn off(&self, id: SubscriptionId) -> bool {
        self.handle.off(id)
    }
}
