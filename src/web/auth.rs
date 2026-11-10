//! Browser authentication boundary: login, logout, protected sessions, and
//! session-bound CSRF validation.

use std::net::SocketAddr;

use askama::Template;
use axum::{
    extract::{
        ConnectInfo, Form, FromRequest, FromRequestParts, Query, Request, State,
        rejection::FormRejection,
    },
    http::{
        HeaderMap, HeaderValue, StatusCode,
        header::{LOCATION, RETRY_AFTER, SET_COOKIE},
        request::Parts,
    },
    middleware::Next,
    response::{Html, IntoResponse, Response},
};
use serde::Deserialize;

use crate::auth::{
    self, SESSION_TTL_SECONDS, Session, clear_login_failures, create_session, delete_session,
    hash_token, is_token_shape, load_session, new_session, now_unix, owner_record,
    record_login_failure, retry_after, tokens_match, verify_off_thread,
};

use super::{AppState, client_key, pages};

const SESSION_COOKIE_SECURE: &str = "__Host-orchard_session";
const LOGIN_CSRF_COOKIE_SECURE: &str = "__Host-orchard_login_csrf";
const SESSION_COOKIE_LOCAL: &str = "orchard_session";
const LOGIN_CSRF_COOKIE_LOCAL: &str = "orchard_login_csrf";
const LOGIN_CSRF_TTL_SECONDS: i64 = 10 * 60;
const MAX_RETURN_LINK_BYTES: usize = 2048;

#[derive(Debug, Deserialize)]
pub(crate) struct LoginQuery {
    next: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct LoginForm {
    csrf_token: String,
    next: String,
    password: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct LogoutForm {
    csrf_token: String,
}

/// A form that carried its session's CSRF token.
pub(crate) struct CsrfForm<T> {
    pub session: Session,
    pub form: T,
}

pub(crate) trait CsrfProtected {
    fn csrf_token(&self) -> &str;
}

impl CsrfProtected for LogoutForm {
    fn csrf_token(&self) -> &str {
        &self.csrf_token
    }
}

#[derive(Template)]
#[template(path = "login.html")]
struct LoginPage<'a> {
    csrf_token: &'a str,
    next: &'a str,
    error: Option<&'a str>,
}

/// Session extractor for handlers behind the protected middleware.
impl FromRequestParts<AppState> for Session {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<Session>()
            .cloned()
            .ok_or(AppError::Unauthenticated)
    }
}

/// Parses a form only after checking the authenticated session's CSRF token.
impl<T> FromRequest<AppState> for CsrfForm<T>
where
    T: CsrfProtected + for<'de> Deserialize<'de> + Send,
{
    type Rejection = AppError;

    async fn from_request(request: Request, state: &AppState) -> Result<Self, Self::Rejection> {
        let session = request
            .extensions()
            .get::<Session>()
            .cloned()
            .ok_or(AppError::Unauthenticated)?;
        let Form(form) = Form::<T>::from_request(request, state)
            .await
            .map_err(|_| AppError::BadRequest)?;
        if !session.accepts_csrf(form.csrf_token()) {
            return Err(AppError::Forbidden);
        }
        Ok(Self { session, form })
    }
}

pub enum AppError {
    Unauthenticated,
    BadRequest,
    Forbidden,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        match self {
            Self::Unauthenticated => pages::unauthenticated(),
            Self::BadRequest => pages::bad_request(),
            Self::Forbidden => pages::forbidden(),
        }
    }
}

/// Rejects requests without a live session before any protected handler runs.
pub(crate) async fn require_session(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    let session = match current_session(&state, request.headers()).await {
        Ok(Some(session)) => session,
        Ok(None) => return redirect_to_login(&request),
        Err(_) => return pages::internal_error(),
    };
    request.extensions_mut().insert(session);
    next.run(request).await
}

pub(crate) async fn login_page(
    State(state): State<AppState>,
    Query(query): Query<LoginQuery>,
    headers: HeaderMap,
) -> Response {
    let next = sanitized_next(query.next.as_deref());

    match current_session(&state, &headers).await {
        Ok(Some(_)) => return redirect(&next),
        Ok(None) => {}
        Err(_) => return pages::internal_error(),
    }

    let existing = cookie_value(&headers, login_csrf_cookie_name(&state)).filter(|value| is_token_shape(value));
    let (csrf_token, issue_cookie) = match existing {
        Some(token) => (token, false),
        None => match auth::generate_token() {
            Ok(token) => (token, true),
            Err(_) => return pages::internal_error(),
        },
    };
    let error = login_error(query.error.as_deref());

    let mut response = login_page_response(&csrf_token, &next, error, StatusCode::OK, None);
    if issue_cookie {
        append_cookie(
            &mut response,
            &cookie(
                &state,
                login_csrf_cookie_name(&state),
                &csrf_token,
                Some(LOGIN_CSRF_TTL_SECONDS),
            ),
        );
    }
    response
}

pub(crate) async fn login_submit(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    form: Result<Form<LoginForm>, FormRejection>,
) -> Response {
    let Ok(Form(form)) = form else {
        return pages::bad_request();
    };
    let next = sanitized_next(Some(&form.next));

    let Some(login_csrf) = cookie_value(&headers, login_csrf_cookie_name(&state)) else {
        return pages::forbidden();
    };
    if !is_token_shape(&login_csrf) || !tokens_match(&login_csrf, &form.csrf_token) {
        return pages::forbidden();
    }

    let client = client_key(&state, peer, &headers);
    let now = now_unix();
    match retry_after(state.store().db(), &client, now).await {
        Ok(Some(seconds)) => {
            return login_page_response(
                &login_csrf,
                &next,
                Some("Too many failed attempts. Try again later."),
                StatusCode::TOO_MANY_REQUESTS,
                Some(seconds),
            );
        }
        Ok(None) => {}
        Err(_) => return pages::internal_error(),
    }

    let owner = match owner_record(state.store().db()).await {
        Ok(Some(owner)) => owner,
        Ok(None) => {
            return pages::error_page(
                StatusCode::SERVICE_UNAVAILABLE,
                "Installation not initialized",
                "Run the host-access initialize command before signing in.",
                false,
            );
        }
        Err(_) => return pages::internal_error(),
    };

    let Ok(_permit) = state.login_permits().acquire().await else {
        return pages::internal_error();
    };
    let verified = verify_off_thread(owner.password_hash, form.password).await;
    if !verified {
        if record_login_failure(state.store().db(), &client, now)
            .await
            .is_err()
        {
            return pages::internal_error();
        }
        return login_page_response(
            &login_csrf,
            &next,
            Some("Invalid password."),
            StatusCode::UNAUTHORIZED,
            None,
        );
    }

    if clear_login_failures(state.store().db(), &client)
        .await
        .is_err()
    {
        return pages::internal_error();
    }

    let (raw_token, session) = match new_session(owner.generation, now) {
        Ok(session) => session,
        Err(_) => return pages::internal_error(),
    };
    if create_session(state.store().db(), &session, now)
        .await
        .is_err()
    {
        return pages::internal_error();
    }

    let mut response = redirect(&next);
    append_cookie(
        &mut response,
        &cookie(
            &state,
            session_cookie_name(&state),
            &raw_token,
            Some(SESSION_TTL_SECONDS),
        ),
    );
    append_cookie(
        &mut response,
        &cookie(&state, login_csrf_cookie_name(&state), "", Some(0)),
    );
    response
}

pub(crate) async fn logout(
    State(state): State<AppState>,
    CsrfForm { session, form }: CsrfForm<LogoutForm>,
) -> Response {
    debug_assert!(session.accepts_csrf(form.csrf_token()));
    if delete_session(state.store().db(), &session.token_hash)
        .await
        .is_err()
    {
        return pages::internal_error();
    }

    let mut response = redirect("/login");
    append_cookie(
        &mut response,
        &cookie(&state, session_cookie_name(&state), "", Some(0)),
    );
    append_cookie(
        &mut response,
        &cookie(&state, login_csrf_cookie_name(&state), "", Some(0)),
    );
    response
}

async fn current_session(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Option<Session>, crate::db::DbError> {
    let Some(token) = session_token(state, headers) else {
        return Ok(None);
    };
    load_session(state.store().db(), &hash_token(&token), now_unix()).await
}

fn session_token(state: &AppState, headers: &HeaderMap) -> Option<String> {
    cookie_value(headers, session_cookie_name(state)).filter(|value| is_token_shape(value))
}

fn session_cookie_name(state: &AppState) -> &'static str {
    if state.config().secure_cookies {
        SESSION_COOKIE_SECURE
    } else {
        SESSION_COOKIE_LOCAL
    }
}

fn login_csrf_cookie_name(state: &AppState) -> &'static str {
    if state.config().secure_cookies {
        LOGIN_CSRF_COOKIE_SECURE
    } else {
        LOGIN_CSRF_COOKIE_LOCAL
    }
}

fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(axum::http::header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|pair| {
            let (key, value) = pair.trim().split_once('=')?;
            (key == name).then(|| value.to_owned())
        })
        .next()
}

fn cookie(state: &AppState, name: &str, value: &str, max_age: Option<i64>) -> String {
    let mut cookie = format!("{name}={value}; Path=/; HttpOnly; SameSite=Lax");
    if state.config().secure_cookies {
        cookie.push_str("; Secure");
    }
    if let Some(age) = max_age {
        cookie.push_str("; Max-Age=");
        cookie.push_str(&age.to_string());
    }
    cookie
}

fn append_cookie(response: &mut Response, cookie: &str) {
    if let Ok(value) = HeaderValue::from_str(cookie) {
        response.headers_mut().append(SET_COOKIE, value);
    }
}

fn login_page_response(
    csrf_token: &str,
    next: &str,
    error: Option<&str>,
    status: StatusCode,
    retry_after_seconds: Option<i64>,
) -> Response {
    let Ok(html) = (LoginPage {
        csrf_token,
        next,
        error,
    })
    .render()
    else {
        return pages::internal_error();
    };
    let mut response = (status, Html(html)).into_response();
    if let Some(seconds) = retry_after_seconds
        && let Ok(value) = HeaderValue::from_str(&seconds.to_string())
    {
        response.headers_mut().insert(RETRY_AFTER, value);
    }
    response
}

fn login_error(code: Option<&str>) -> Option<&'static str> {
    match code {
        Some("invalid") => Some("Invalid password."),
        Some("throttled") => Some("Too many failed attempts. Try again later."),
        _ => None,
    }
}

fn redirect(location: &str) -> Response {
    let value = HeaderValue::from_str(location).unwrap_or_else(|_| HeaderValue::from_static("/"));
    (StatusCode::SEE_OTHER, [(LOCATION, value)]).into_response()
}

fn redirect_to_login(request: &Request) -> Response {
    let target = request
        .uri()
        .path_and_query()
        .map(|parts| parts.as_str())
        .unwrap_or("/");
    let next = sanitized_next(Some(target));
    redirect(&format!("/login?next={}", encode_query_component(&next)))
}

/// Accepts only application-local paths, never schemes, protocol-relative
/// URLs, control characters, or the authentication endpoints themselves.
fn sanitized_next(raw: Option<&str>) -> String {
    raw.and_then(local_target)
        .filter(|target| {
            !matches!(*target, "/login" | "/logout")
                && !target.starts_with("/login?")
                && !target.starts_with("/logout?")
        })
        .unwrap_or("/")
        .to_owned()
}

fn local_target(raw: &str) -> Option<&str> {
    if raw.len() > MAX_RETURN_LINK_BYTES
        || !raw.starts_with('/')
        || raw.starts_with("//")
        || raw.contains('\\')
        || raw.chars().any(char::is_control)
    {
        return None;
    }
    Some(raw)
}

fn encode_query_component(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                encoded.push(char::from(byte));
            }
            _ => {
                const HEX: &[u8; 16] = b"0123456789ABCDEF";
                encoded.push('%');
                encoded.push(char::from(HEX[usize::from(byte >> 4)]));
                encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
            }
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn return_links_stay_local() {
        assert_eq!(sanitized_next(Some("/budget?month=2026-11")), "/budget?month=2026-11");
        assert_eq!(sanitized_next(Some("https://evil.example/")), "/");
        assert_eq!(sanitized_next(Some("//evil.example/")), "/");
        assert_eq!(sanitized_next(Some("/\\evil.example")), "/");
        assert_eq!(sanitized_next(Some("/login")), "/");
        assert_eq!(sanitized_next(None), "/");
    }

    #[test]
    fn query_components_escape_reserved_characters() {
        assert_eq!(encode_query_component("/budget?x=1&y=2"), "%2Fbudget%3Fx%3D1%26y%3D2");
    }
}
