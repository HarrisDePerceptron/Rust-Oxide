use std::collections::BTreeMap;

use askama::Template;
use axum::{Json, Router, http::StatusCode, response::Html, routing::get};
use chrono::Local;
use tower_http::services::ServeDir;

use crate::error::AppError;
use crate::routes::route_list::{RouteInfo, routes};
use crate::db::entity_catalog::{self, EntityInfo};

#[derive(Clone)]
struct RouteItem {
    method: String,
    path: String,
    request: String,
    response: String,
}

#[derive(Clone)]
struct RouteGroup {
    source: String,
    routes: Vec<RouteItem>,
    route_count: usize,
}

#[derive(Template)]
#[template(path = "index.html")]
struct IndexTemplate {
    now: String,
    route_groups: Vec<RouteGroup>,
}

#[derive(Template)]
#[template(path = "routes.html")]
struct RoutesTemplate {
    now: String,
    route_groups: Vec<RouteGroup>,
}

#[derive(Template)]
#[template(path = "entities.html")]
struct EntitiesTemplate {
    now: String,
    entities: &'static [EntityInfo],
    erd_mermaid: &'static str,
}

#[derive(Template)]
#[template(path = "docs.html")]
struct DocsTemplate {
    now: String,
}

pub fn router() -> Router {
    let public_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("public");
    Router::new()
        .route_service("/{*file}", ServeDir::new(public_dir))
        .route("/", get(index))
        .route("/public", get(handler))
        .route("/entities", get(entities_view))
        .route("/docs", get(docs_view))
        .route("/routes", get(routes_view))
        .route("/routes.json", get(list_routes_json))
}

async fn handler() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "ok": true, "route": "public" }))
}

async fn list_routes_json() -> Json<&'static [RouteInfo]> {
    Json(routes())
}

async fn index() -> Result<Html<String>, AppError> {
    let now = Local::now().to_rfc3339();
    let route_groups = build_route_groups();
    let rendered = IndexTemplate { now, route_groups }
        .render()
        .map_err(|_| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, "failed to render index"))?;
    Ok(Html(rendered))
}

async fn routes_view() -> Result<Html<String>, AppError> {
    let now = Local::now().to_rfc3339();
    let route_groups = build_route_groups();
    let rendered = RoutesTemplate { now, route_groups }
        .render()
        .map_err(|_| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, "failed to render routes"))?;
    Ok(Html(rendered))
}

async fn entities_view() -> Result<Html<String>, AppError> {
    let now = Local::now().to_rfc3339();
    let entities = entity_catalog::entities();
    let erd_mermaid = entity_catalog::erd_mermaid();
    let rendered = EntitiesTemplate {
        now,
        entities,
        erd_mermaid,
    }
        .render()
        .map_err(|_| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, "failed to render entities"))?;
    Ok(Html(rendered))
}

async fn docs_view() -> Result<Html<String>, AppError> {
    let now = Local::now().to_rfc3339();
    let rendered = DocsTemplate { now }
        .render()
        .map_err(|_| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, "failed to render docs"))?;
    Ok(Html(rendered))
}

fn build_route_groups() -> Vec<RouteGroup> {
    let mut grouped: BTreeMap<String, Vec<RouteItem>> = BTreeMap::new();
    for route in routes() {
        grouped
            .entry(route.source.to_string())
            .or_default()
            .push(RouteItem {
                method: route.method.to_string(),
                path: route.path.to_string(),
                request: route.request.to_string(),
                response: route.response.to_string(),
            });
    }

    let mut route_groups: Vec<RouteGroup> = grouped
        .into_iter()
        .map(|(source, mut routes)| {
            routes.sort_by(|a, b| a.path.cmp(&b.path).then(a.method.cmp(&b.method)));
            let route_count = routes.len();
            RouteGroup {
                source,
                routes,
                route_count,
            }
        })
        .collect();
    route_groups.sort_by(|a, b| a.source.cmp(&b.source));
    route_groups
}
