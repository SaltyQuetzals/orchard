//! Server-rendered owner pages and shared error responses.

use askama::Template;
use axum::{
    extract::State,
    http::StatusCode,
    response::{Html, IntoResponse, Response},
};

use crate::auth::budget_summary;

use super::AppState;
use crate::auth::Session;

#[derive(Template)]
#[template(path = "home.html")]
struct HomePage<'a> {
    budget_name: &'a str,
    currency: &'a str,
    timezone: &'a str,
    csrf_token: &'a str,
}

pub(crate) async fn home(State(state): State<AppState>, session: Session) -> Response {
    match budget_summary(state.store().db()).await {
        Ok(Some(budget)) => render_page(&HomePage {
            budget_name: &budget.name,
            currency: &budget.currency,
            timezone: &budget.timezone,
            csrf_token: &session.csrf_token,
        }),
        Ok(None) => error_page(
            StatusCode::SERVICE_UNAVAILABLE,
            "Installation not initialized",
            "Run the host-access initialize command before signing in.",
            false,
        ),
        Err(_) => internal_error(),
    }
}

#[derive(Template)]
#[template(path = "error.html")]
struct ErrorPage {
    title: &'static str,
    message: &'static str,
    show_login: bool,
}

pub(crate) fn error_page(
    status: StatusCode,
    title: &'static str,
    message: &'static str,
    show_login: bool,
) -> Response {
    match (ErrorPage {
        title,
        message,
        show_login,
    })
    .render()
    {
        Ok(html) => (status, Html(html)).into_response(),
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "Unable to render page.\n").into_response(),
    }
}

pub(crate) fn forbidden() -> Response {
    error_page(
        StatusCode::FORBIDDEN,
        "Request rejected",
        "The request did not pass the browser security checks and was not applied.",
        true,
    )
}

pub(crate) fn bad_request() -> Response {
    error_page(
        StatusCode::BAD_REQUEST,
        "Invalid submission",
        "The form could not be read. Reload the page and try again.",
        true,
    )
}

pub(crate) fn unauthenticated() -> Response {
    error_page(
        StatusCode::UNAUTHORIZED,
        "Sign in required",
        "Your session is missing or has expired.",
        true,
    )
}

pub(crate) fn internal_error() -> Response {
    error_page(
        StatusCode::INTERNAL_SERVER_ERROR,
        "Something went wrong",
        "The request could not be completed. Try again.",
        false,
    )
}

pub(crate) fn render_page<T: Template>(template: &T) -> Response {
    match template.render() {
        Ok(html) => Html(html).into_response(),
        Err(_) => internal_error(),
    }
}
