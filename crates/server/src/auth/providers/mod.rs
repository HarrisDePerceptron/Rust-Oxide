#[cfg(feature = "auth-local")]
pub mod local;
mod registry;

#[cfg(feature = "auth-local")]
pub use local::LocalAuthProvider;
pub use registry::{AuthProvider, AuthProviderId, AuthProviders};
