use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Method, Request, StatusCode, header};
use axum::response::Response;
use http_body_util::BodyExt;
use orchard::auth::{self, AuthError, SESSION_TTL_SECONDS};
use orchard::db::{Store, StorePaths};
use orchard::secrets::{self, SecretKey};
use orchard::web::{AppState, WebConfig, router};
use tempfile::TempDir;
use tower::ServiceExt;

const PASSWORD: &str = "correct horse battery staple";
const REPLACEMENT: &str = "replacement battery staple";
const ORIGIN: &str = "http://localhost";

struct TestApp {
    _dir: TempDir,
    store: Arc<Store>,
    app: Router,
    key: SecretKey,
}

fn test_app() -> TestApp {
    let directory = TempDir::new().unwrap();
    let key = SecretKey::from_bytes([9; 32]);
    let store = Arc::new(Store::open(StorePaths::new(directory.path()), &key).unwrap());
    let state = AppState::new(Arc::clone(&store), WebConfig::default());
    TestApp {
        _dir: directory,
        app: router(state),
        store,
        key,
    }
}

fn with_peer(mut request: Request<Body>, peer: &str) -> Request<Body> {
    let socket: SocketAddr = format!("{peer}:50000").parse().unwrap();
    request.extensions_mut().insert(ConnectInfo(socket));
    request
}

fn request(method: Method, uri: &str) -> axum::http::request::Builder {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(header::HOST, "localhost")
}

async fn send(app: &Router, request: Request<Body>) -> Response {
    app.clone().oneshot(request).await.unwrap()
}

async fn get(app: &Router, uri: &str, cookies: &[String], peer: &str) -> Response {
    let mut builder = request(Method::GET, uri);
    if !cookies.is_empty() {
        builder = builder.header(header::COOKIE, cookies.join("; "));
    }
    send(app, with_peer(builder.body(Body::empty()).unwrap(), peer)).await
}

async fn post_form(
    app: &Router,
    uri: &str,
    cookies: &[String],
    peer: &str,
    origin: Option<&str>,
    fields: &[(&str, &str)],
) -> Response {
    let body = fields
        .iter()
        .map(|(key, value)| format!("{}={}", encode(key), encode(value)))
        .collect::<Vec<_>>()
        .join("&");
    let mut builder = request(Method::POST, uri).header(
        header::CONTENT_TYPE,
        "application/x-www-form-urlencoded",
    );
    if let Some(origin) = origin {
        builder = builder.header(header::ORIGIN, origin);
    }
    if !cookies.is_empty() {
        builder = builder.header(header::COOKIE, cookies.join("; "));
    }
    send(
        app,
        with_peer(builder.body(Body::from(body)).unwrap(), peer),
    )
    .await
}

async fn body_text(response: Response) -> String {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

fn response_cookies(response: &Response) -> Vec<String> {
    response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .map(str::to_owned)
        .collect()
}

fn cookie_pair(cookies: &[String], name: &str) -> Option<String> {
    cookies
        .iter()
        .find(|cookie| cookie.starts_with(&format!("{name}=")))
        .map(|cookie| cookie.split(';').next().unwrap().to_owned())
}

fn hidden_value(html: &str, name: &str) -> String {
    let marker = format!("name=\"{name}\" value=\"");
    let start = html.find(&marker).unwrap() + marker.len();
    let end = html[start..].find('"').unwrap();
    html[start..start + end].to_owned()
}

fn encode(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                encoded.push(char::from(byte));
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}


async fn login(
    app: &Router,
    password: &str,
    peer: &str,
    next: Option<&str>,
) -> Response {
    let uri = match next {
        Some(next) => format!("/login?next={}", encode(next)),
        None => "/login".to_owned(),
    };
    let response = get(app, &uri, &[], peer).await;
    let cookies = response_cookies(&response);
    let login_csrf = cookie_pair(&cookies, "__Host-orchard_login_csrf").unwrap();
    let html = body_text(response).await;
    let csrf = hidden_value(&html, "csrf_token");
    let hidden_next = hidden_value(&html, "next");
    post_form(
        app,
        "/login",
        &[login_csrf],
        peer,
        Some(ORIGIN),
        &[
            ("csrf_token", &csrf),
            ("next", &hidden_next),
            ("password", password),
        ],
    )
    .await
}

fn session_cookie(response: &Response) -> String {
    cookie_pair(&response_cookies(response), "__Host-orchard_session").unwrap()
}

async fn initialize(app: &TestApp) {
    auth::initialize(app.store.db(), PASSWORD, "America/Chicago", "Home")
        .await
        .unwrap();
}

#[tokio::test]
async fn initialization_creates_one_owner_and_usd_budget_exactly_once() {
    let app = test_app();

    let invalid = auth::initialize(app.store.db(), PASSWORD, "Mars/Olympus", "Home")
        .await
        .unwrap_err();
    assert!(matches!(invalid, AuthError::InvalidTimezone { .. }));

    initialize(&app).await;
    let repeated = auth::initialize(app.store.db(), REPLACEMENT, "Europe/London", "Other")
        .await
        .unwrap_err();
    assert!(matches!(repeated, AuthError::AlreadyInitialized));

    let (owners, name, currency, timezone): (i64, String, String, String) = app
        .store
        .db()
        .read(|conn| {
            let owners = conn.query_row("SELECT COUNT(*) FROM owners", [], |row| row.get(0))?;
            let (name, currency, timezone) = conn.query_row(
                "SELECT name, currency, timezone FROM budgets WHERE id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )?;
            Ok((owners, name, currency, timezone))
        })
        .await
        .unwrap();
    assert_eq!(owners, 1);
    assert_eq!(name, "Home");
    assert_eq!(currency, "USD");
    assert_eq!(timezone, "America/Chicago");

    let hash: String = app
        .store
        .db()
        .read(|conn| {
            Ok(conn.query_row("SELECT password_hash FROM owners WHERE id = 1", [], |row| {
                row.get(0)
            })?)
        })
        .await
        .unwrap();
    assert!(!orchard::auth::verify_off_thread(hash, REPLACEMENT.to_owned()).await);
}

#[tokio::test]
async fn login_creates_a_hashed_session_and_logout_revokes_it() {
    let app = test_app();
    initialize(&app).await;

    let response = login(&app.app, PASSWORD, "198.51.100.10", None).await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(response.headers()[header::LOCATION], "/");
    let cookies = response_cookies(&response);
    let session = cookie_pair(&cookies, "__Host-orchard_session").unwrap();
    assert!(
        cookies
            .iter()
            .any(|cookie| cookie.starts_with("__Host-orchard_session=")
                && cookie.contains("; Secure")
                && cookie.contains("; HttpOnly")
                && cookie.contains("; SameSite=Lax")
                && cookie.contains(&format!("Max-Age={SESSION_TTL_SECONDS}")))
    );

    let raw_token = session.split_once('=').unwrap().1.to_owned();
    let token_hash = auth::hash_token(&raw_token);
    let stored: i64 = app
        .store
        .db()
        .read(move |conn| {
            Ok(conn.query_row(
                "SELECT COUNT(*) FROM sessions WHERE token_hash = ?1",
                [token_hash],
                |row| row.get(0),
            )?)
        })
        .await
        .unwrap();
    assert_eq!(stored, 1);
    let raw_rows: i64 = app
        .store
        .db()
        .read(move |conn| {
            Ok(conn.query_row(
                "SELECT COUNT(*) FROM sessions WHERE token_hash = ?1",
                [raw_token],
                |row| row.get(0),
            )?)
        })
        .await
        .unwrap();
    assert_eq!(raw_rows, 0);

    let home = get(&app.app, "/", std::slice::from_ref(&session), "198.51.100.10").await;
    assert_eq!(home.status(), StatusCode::OK);
    let html = body_text(home).await;
    assert!(html.contains("Home"));
    assert!(html.contains("America/Chicago"));
    assert!(html.contains("USD"));
    let csrf = hidden_value(&html, "csrf_token");

    let logout = post_form(
        &app.app,
        "/logout",
        std::slice::from_ref(&session),
        "198.51.100.10",
        Some(ORIGIN),
        &[("csrf_token", &csrf)],
    )
    .await;
    assert_eq!(logout.status(), StatusCode::SEE_OTHER);
    assert_eq!(logout.headers()[header::LOCATION], "/login");

    let reused = get(&app.app, "/", &[session], "198.51.100.10").await;
    assert_eq!(reused.status(), StatusCode::SEE_OTHER);
    assert!(
        reused.headers()[header::LOCATION]
            .to_str()
            .unwrap()
            .starts_with("/login?next=")
    );
    let sessions: i64 = app
        .store
        .db()
        .read(|conn| Ok(conn.query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))?))
        .await
        .unwrap();
    assert_eq!(sessions, 0);
}

#[tokio::test]
async fn expired_sessions_cannot_read_or_mutate() {
    let app = test_app();
    initialize(&app).await;

    let response = login(&app.app, PASSWORD, "198.51.100.11", None).await;
    let session = session_cookie(&response);
    let csrf = {
        let home = get(&app.app, "/", std::slice::from_ref(&session), "198.51.100.11").await;
        hidden_value(&body_text(home).await, "csrf_token")
    };

    app.store
        .db()
        .transaction(|tx| {
            tx.execute("UPDATE sessions SET expires_at = 1", [])?;
            Ok(())
        })
        .await
        .unwrap();

    let read = get(&app.app, "/", std::slice::from_ref(&session), "198.51.100.11").await;
    assert_eq!(read.status(), StatusCode::SEE_OTHER);
    assert!(
        read.headers()[header::LOCATION]
            .to_str()
            .unwrap()
            .starts_with("/login?next=")
    );

    let mutation = post_form(
        &app.app,
        "/logout",
        &[session],
        "198.51.100.11",
        Some(ORIGIN),
        &[("csrf_token", &csrf)],
    )
    .await;
    assert_eq!(mutation.status(), StatusCode::SEE_OTHER);
    let sessions: i64 = app
        .store
        .db()
        .read(|conn| Ok(conn.query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))?))
        .await
        .unwrap();
    assert_eq!(sessions, 1, "expired mutation must not reach its handler");
}

#[tokio::test]
async fn cross_origin_and_missing_csrf_mutations_do_not_commit() {
    let app = test_app();
    initialize(&app).await;

    let response = login(&app.app, PASSWORD, "198.51.100.12", None).await;
    let session = session_cookie(&response);
    let csrf = {
        let home = get(&app.app, "/", std::slice::from_ref(&session), "198.51.100.12").await;
        hidden_value(&body_text(home).await, "csrf_token")
    };

    let cross_origin = post_form(
        &app.app,
        "/logout",
        std::slice::from_ref(&session),
        "198.51.100.12",
        Some("https://evil.example"),
        &[("csrf_token", &csrf)],
    )
    .await;
    assert_eq!(cross_origin.status(), StatusCode::FORBIDDEN);

    let no_origin = post_form(
        &app.app,
        "/logout",
        std::slice::from_ref(&session),
        "198.51.100.12",
        None,
        &[("csrf_token", &csrf)],
    )
    .await;
    assert_eq!(no_origin.status(), StatusCode::FORBIDDEN);

    let stale_token = "0".repeat(64);
    let stale_csrf = post_form(
        &app.app,
        "/logout",
        std::slice::from_ref(&session),
        "198.51.100.12",
        Some(ORIGIN),
        &[("csrf_token", stale_token.as_str())],
    )
    .await;
    assert_eq!(stale_csrf.status(), StatusCode::FORBIDDEN);

    let still_valid = get(&app.app, "/", &[session], "198.51.100.12").await;
    assert_eq!(still_valid.status(), StatusCode::OK);
}

#[tokio::test]
async fn repeated_invalid_credentials_are_throttled_per_trusted_client() {
    let app = test_app();
    initialize(&app).await;

    for _ in 0..5 {
        let response = login(&app.app, "wrong password value", "203.0.113.9", None).await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    let blocked = login(&app.app, PASSWORD, "203.0.113.9", None).await;
    assert_eq!(blocked.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(blocked.headers().contains_key(header::RETRY_AFTER));

    let other_client = login(&app.app, PASSWORD, "203.0.113.10", None).await;
    assert_eq!(other_client.status(), StatusCode::SEE_OTHER);

    // The peer is not a configured proxy here, so a spoofed forwarded address
    // must not reset the throttle key.
    let proxied = AppState::new(
        Arc::clone(&app.store),
        WebConfig {
            trusted_proxies: vec!["127.0.0.1".parse().unwrap()],
            ..WebConfig::default()
        },
    );
    let trusted_app = router(proxied);
    let trusted_login = login(&trusted_app, PASSWORD, "127.0.0.1", None).await;
    assert_eq!(trusted_login.status(), StatusCode::SEE_OTHER);
}

#[tokio::test]
async fn host_recovery_preserves_data_and_invalidates_password_and_sessions() {
    let app = test_app();
    initialize(&app).await;

    let response = login(&app.app, PASSWORD, "198.51.100.13", None).await;
    let old_session = session_cookie(&response);

    let key = app.key.clone();
    app.store
        .db()
        .transaction(move |tx| {
            secrets::store_provider_secret(tx, &key, "simplefin", b"access-url")?;
            Ok(())
        })
        .await
        .unwrap();
    let envelope_before: Vec<u8> = app
        .store
        .db()
        .read(|conn| {
            Ok(conn.query_row(
                "SELECT envelope FROM provider_secrets WHERE provider = 'simplefin'",
                [],
                |row| row.get(0),
            )?)
        })
        .await
        .unwrap();

    auth::recover(app.store.db(), REPLACEMENT).await.unwrap();

    let sessions: i64 = app
        .store
        .db()
        .read(|conn| Ok(conn.query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))?))
        .await
        .unwrap();
    assert_eq!(sessions, 0);

    let old_cookie = get(&app.app, "/", &[old_session], "198.51.100.13").await;
    assert_eq!(old_cookie.status(), StatusCode::SEE_OTHER);

    let old_password = login(&app.app, PASSWORD, "198.51.100.13", None).await;
    assert_eq!(old_password.status(), StatusCode::UNAUTHORIZED);
    let new_password = login(&app.app, REPLACEMENT, "198.51.100.13", None).await;
    assert_eq!(new_password.status(), StatusCode::SEE_OTHER);

    let (name, timezone): (String, String) = app
        .store
        .db()
        .read(|conn| {
            Ok(conn.query_row(
                "SELECT name, timezone FROM budgets WHERE id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?)
        })
        .await
        .unwrap();
    assert_eq!(name, "Home");
    assert_eq!(timezone, "America/Chicago");

    let envelope_after: Vec<u8> = app
        .store
        .db()
        .read(|conn| {
            Ok(conn.query_row(
                "SELECT envelope FROM provider_secrets WHERE provider = 'simplefin'",
                [],
                |row| row.get(0),
            )?)
        })
        .await
        .unwrap();
    assert_eq!(envelope_after, envelope_before);

    let key = app.key.clone();
    let plaintext = app
        .store
        .db()
        .read(move |conn| Ok(secrets::read_provider_secret(conn, &key, "simplefin")?))
        .await
        .unwrap();
    assert_eq!(plaintext, b"access-url");
}

#[tokio::test]
async fn unauthenticated_requests_disclose_no_private_data() {
    let app = test_app();
    initialize(&app).await;

    let private_page = get(&app.app, "/", &[], "198.51.100.14").await;
    assert_eq!(private_page.status(), StatusCode::SEE_OTHER);
    assert!(
        private_page.headers()[header::LOCATION]
            .to_str()
            .unwrap()
            .starts_with("/login?next=")
    );
    let body = body_text(private_page).await;
    assert!(!body.contains("Home"));
    assert!(!body.contains("America/Chicago"));

    let unknown = get(&app.app, "/accounts/1", &[], "198.51.100.14").await;
    assert_eq!(unknown.status(), StatusCode::NOT_FOUND);
    let body = body_text(unknown).await;
    assert!(!body.contains("Home"));
    assert!(!body.contains("America/Chicago"));

    let login_html = body_text(get(&app.app, "/login", &[], "198.51.100.14").await).await;
    assert!(!login_html.contains("Home"));
    assert!(!login_html.contains("America/Chicago"));
}

#[tokio::test]
async fn post_login_returns_only_to_local_paths() {
    let app = test_app();
    initialize(&app).await;

    let response = get(&app.app, "/login?next=https://evil.example/x", &[], "198.51.100.15").await;
    let html = body_text(response).await;
    assert_eq!(hidden_value(&html, "next"), "/");

    let response = get(
        &app.app,
        "/login?next=%2Fbudget%3Fmonth%3D2026-11",
        &[],
        "198.51.100.15",
    )
    .await;
    let html = body_text(response).await;
    assert_eq!(hidden_value(&html, "next"), "/budget?month=2026-11");

    let response = login(&app.app, PASSWORD, "198.51.100.15", Some("/budget?month=2026-11")).await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        response.headers()[header::LOCATION],
        "/budget?month=2026-11"
    );
}

#[tokio::test]
async fn forwarded_client_addresses_are_trusted_only_from_configured_proxies() {
    let app = test_app();
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        "x-forwarded-for",
        "192.0.2.55, 10.0.0.2".parse().unwrap(),
    );
    let peer: SocketAddr = "127.0.0.1:50000".parse().unwrap();

    let direct = AppState::new(Arc::clone(&app.store), WebConfig::default());
    assert_eq!(orchard::web::client_key(&direct, peer, &headers), "127.0.0.1");

    let proxied = AppState::new(
        Arc::clone(&app.store),
        WebConfig {
            trusted_proxies: vec!["127.0.0.1".parse().unwrap()],
            ..WebConfig::default()
        },
    );
    assert_eq!(
        orchard::web::client_key(&proxied, peer, &headers),
        "192.0.2.55"
    );
}

#[tokio::test]
async fn login_page_keeps_the_browser_origin_header_intact() {
    let app = test_app();
    let response = get(&app.app, "/login", &[], "198.51.100.16").await;
    assert_eq!(response.status(), StatusCode::OK);
    // Chromium sends `Origin: null` for form POSTs when the document is served
    // with `Referrer-Policy: no-referrer`, which would reject every login.
    assert_eq!(
        response.headers()[header::REFERRER_POLICY],
        "same-origin"
    );
}
