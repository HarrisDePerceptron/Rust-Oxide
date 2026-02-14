#[cfg(debug_assertions)]
use std::collections::BTreeMap;
use std::{path::PathBuf, sync::Arc};

use askama::Template;
#[cfg(not(debug_assertions))]
use axum::response::Redirect;
use axum::{Router, extract::State, http::StatusCode, response::Html, routing::get};
use chrono::Local;
use tower_http::services::ServeDir;

#[cfg(debug_assertions)]
use crate::db::entity_catalog::{self, EntityInfo};
#[cfg(debug_assertions)]
use crate::routes::route_list::routes;
use crate::state::AppState;

include!(concat!(env!("OUT_DIR"), "/docs_sections_generated.rs"));

#[derive(Clone, Copy)]
struct NavVisibility {
    show_docs_link: bool,
    show_debug_links: bool,
}

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
    show_docs_link: bool,
    show_debug_links: bool,
}

#[cfg(debug_assertions)]
#[derive(Template)]
#[template(path = "routes.html")]
struct RoutesTemplate {
    now: String,
    route_groups: Vec<RouteGroup>,
    project_name: String,
    show_docs_link: bool,
    show_debug_links: bool,
}

#[cfg(debug_assertions)]
#[derive(Template)]
#[template(path = "entities.html")]
struct EntitiesTemplate {
    now: String,
    entities: &'static [EntityInfo],
    erd_mermaid: &'static str,
    project_name: String,
    show_docs_link: bool,
    show_debug_links: bool,
}

#[derive(Template)]
#[template(path = "docs.html")]
struct DocsTemplate {
    now: String,
    project_name: String,
    sections_html: String,
    show_docs_link: bool,
    show_debug_links: bool,
}

#[derive(Template)]
#[template(path = "not_available.html")]
struct NotAvailableTemplate {
    now: String,
    project_name: String,
    show_docs_link: bool,
    show_debug_links: bool,
}

type HtmlError = (StatusCode, Html<String>);

pub fn router(state: Arc<AppState>) -> Router {
    let public_dir = resolve_public_dir();
    #[cfg(not(debug_assertions))]
    let docs_enabled = docs_enabled(state.as_ref());

    let router = Router::new()
        .route("/", get(index))
        .route("/not-available", get(not_available_view));

    #[cfg(debug_assertions)]
    let router = router.route("/docs", get(docs_view));

    #[cfg(not(debug_assertions))]
    let router = if docs_enabled {
        router.route("/docs", get(docs_view))
    } else {
        router.route("/docs", get(not_available_redirect))
    };

    #[cfg(debug_assertions)]
    let router = router
        .route("/entities", get(entities_view))
        .route("/routes", get(routes_view));

    #[cfg(not(debug_assertions))]
    let router = router
        .route("/entities", get(not_available_redirect))
        .route("/routes", get(not_available_redirect));

    router
        .route_service("/{*file}", ServeDir::new(public_dir))
        .with_state(state)
}

fn resolve_public_dir() -> PathBuf {
    if let Some(path) = std::env::var_os("RUST_OXIDE_PUBLIC_DIR") {
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

fn docs_enabled(state: &AppState) -> bool {
    cfg!(debug_assertions) || state.config.general.enable_docs_in_release
}

fn nav_visibility(state: &AppState) -> NavVisibility {
    NavVisibility {
        show_docs_link: docs_enabled(state),
        show_debug_links: cfg!(debug_assertions),
    }
}

async fn index(State(state): State<Arc<AppState>>) -> Result<Html<String>, HtmlError> {
    let now = formatted_build_time();
    let project_name = project_name();
    let nav = nav_visibility(state.as_ref());
    let rendered = IndexTemplate {
        now,
        project_name,
        show_docs_link: nav.show_docs_link,
        show_debug_links: nav.show_debug_links,
    }
    .render()
    .map_err(|_| html_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to render index"))?;
    Ok(Html(rendered))
}

#[cfg(debug_assertions)]
async fn routes_view(State(state): State<Arc<AppState>>) -> Result<Html<String>, HtmlError> {
    let now = formatted_build_time();
    let route_groups = build_route_groups();
    let project_name = project_name();
    let nav = nav_visibility(state.as_ref());
    let rendered = RoutesTemplate {
        now,
        route_groups,
        project_name,
        show_docs_link: nav.show_docs_link,
        show_debug_links: nav.show_debug_links,
    }
    .render()
    .map_err(|_| html_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to render routes"))?;
    Ok(Html(rendered))
}

#[cfg(debug_assertions)]
async fn entities_view(State(state): State<Arc<AppState>>) -> Result<Html<String>, HtmlError> {
    let now = formatted_build_time();
    let entities = entity_catalog::entities();
    let erd_mermaid = entity_catalog::erd_mermaid();
    let project_name = project_name();
    let nav = nav_visibility(state.as_ref());
    let rendered = EntitiesTemplate {
        now,
        entities,
        erd_mermaid,
        project_name,
        show_docs_link: nav.show_docs_link,
        show_debug_links: nav.show_debug_links,
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

async fn docs_view(State(state): State<Arc<AppState>>) -> Result<Html<String>, HtmlError> {
    let now = formatted_build_time();
    let project_name = project_name();
    let sections_html = DOCS_SECTIONS_HTML.to_string();
    let nav = nav_visibility(state.as_ref());
    let rendered = DocsTemplate {
        now,
        project_name,
        sections_html,
        show_docs_link: nav.show_docs_link,
        show_debug_links: nav.show_debug_links,
    }
    .render()
    .map_err(|_| html_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to render docs"))?;
    Ok(Html(rendered))
}

async fn not_available_view(State(state): State<Arc<AppState>>) -> Result<Html<String>, HtmlError> {
    let now = formatted_build_time();
    let project_name = project_name();
    let nav = nav_visibility(state.as_ref());
    let rendered = NotAvailableTemplate {
        now,
        project_name,
        show_docs_link: nav.show_docs_link,
        show_debug_links: nav.show_debug_links,
    }
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

pub(crate) fn formatted_build_time() -> String {
    Local::now().format("%d-%m-%Y %H:%M").to_string()
}

fn html_error(status: StatusCode, message: &'static str) -> HtmlError {
    (status, Html(message.to_string()))
}
