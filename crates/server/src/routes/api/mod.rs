pub mod admin;
pub mod auth;
pub mod protected;
pub mod public;
pub mod realtime;
mod router;
pub mod todo_crud;

pub use router::router;
