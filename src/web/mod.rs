use askama::Template;
use axum::{
    Router,
    http::{StatusCode, header},
    response::{Html, IntoResponse, Response},
    routing::get,
};

pub(crate) fn router() -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/static/app.css", get(stylesheet))
        .fallback(not_found)
}

async fn health() -> impl IntoResponse {
    (
        [(header::CACHE_CONTROL, "no-store")],
        "ok
",
    )
}

async fn stylesheet() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
        include_str!("../../static/app.css"),
    )
}

#[derive(Template)]
#[template(path = "not_found.html")]
struct NotFoundPage;

async fn not_found() -> Response {
    match NotFoundPage.render() {
        Ok(html) => (StatusCode::NOT_FOUND, Html(html)).into_response(),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Unable to render page.
",
        )
            .into_response(),
    }
}
