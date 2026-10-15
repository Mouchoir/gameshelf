//! Connector errors (contracts/connector.md section 4). Messages never contain secrets or full URLs.

use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ConnectorError {
    /// Refresh failed or the credential was rejected: the account needs reconnecting.
    #[error("the credential expired or was rejected")]
    AuthExpired,
    /// Wrong key or account mismatch during validation.
    #[error("the credential is invalid: {0}")]
    AuthInvalid(String),
    #[error("rate limited by the platform")]
    RateLimited { retry_after: Option<Duration> },
    /// Network failure or server error, retried with backoff.
    #[error("temporary failure: {0}")]
    Transient(String),
    /// Other client error, region restriction...
    #[error("request refused: {0}")]
    Permanent(String),
    /// Unexpected payload shape.
    #[error("unexpected response: {context}")]
    Parse { context: String },
    #[error("not supported by this platform")]
    Unsupported,
}

impl ConnectorError {
    /// Stable kind stored in sync runs.
    pub fn kind(&self) -> &'static str {
        match self {
            ConnectorError::AuthExpired | ConnectorError::AuthInvalid(_) => "auth_expired",
            ConnectorError::RateLimited { .. } => "rate_limited",
            ConnectorError::Transient(_) => "transient",
            ConnectorError::Permanent(_) | ConnectorError::Unsupported => "permanent",
            ConnectorError::Parse { .. } => "parse",
        }
    }

    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            ConnectorError::Transient(_) | ConnectorError::RateLimited { .. }
        )
    }

    pub fn is_auth(&self) -> bool {
        matches!(self, ConnectorError::AuthExpired | ConnectorError::AuthInvalid(_))
    }

    pub(crate) fn parse(context: impl Into<String>) -> Self {
        ConnectorError::Parse {
            context: context.into(),
        }
    }
}

/// Converts a transport error without leaking the URL (which may carry a key in its query string).
impl From<reqwest::Error> for ConnectorError {
    fn from(e: reqwest::Error) -> Self {
        let e = e.without_url();
        if e.is_decode() {
            ConnectorError::parse(e.to_string())
        } else {
            ConnectorError::Transient(e.to_string())
        }
    }
}
