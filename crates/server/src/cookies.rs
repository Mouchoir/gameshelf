//! Marks cookies `Secure` on requests that reach the public HTTPS address, and leaves them plain on the local
//! network, where the server is usually reached over HTTP. A browser drops a `Secure` cookie set over HTTP, so a
//! fixed flag would break either the tunnel or the LAN.

use axum::{
    body::Body,
    extract::State,
    http::{HeaderMap, HeaderValue, Request, header},
    middleware::Next,
    response::Response,
};
use url::Url;

/// Whether the request was addressed to the public HTTPS host.
fn is_public(base: Option<&Url>, headers: &HeaderMap) -> bool {
    let Some(base) = base.filter(|b| b.scheme() == "https") else {
        return false;
    };
    let Some(host) = headers.get(header::HOST).and_then(|h| h.to_str().ok()) else {
        return false;
    };
    let name = host_name(host);
    base.host_str().is_some_and(|b| b.eq_ignore_ascii_case(name))
}

/// The host name without its port; IPv6 brackets are kept, as in `Url::host_str`.
fn host_name(host: &str) -> &str {
    if host.starts_with('[') {
        return match host.find(']') {
            Some(i) => &host[..=i],
            None => host,
        };
    }
    match host.rsplit_once(':') {
        Some((name, port)) if !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()) => name,
        _ => host,
    }
}

/// Adds `Secure` to every `Set-Cookie` header that lacks it.
fn mark_secure(headers: &mut HeaderMap) {
    let values: Vec<HeaderValue> = headers.get_all(header::SET_COOKIE).iter().cloned().collect();
    if values.is_empty() {
        return;
    }
    headers.remove(header::SET_COOKIE);
    for v in values {
        let updated = match v.to_str() {
            Ok(s)
                if !s
                    .split(';')
                    .any(|part| part.trim().eq_ignore_ascii_case("secure")) =>
            {
                HeaderValue::from_str(&format!("{s}; Secure")).unwrap_or(v)
            }
            _ => v,
        };
        headers.append(header::SET_COOKIE, updated);
    }
}

pub async fn secure(State(base): State<Option<Url>>, req: Request<Body>, next: Next) -> Response {
    let public = is_public(base.as_ref(), req.headers());
    let mut resp = next.run(req).await;
    if public {
        mark_secure(resp.headers_mut());
    }
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(h: &str) -> HeaderMap {
        let mut m = HeaderMap::new();
        m.insert(header::HOST, h.parse().expect("header"));
        m
    }

    #[test]
    fn only_the_public_https_host_is_public() {
        let base = Url::parse("https://gameshelf.example.com").ok();
        assert!(is_public(base.as_ref(), &host("gameshelf.example.com")));
        assert!(is_public(base.as_ref(), &host("GameShelf.example.com:443")));
        assert!(!is_public(base.as_ref(), &host("192.0.2.10:8080")));
        assert!(!is_public(base.as_ref(), &host("[2001:db8::1]:8080")));
        assert!(!is_public(None, &host("gameshelf.example.com")));
        let http = Url::parse("http://gameshelf.example.com").ok();
        assert!(!is_public(http.as_ref(), &host("gameshelf.example.com")));
    }

    #[test]
    fn cookies_get_the_secure_flag_once() {
        let mut h = HeaderMap::new();
        h.append(
            header::SET_COOKIE,
            HeaderValue::from_static("a=1; Path=/; HttpOnly"),
        );
        h.append(header::SET_COOKIE, HeaderValue::from_static("b=2; Secure"));
        mark_secure(&mut h);
        let all: Vec<&str> = h
            .get_all(header::SET_COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .collect();
        assert_eq!(all, vec!["a=1; Path=/; HttpOnly; Secure", "b=2; Secure"]);
    }
}
