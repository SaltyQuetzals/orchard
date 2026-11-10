//! Authenticated HTTP delivery: public health/static assets, the login
//! boundary, and protected owner pages.

mod auth;
mod origin;
mod pages;

use std::net::IpAddr;
use std::sync::Arc;

use askama::Template;
use axum::{
    Router,
    http::{StatusCode, header},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use tokio::sync::Semaphore;

use crate::db::Store;

pub use crate::auth::Session;
pub use auth::AppError;
pub use origin::client_key;

/// Deployment-level browser security and reverse-proxy settings.
#[derive(Debug, Clone)]
pub struct WebConfig {
    /// Public HTTPS origin, for example `https://budget.example.com`.
    pub public_url: Option<String>,
    /// Proxy addresses whose forwarded client/scheme headers are trusted.
    pub trusted_proxies: Vec<IpAddr>,
    /// Whether session cookies carry the `Secure` attribute.
    pub secure_cookies: bool,
}

impl Default for WebConfig {
    fn default() -> Self {
        Self {
            public_url: None,
            trusted_proxies: Vec::new(),
            secure_cookies: true,
        }
    }
}

/// Shared request state for the single-owner application.
#[derive(Clone)]
pub struct AppState {
    store: Arc<Store>,
    config: Arc<WebConfig>,
    login_permits: Arc<Semaphore>,
}

impl AppState {
    pub fn new(store: Arc<Store>, config: WebConfig) -> Self {
        Self {
            store,
            config: Arc::new(config),
            // Bound concurrent Argon2 work independently of HTTP concurrency.
            login_permits: Arc::new(Semaphore::new(2)),
        }
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    pub fn config(&self) -> &WebConfig {
        &self.config
    }

    pub(crate) fn login_permits(&self) -> &Semaphore {
        &self.login_permits
    }
}

/// Builds the application router. Every route except health, static CSS, and
/// the login boundary is wrapped by the session middleware.
pub fn router(state: AppState) -> Router {
    let protected = Router::new()
        .route("/", get(pages::home))
        .route("/logout", post(auth::logout))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_session,
        ));

    Router::new()
        .route("/health", get(health))
        .route("/static/app.css", get(stylesheet))
        .route("/login", get(auth::login_page).post(auth::login_submit))
        .merge(protected)
        .fallback(not_found)
        .layer(middleware::from_fn_with_state(state.clone(), origin::enforce_origin))
        .layer(middleware::from_fn(security_headers))
        .with_state(state)
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
        [
            (header::CONTENT_TYPE, "text/css; charset=utf-8"),
            (header::CACHE_CONTROL, "public, max-age=3600"),
        ],
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

/// Adds non-sensitive security headers and defaults every response to
/// `no-store` unless a handler explicitly chose a public cache policy.
async fn security_headers(request: axum::extract::Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        header::HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::X_FRAME_OPTIONS,
        header::HeaderValue::from_static("DENY"),
    );
    // Chromium serializes the Origin header of form POSTs as `null` when the
    // document uses `no-referrer`, which would make every legitimate login fail
    // the same-origin check. `same-origin` keeps referrers local and preserves
    // the browser's real Origin header.
    headers.insert(
        header::REFERRER_POLICY,
        header::HeaderValue::from_static("same-origin"),
    );
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        header::HeaderValue::from_static(
            "default-src 'none'; style-src 'self'; img-src 'self'; form-action 'self'; base-uri 'none'; frame-ancestors 'none'",
        ),
    );
    if !headers.contains_key(header::CACHE_CONTROL) {
        headers.insert(
            header::CACHE_CONTROL,
            header::HeaderValue::from_static("no-store"),
        );
    }
    response
}
