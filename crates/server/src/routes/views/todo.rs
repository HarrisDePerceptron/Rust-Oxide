use askama::Template;
use axum::{Router, http::StatusCode, response::Html, routing::get};
use chrono::Local;

#[derive(Template)]
#[template(path = "todo.html")]
struct TodoUiTemplate {
    now: String,
    project_name: String,
}

type HtmlError = (StatusCode, Html<String>);

pub fn router() -> Router {
    Router::new().route("/todo/ui", get(todo_ui))
}

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

fn html_error(status: StatusCode, message: &'static str) -> HtmlError {
    (status, Html(message.to_string()))
}
