use std::collections::BTreeMap;

use askama::Template;
use axum::{Router, http::StatusCode, response::Html, routing::get};
use chrono::Local;
use tower_http::services::ServeDir;

use crate::routes::route_list::{RouteInfo, routes};
use crate::response::{ApiResult, JsonApiResponse};
use crate::db::entity_catalog::{self, EntityInfo};

#[derive(Clone)]
struct RouteItem {
    method: String,
    path: String,
    request: String,
    response: String,
    required_headers: String,
    curl: String,
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
    project_name: String,
}

#[derive(Template)]
#[template(path = "routes.html")]
struct RoutesTemplate {
    now: String,
    route_groups: Vec<RouteGroup>,
    project_name: String,
}

#[derive(Template)]
#[template(path = "entities.html")]
struct EntitiesTemplate {
    now: String,
    entities: &'static [EntityInfo],
    erd_mermaid: &'static str,
    project_name: String,
}

#[derive(Template)]
#[template(path = "docs.html")]
struct DocsTemplate {
    now: String,
    project_name: String,
}

type HtmlError = (StatusCode, Html<String>);

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

async fn handler() -> ApiResult<serde_json::Value> {
    JsonApiResponse::ok(serde_json::json!({ "ok": true, "route": "public" }))
}

async fn list_routes_json() -> ApiResult<&'static [RouteInfo]> {
    JsonApiResponse::ok(routes())
}

async fn index() -> Result<Html<String>, HtmlError> {
    let now = Local::now().to_rfc3339();
    let project_name = project_name();
    let rendered = IndexTemplate {
        now,
        project_name,
    }
        .render()
        .map_err(|_| html_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to render index"))?;
    Ok(Html(rendered))
}

async fn routes_view() -> Result<Html<String>, HtmlError> {
    let now = Local::now().to_rfc3339();
    let route_groups = build_route_groups();
    let project_name = project_name();
    let rendered = RoutesTemplate {
        now,
        route_groups,
        project_name,
    }
        .render()
        .map_err(|_| html_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to render routes"))?;
    Ok(Html(rendered))
}

async fn entities_view() -> Result<Html<String>, HtmlError> {
    let now = Local::now().to_rfc3339();
    let entities = entity_catalog::entities();
    let erd_mermaid = entity_catalog::erd_mermaid();
    let project_name = project_name();
    let rendered = EntitiesTemplate {
        now,
        entities,
        erd_mermaid,
        project_name,
    }
        .render()
        .map_err(|_| {
            html_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to render entities")
        })?;
    Ok(Html(rendered))
}

async fn docs_view() -> Result<Html<String>, HtmlError> {
    let now = Local::now().to_rfc3339();
    let project_name = project_name();
    let rendered = DocsTemplate { now, project_name }
        .render()
        .map_err(|_| html_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to render docs"))?;
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
                required_headers: route.required_headers.to_string(),
                curl: route.curl.to_string(),
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

pub(crate) fn project_name() -> String {
    let raw = env!("CARGO_PKG_NAME");
    let mut words = Vec::new();
    let mut current = String::new();
    for ch in raw.chars() {
        if ch == '_' || ch == '-' {
            if !current.is_empty() {
                words.push(current.clone());
                current.clear();
            }
        } else {
            current.push(ch);
        }
    }
    if !current.is_empty() {
        words.push(current);
    }

    let mut out = String::new();
    for (idx, word) in words.into_iter().enumerate() {
        if idx > 0 {
            out.push(' ');
        }
        let mut chars = word.chars();
        if let Some(first) = chars.next() {
            out.push(first.to_ascii_uppercase());
            for ch in chars {
                out.push(ch.to_ascii_lowercase());
            }
        }
    }
    if out.is_empty() {
        "Project".to_string()
    } else {
        out
    }
}

fn html_error(status: StatusCode, message: &'static str) -> HtmlError {
    (status, Html(message.to_string()))
}
