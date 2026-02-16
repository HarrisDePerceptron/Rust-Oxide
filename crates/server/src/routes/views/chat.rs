#[cfg(debug_assertions)]
use askama::Template;
#[cfg(not(debug_assertions))]
use axum::response::Redirect;
use axum::{Router, routing::get};
#[cfg(debug_assertions)]
use axum::{http::StatusCode, response::Html};

#[cfg(debug_assertions)]
#[derive(Template)]
#[template(path = "chat.html")]
struct ChatUiTemplate {
    now: String,
    project_name: String,
    show_docs_link: bool,
    show_debug_links: bool,
}

#[cfg(debug_assertions)]
type HtmlError = (StatusCode, Html<String>);

pub fn router() -> Router {
    let router = Router::new();
    #[cfg(debug_assertions)]
    let router = router.route("/chat/ui", get(chat_ui));
    #[cfg(not(debug_assertions))]
    let router = router.route("/chat/ui", get(chat_ui_unavailable));
    router
}

#[cfg(debug_assertions)]
async fn chat_ui() -> Result<Html<String>, HtmlError> {
    let now = crate::routes::views::public::formatted_build_time();
    let project_name = crate::routes::views::public::project_name();
    let rendered = ChatUiTemplate {
        now,
        project_name,
        show_docs_link: true,
        show_debug_links: true,
    }
    .render()
    .map_err(|_| {
        html_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to render chat ui",
        )
    })?;
    Ok(Html(rendered))
}

#[cfg(not(debug_assertions))]
async fn chat_ui_unavailable() -> Redirect {
    Redirect::to("/not-available")
}

#[cfg(debug_assertions)]
fn html_error(status: StatusCode, message: &'static str) -> HtmlError {
    (status, Html(message.to_string()))
}
