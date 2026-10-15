//! Shared HTTP client with per-platform rate limiting and retries (research D6).

use crate::{ConnectorError, descriptor::RateLimit};
use backon::{ExponentialBuilder, Retryable};
use governor::{DefaultDirectRateLimiter, Quota, RateLimiter};
use reqwest::{Response, StatusCode, header::RETRY_AFTER};
use serde::de::DeserializeOwned;
use std::{num::NonZeroU32, sync::Arc, time::Duration};

/// User agent sent to platforms: identifies the software, never the operator.
pub const USER_AGENT: &str = concat!(
    "gameshelf/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/Mouchoir/gameshelf)"
);

/// Retries after the first attempt.
const MAX_RETRIES: usize = 3;

/// Context handed to connectors for one platform.
#[derive(Clone)]
pub struct Context {
    pub http: reqwest::Client,
    limiter: Arc<DefaultDirectRateLimiter>,
    retry: ExponentialBuilder,
}

impl std::fmt::Debug for Context {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Context").finish_non_exhaustive()
    }
}

/// Builds the shared client.
pub fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(10))
        .build()
        .unwrap_or_default()
}

impl Context {
    pub fn new(http: reqwest::Client, rate: RateLimit) -> Self {
        let burst = NonZeroU32::new(rate.requests.max(1)).unwrap_or(NonZeroU32::MIN);
        let period = rate.per / burst.get();
        let quota = Quota::with_period(period.max(Duration::from_millis(1)))
            .unwrap_or_else(|| Quota::per_second(NonZeroU32::MIN))
            .allow_burst(burst);
        Self {
            http,
            limiter: Arc::new(RateLimiter::direct(quota)),
            retry: ExponentialBuilder::default()
                .with_min_delay(Duration::from_millis(500))
                .with_max_delay(Duration::from_secs(30))
                .with_max_times(MAX_RETRIES)
                .with_jitter(),
        }
    }

    /// A context with faster retries, for tests.
    pub fn for_tests() -> Self {
        let mut cx = Self::new(
            client(),
            RateLimit {
                requests: 1000,
                per: Duration::from_secs(1),
            },
        );
        cx.retry = ExponentialBuilder::default()
            .with_min_delay(Duration::from_millis(1))
            .with_max_delay(Duration::from_millis(5))
            .with_max_times(MAX_RETRIES);
        cx
    }

    /// Sends a request built by `make` (called again for each retry), waits for the rate limiter, retries
    /// transient failures and maps statuses to [`ConnectorError`].
    pub async fn send<F>(&self, make: F) -> Result<Response, ConnectorError>
    where
        F: Fn() -> reqwest::RequestBuilder,
    {
        let attempt = || async {
            self.limiter.until_ready().await;
            let resp = make().send().await?;
            check_status(resp).await
        };
        attempt
            .retry(self.retry)
            .when(ConnectorError::is_retryable)
            // Honor Retry-After, but only while the backoff still allows a retry (`dur` is None once exhausted).
            .adjust(|err, dur| match (err, dur) {
                (
                    ConnectorError::RateLimited {
                        retry_after: Some(after),
                    },
                    Some(_),
                ) => Some((*after).min(Duration::from_secs(120))),
                _ => dur,
            })
            .await
    }

    /// Sends a request and decodes a JSON body.
    pub async fn json<T, F>(&self, make: F) -> Result<T, ConnectorError>
    where
        T: DeserializeOwned,
        F: Fn() -> reqwest::RequestBuilder,
    {
        let resp = self.send(make).await?;
        let bytes = resp.bytes().await?;
        serde_json::from_slice(&bytes).map_err(|e| ConnectorError::parse(format!("invalid JSON: {e}")))
    }
}

async fn check_status(resp: Response) -> Result<Response, ConnectorError> {
    let status = resp.status();
    if status.is_success() {
        return Ok(resp);
    }
    Err(match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => ConnectorError::AuthExpired,
        StatusCode::TOO_MANY_REQUESTS => ConnectorError::RateLimited {
            retry_after: resp
                .headers()
                .get(RETRY_AFTER)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.trim().parse::<u64>().ok())
                .map(Duration::from_secs),
        },
        s if s.is_server_error() => ConnectorError::Transient(format!("server error {}", s.as_u16())),
        s => ConnectorError::Permanent(format!("status {}", s.as_u16())),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path},
    };

    #[tokio::test]
    async fn retries_server_errors_then_succeeds() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/x"))
            .respond_with(ResponseTemplate::new(503))
            .up_to_n_times(2)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/x"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true})))
            .mount(&server)
            .await;
        let cx = Context::for_tests();
        let url = format!("{}/x", server.uri());
        let v: serde_json::Value = cx.json(|| cx.http.get(&url)).await.expect("eventually ok");
        assert_eq!(v["ok"], true);
    }

    #[tokio::test]
    async fn auth_errors_are_not_retried() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(401))
            .expect(1)
            .mount(&server)
            .await;
        let cx = Context::for_tests();
        let url = server.uri();
        let err = cx.send(|| cx.http.get(&url)).await.unwrap_err();
        assert_eq!(err, ConnectorError::AuthExpired);
    }

    #[tokio::test]
    async fn rate_limit_honors_retry_after_and_gives_up() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "0"))
            .mount(&server)
            .await;
        let cx = Context::for_tests();
        let url = server.uri();
        let err = cx.send(|| cx.http.get(&url)).await.unwrap_err();
        assert!(matches!(err, ConnectorError::RateLimited { .. }));
    }

    #[tokio::test]
    async fn errors_never_contain_the_query_string() {
        let cx = Context::for_tests();
        let err = cx
            .send(|| cx.http.get("http://127.0.0.1:9/never?key=SECRET-KEY"))
            .await
            .unwrap_err();
        assert!(!err.to_string().contains("SECRET-KEY"));
    }
}
