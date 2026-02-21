use std::sync::Arc;

use async_trait::async_trait;

use super::{RealtimeError, SessionAuth, SocketServerHandle};

#[async_trait]
pub trait RealtimeTokenVerifier: Send + Sync + 'static {
    /// Verify an access token and return authenticated session context.
    ///
    /// Returning `Err(RealtimeError::unauthorized(...))` denies websocket upgrade.
    async fn verify_token(&self, token: &str) -> Result<SessionAuth, RealtimeError>;
}

/// Shared application state for realtime HTTP integration.
///
/// This bundles:
/// - `handle`: transport runtime and messaging API.
/// - `verifier`: token verification used during websocket upgrade.
///
/// Use `state.handle` for send/subscribe operations.
#[derive(Clone)]
pub struct SocketAppState {
    pub handle: SocketServerHandle,
    pub verifier: Arc<dyn RealtimeTokenVerifier>,
}

impl SocketAppState {
    /// Create runtime state from a realtime handle and concrete verifier.
    pub fn new<V>(handle: SocketServerHandle, verifier: V) -> Self
    where
        V: RealtimeTokenVerifier,
    {
        Self {
            handle,
            verifier: Arc::new(verifier),
        }
    }

    /// Create runtime state from a realtime handle and shared verifier trait object.
    pub fn new_with_shared_verifier(
        handle: SocketServerHandle,
        verifier: Arc<dyn RealtimeTokenVerifier>,
    ) -> Self {
        Self { handle, verifier }
    }
}
