#[cfg(debug_assertions)]
use askama::Template;
#[cfg(not(debug_assertions))]
use axum::response::Redirect;
use axum::{Router, routing::get};
#[cfg(debug_assertions)]
use axum::{http::StatusCode, response::Html};
#[cfg(debug_assertions)]
use chrono::Local;

#[cfg(debug_assertions)]
#[derive(Template)]
#[template(path = "todo.html")]
struct TodoUiTemplate {
    now: String,
    project_name: String,
}

#[cfg(debug_assertions)]
type HtmlError = (StatusCode, Html<String>);

pub fn router() -> Router {
    let router = Router::new();
    #[cfg(debug_assertions)]
    let router = router.route("/todo/ui", get(todo_ui));
    #[cfg(not(debug_assertions))]
    let router = router.route("/todo/ui", get(todo_ui_unavailable));
    router
}

#[cfg(debug_assertions)]
async fn todo_ui() -> Result<Html<String>, HtmlError> {
    let now = Local::now().to_rfc3339();
    let project_name = crate::routes::views::public::project_name();
    let rendered = TodoUiTemplate { now, project_name }.render().map_err(|_| {
        html_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to render todo ui",
        )
    })?;
    Ok(Html(rendered))
}

#[cfg(not(debug_assertions))]
async fn todo_ui_unavailable() -> Redirect {
    Redirect::to("/not-available")
}

#[cfg(debug_assertions)]
fn html_error(status: StatusCode, message: &'static str) -> HtmlError {
    (status, Html(message.to_string()))
}
