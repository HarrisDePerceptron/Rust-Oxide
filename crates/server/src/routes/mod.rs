pub mod api;
pub mod base_api_router;
pub mod base_router;
pub mod crud_api_router;
mod entry;
pub mod middleware;
pub mod response;
pub mod route_list;
pub mod views;

pub use crud_api_router::{CrudApiRouter, Method};
pub use entry::{API_PREFIX, router};
pub use middleware::{
    AdminRole, AuthGuard, AuthRolGuardLayer, AuthRoleGuard, RequiredRole, UserRole,
    catch_panic_layer, json_error_middleware,
};
pub use response::{ApiResult, JsonApiResponse};
