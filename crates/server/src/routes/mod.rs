pub mod api;
pub mod base_api_router;
pub mod base_router;
mod entry;
pub mod route_list;
pub mod views;

pub use entry::{API_PREFIX, router};
