use std::sync::Arc;

use async_trait::async_trait;

use super::{RealtimeError, RealtimeHandle, SessionAuth};

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
}
