#[cfg(debug_assertions)]
use std::collections::BTreeMap;
use std::path::PathBuf;

use askama::Template;
#[cfg(not(debug_assertions))]
use axum::response::Redirect;
use axum::{Router, http::StatusCode, response::Html, routing::get};
use chrono::Local;
use tower_http::services::ServeDir;

#[cfg(debug_assertions)]
use crate::db::entity_catalog::{self, EntityInfo};
#[cfg(debug_assertions)]
use crate::routes::route_list::routes;
#[cfg(debug_assertions)]
include!(concat!(env!("OUT_DIR"), "/docs_sections_generated.rs"));

#[cfg(debug_assertions)]
#[derive(Clone)]
struct RouteItem {
    method: String,
    path: String,
    request: String,
    response: String,
    required_headers: String,
    curl: String,
}

#[cfg(debug_assertions)]
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

#[cfg(debug_assertions)]
#[derive(Template)]
#[template(path = "routes.html")]
struct RoutesTemplate {
    now: String,
    route_groups: Vec<RouteGroup>,
    project_name: String,
}

#[cfg(debug_assertions)]
#[derive(Template)]
#[template(path = "entities.html")]
struct EntitiesTemplate {
    now: String,
    entities: &'static [EntityInfo],
    erd_mermaid: &'static str,
    project_name: String,
}

#[cfg(debug_assertions)]
#[derive(Template)]
#[template(path = "docs.html")]
struct DocsTemplate {
    now: String,
    project_name: String,
    sections_html: String,
}

#[derive(Template)]
#[template(path = "not_available.html")]
struct NotAvailableTemplate {
    now: String,
    project_name: String,
}

type HtmlError = (StatusCode, Html<String>);

pub fn router() -> Router {
    let public_dir = resolve_public_dir();
    let router = Router::new()
        .route("/", get(index))
        .route("/not-available", get(not_available_view));

    #[cfg(debug_assertions)]
    let router = router
        .route("/docs", get(docs_view))
        .route("/entities", get(entities_view))
        .route("/routes", get(routes_view));

    #[cfg(not(debug_assertions))]
    let router = router
        .route("/docs", get(not_available_redirect))
        .route("/entities", get(not_available_redirect))
        .route("/routes", get(not_available_redirect));

    router.route_service("/{*file}", ServeDir::new(public_dir))
}

fn resolve_public_dir() -> PathBuf {
    if let Some(path) = std::env::var_os("APP_PUBLIC_DIR") {
        return PathBuf::from(path);
    }

    if let Ok(current_dir) = std::env::current_dir() {
        let candidate = current_dir.join("public");
        if candidate.exists() {
            return candidate;
        }
    }

    if let Ok(exe_path) = std::env::current_exe()
        && let Some(exe_dir) = exe_path.parent()
    {
        let candidate = exe_dir.join("public");
        if candidate.exists() {
            return candidate;
        }
    }

    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("public")
}

async fn index() -> Result<Html<String>, HtmlError> {
    let now = Local::now().to_rfc3339();
    let project_name = project_name();
    let rendered = IndexTemplate { now, project_name }
        .render()
        .map_err(|_| html_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to render index"))?;
    Ok(Html(rendered))
}

#[cfg(debug_assertions)]
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

#[cfg(debug_assertions)]
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
        html_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to render entities",
        )
    })?;
    Ok(Html(rendered))
}

#[cfg(debug_assertions)]
async fn docs_view() -> Result<Html<String>, HtmlError> {
    let now = Local::now().to_rfc3339();
    let project_name = project_name();
    let sections_html = DOCS_SECTIONS_HTML.to_string();
    let rendered = DocsTemplate {
        now,
        project_name,
        sections_html,
    }
    .render()
    .map_err(|_| html_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to render docs"))?;
    Ok(Html(rendered))
}

async fn not_available_view() -> Result<Html<String>, HtmlError> {
    let now = Local::now().to_rfc3339();
    let project_name = project_name();
    let rendered = NotAvailableTemplate { now, project_name }
        .render()
        .map_err(|_| {
            html_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to render availability page",
            )
        })?;
    Ok(Html(rendered))
}

#[cfg(not(debug_assertions))]
async fn not_available_redirect() -> Redirect {
    Redirect::to("/not-available")
}

#[cfg(debug_assertions)]
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
