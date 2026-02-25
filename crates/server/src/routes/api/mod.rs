#[cfg(feature = "auth-local")]
pub mod admin;
#[cfg(feature = "auth-local")]
pub mod auth;
#[cfg(feature = "auth-local")]
pub mod protected;
pub mod public;
#[cfg(feature = "realtime")]
pub mod realtime;
mod router;
#[cfg(feature = "todo-example")]
pub mod todo_crud;

pub use router::router;
