pub mod local;
mod registry;

pub use local::LocalAuthProvider;
pub use registry::{AuthProvider, AuthProviderId, AuthProviders};
