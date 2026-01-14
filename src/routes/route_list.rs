use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize)]
pub struct RouteInfo {
    pub method: &'static str,
    pub path: &'static str,
    pub source: &'static str,
    pub request: &'static str,
    pub response: &'static str,
}

include!(concat!(env!("OUT_DIR"), "/routes_generated.rs"));

pub fn routes() -> &'static [RouteInfo] {
    ROUTES
}
