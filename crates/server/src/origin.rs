//! Origin check for state-changing requests (contracts/http-api.md section 3).
//!
//! Browsers always send `Origin` on cross-site and same-site POSTs; a request whose Origin (or, failing that,
//! Referer) names another site is refused. The agent API authenticates with bearer tokens and is exempt.

use axum::{
    body::Body,
    extract::State,
    http::{HeaderMap, Method, Request, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use url::Url;

#[derive(Clone, Debug)]
pub struct Policy {
    base: Option<Url>,
}

impl Policy {
    pub fn new(base: Option<Url>) -> Self {
        Self { base }
    }

    /// Whether `source` (an Origin or Referer value) is acceptable for a request carrying `headers`.
    fn allows(&self, source: &str, headers: &HeaderMap) -> bool {
        let Ok(url) = Url::parse(source) else { return false };
        let Some(host) = url.host_str() else { return false };
        let authority = match url.port() {
            Some(p) => format!("{host}:{p}"),
            None => host.to_string(),
        };
        if let Some(base) = &self.base
            && base.host_str() == Some(host)
            && base.port_or_known_default() == url.port_or_known_default()
        {
            return true;
        }
        let request_host = headers
            .get(header::HOST)
            .and_then(|h| h.to_str().ok())
            .unwrap_or_default();
        if request_host.eq_ignore_ascii_case(&authority) {
            return true;
        }
        matches!(host, "localhost" | "127.0.0.1" | "[::1]") && request_host.starts_with(host)
    }
}

pub async fn check(State(policy): State<Policy>, req: Request<Body>, next: Next) -> Response {
    let safe = matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS);
    if safe || req.uri().path().starts_with("/api/agent/") {
        return next.run(req).await;
    }
    let headers = req.headers();
    let source = headers
        .get(header::ORIGIN)
        .or_else(|| headers.get(header::REFERER))
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    match source {
        Some(s) if s != "null" && !policy.allows(&s, headers) => {
            tracing::warn!(origin = %s, "refused a cross-site request");
            (StatusCode::FORBIDDEN, "cross-site request refused").into_response()
        }
        _ => next.run(req).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(host: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(header::HOST, host.parse().expect("header"));
        h
    }

    #[test]
    fn same_host_is_allowed() {
        let p = Policy::new(None);
        assert!(p.allows("http://192.0.2.10:8080", &headers("192.0.2.10:8080")));
        assert!(p.allows("http://192.0.2.10:8080/library", &headers("192.0.2.10:8080")));
    }

    #[test]
    fn other_sites_are_refused() {
        let p = Policy::new(None);
        assert!(!p.allows("https://evil.example", &headers("192.0.2.10:8080")));
        assert!(!p.allows("not a url", &headers("192.0.2.10:8080")));
    }

    #[test]
    fn the_public_base_url_is_allowed_behind_a_proxy() {
        let p = Policy::new(Url::parse("https://gameshelf.example.com").ok());
        assert!(p.allows("https://gameshelf.example.com", &headers("internal:8080")));
        assert!(!p.allows("https://other.example.com", &headers("internal:8080")));
    }
}
