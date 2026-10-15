//! HTTP client of the agent API (contracts/agent-api.md). The server never connects to the agent.

use flate2::{Compression, write::GzEncoder};
use gameshelf_core::agent::{AgentConfig, Heartbeat, PairRequest, PairResponse, Report, ReportResult};
use reqwest::{StatusCode, header};
use std::{io::Write, time::Duration};
use url::Url;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
const VERSION_HEADER: &str = "x-gameshelf-agent-version";

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    /// The agent was removed in the web app.
    #[error("this agent was removed from the server")]
    Revoked,
    #[error("the server needs agent version {0} or newer")]
    Upgrade(String),
    #[error("{message} ({code})")]
    Refused {
        status: u16,
        code: String,
        message: String,
    },
    #[error("cannot reach the server: {0}")]
    Network(String),
}

#[derive(Clone, Debug)]
pub struct Client {
    http: reqwest::Client,
    base: String,
}

/// Checks a server address typed by the player. Plain HTTP is allowed (local networks) but flagged.
pub fn parse_server(text: &str) -> Result<(String, bool), String> {
    let text = text.trim().trim_end_matches('/');
    let with_scheme = if text.contains("://") {
        text.to_string()
    } else {
        format!("https://{text}")
    };
    let url = Url::parse(&with_scheme).map_err(|e| e.to_string())?;
    match url.scheme() {
        "https" => Ok((with_scheme, false)),
        "http" => Ok((with_scheme, true)),
        other => Err(format!("unsupported scheme {other}")),
    }
}

impl Client {
    pub fn new(server: &str) -> Result<Self, String> {
        let (base, _) = parse_server(server)?;
        let http = reqwest::Client::builder()
            .user_agent(format!("gameshelf-agent/{VERSION}"))
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self { http, base })
    }

    fn url(&self, path: &str) -> String {
        format!("{}/api/agent/v1{path}", self.base)
    }

    fn authed(&self, req: reqwest::RequestBuilder, token: &str) -> reqwest::RequestBuilder {
        req.bearer_auth(token).header(VERSION_HEADER, VERSION)
    }

    async fn check(resp: Result<reqwest::Response, reqwest::Error>) -> Result<reqwest::Response, ApiError> {
        let resp = resp.map_err(|e| ApiError::Network(e.without_url().to_string()))?;
        let status = resp.status();
        if status.is_success() {
            return Ok(resp);
        }
        let body: serde_json::Value = resp.json().await.unwrap_or_default();
        let code = body["error"]["code"].as_str().unwrap_or("http_error").to_string();
        if status == StatusCode::UPGRADE_REQUIRED {
            return Err(ApiError::Upgrade(
                body["min_version"].as_str().unwrap_or("?").to_string(),
            ));
        }
        if status == StatusCode::UNAUTHORIZED && code == "agent_revoked" {
            return Err(ApiError::Revoked);
        }
        Err(ApiError::Refused {
            status: status.as_u16(),
            message: body["error"]["message"]
                .as_str()
                .unwrap_or(status.as_str())
                .to_string(),
            code,
        })
    }

    pub async fn pair(&self, req: &PairRequest) -> Result<PairResponse, ApiError> {
        let resp = Self::check(self.http.post(self.url("/pair")).json(req).send().await).await?;
        resp.json()
            .await
            .map_err(|e| ApiError::Network(e.without_url().to_string()))
    }

    pub async fn config(&self, token: &str) -> Result<AgentConfig, ApiError> {
        let resp = Self::check(
            self.authed(self.http.get(self.url("/config")), token)
                .send()
                .await,
        )
        .await?;
        resp.json()
            .await
            .map_err(|e| ApiError::Network(e.without_url().to_string()))
    }

    pub async fn heartbeat(&self, token: &str, hb: &Heartbeat) -> Result<(), ApiError> {
        Self::check(
            self.authed(self.http.post(self.url("/heartbeat")), token)
                .json(hb)
                .send()
                .await,
        )
        .await
        .map(|_| ())
    }

    /// Sends a report, gzip-compressed. Returns `applied` or `unchanged`.
    pub async fn report(&self, token: &str, report: &Report) -> Result<String, ApiError> {
        let json = serde_json::to_vec(report).map_err(|e| ApiError::Network(e.to_string()))?;
        let mut enc = GzEncoder::new(Vec::new(), Compression::default());
        enc.write_all(&json)
            .map_err(|e| ApiError::Network(e.to_string()))?;
        let body = enc.finish().map_err(|e| ApiError::Network(e.to_string()))?;
        let resp = Self::check(
            self.authed(self.http.post(self.url("/reports")), token)
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::CONTENT_ENCODING, "gzip")
                .body(body)
                .send()
                .await,
        )
        .await?;
        let result: ReportResult = resp
            .json()
            .await
            .map_err(|e| ApiError::Network(e.without_url().to_string()))?;
        Ok(result.result)
    }

    /// Unpairs this agent on the server.
    pub async fn unpair(&self, token: &str) -> Result<(), ApiError> {
        Self::check(
            self.authed(self.http.delete(self.url("/self")), token)
                .send()
                .await,
        )
        .await
        .map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_addresses_are_normalized() {
        assert_eq!(
            parse_server("games.example.com/").expect("ok"),
            ("https://games.example.com".to_string(), false)
        );
        assert_eq!(
            parse_server("http://192.0.2.10:3000").expect("ok"),
            ("http://192.0.2.10:3000".to_string(), true)
        );
        assert!(parse_server("ftp://example.com").is_err());
    }
}
