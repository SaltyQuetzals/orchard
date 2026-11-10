//! Same-origin enforcement for browser mutations and trusted-proxy handling.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use axum::{
    extract::{ConnectInfo, Request, State},
    http::{HeaderMap, Method, Uri, header},
    middleware::Next,
    response::Response,
};

use super::{AppState, pages};

/// Rejects cross-site unsafe requests before any handler body is parsed.
///
/// Browsers send `Origin` on form/fetch POSTs; a `Referer` is accepted as a
/// fallback. Requests with neither are refused rather than assumed local.
pub(crate) async fn enforce_origin(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    if !is_unsafe(request.method()) {
        return next.run(request).await;
    }

    if request
        .headers()
        .get("sec-fetch-site")
        .and_then(|value| value.to_str().ok())
        == Some("cross-site")
    {
        return pages::forbidden();
    }

    let Some(origin) = request_origin(request.headers()) else {
        return pages::forbidden();
    };
    let Some(expected) = expected_origin(&state, &request) else {
        return pages::forbidden();
    };
    if !origin.eq_ignore_ascii_case(&expected) {
        return pages::forbidden();
    }

    next.run(request).await
}

fn is_unsafe(method: &Method) -> bool {
    matches!(
        *method,
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    )
}

/// Parses an `Origin` or `Referer` value into `scheme://authority`.
fn origin_value(value: &str) -> Option<String> {
    let uri: Uri = value.parse().ok()?;
    let scheme = uri.scheme_str()?;
    let authority = uri.authority()?;
    Some(format!("{}://{}", scheme.to_ascii_lowercase(), authority))
}

fn request_origin(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        .and_then(origin_value)
        .or_else(|| {
            headers
                .get(header::REFERER)
                .and_then(|value| value.to_str().ok())
                .and_then(origin_value)
        })
}

/// The configured public origin, or the request's own Host when no public URL
/// is configured (local HTTP use).
fn expected_origin(state: &AppState, request: &Request) -> Option<String> {
    if let Some(public_url) = &state.config().public_url {
        return origin_value(public_url);
    }
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())?;
    let scheme = if is_trusted_proxy(state, peer_ip(request)) {
        request
            .headers()
            .get("x-forwarded-proto")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(',').next())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("http")
            .to_ascii_lowercase()
    } else {
        "http".to_owned()
    };
    Some(format!("{}://{}", scheme, host.to_ascii_lowercase()))
}

/// Client address for login throttling.
///
/// Forwarded addresses are used only when the TCP peer is one of the
/// configured proxies; otherwise a client could choose its own throttle key.
pub fn client_key(state: &AppState, peer: SocketAddr, headers: &HeaderMap) -> String {
    if is_trusted_proxy(state, peer.ip())
        && let Some(forwarded) = headers
            .get("x-forwarded-for")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(',').next())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .and_then(|value| value.parse::<IpAddr>().ok())
    {
        return forwarded.to_string();
    }
    peer.ip().to_string()
}

fn is_trusted_proxy(state: &AppState, peer: IpAddr) -> bool {
    state.config().trusted_proxies.contains(&peer)
}

/// TCP peer address supplied by Axum's connect-info service.
pub fn peer_ip(request: &Request) -> IpAddr {
    request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|info| info.0.ip())
        .unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST))
}
