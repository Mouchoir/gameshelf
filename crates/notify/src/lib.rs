//! Notification senders (contracts/notifications.md, research D12).
//!
//! A target is one destination (a Discord webhook, an ntfy topic, a Gotify application). Messages never carry
//! credentials, tokens or install paths. Each send is independent: one failing target never blocks another.

pub mod text;

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Kinds of targets besides the in-app feed, which is always on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetKind {
    Discord,
    Ntfy,
    Gotify,
}

impl TargetKind {
    pub fn as_str(self) -> &'static str {
        match self {
            TargetKind::Discord => "discord",
            TargetKind::Ntfy => "ntfy",
            TargetKind::Gotify => "gotify",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "discord" => Some(TargetKind::Discord),
            "ntfy" => Some(TargetKind::Ntfy),
            "gotify" => Some(TargetKind::Gotify),
            _ => None,
        }
    }
}

/// Where and how to send. Stored sealed: URLs and tokens are secrets.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TargetConfig {
    Discord {
        webhook_url: String,
    },
    Ntfy {
        server: String,
        topic: String,
        token: Option<String>,
    },
    Gotify {
        server: String,
        token: String,
    },
}

impl std::fmt::Debug for TargetConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            TargetConfig::Discord { .. } => "TargetConfig::Discord(..)",
            TargetConfig::Ntfy { .. } => "TargetConfig::Ntfy(..)",
            TargetConfig::Gotify { .. } => "TargetConfig::Gotify(..)",
        })
    }
}

impl TargetConfig {
    pub fn kind(&self) -> TargetKind {
        match self {
            TargetConfig::Discord { .. } => TargetKind::Discord,
            TargetConfig::Ntfy { .. } => TargetKind::Ntfy,
            TargetConfig::Gotify { .. } => TargetKind::Gotify,
        }
    }

    /// Checks URLs and required fields before saving.
    pub fn validate(&self) -> Result<(), NotifyError> {
        let http = |s: &str| {
            url::Url::parse(s.trim())
                .ok()
                .filter(|u| matches!(u.scheme(), "http" | "https") && u.host_str().is_some())
                .ok_or(NotifyError::Invalid("the address must be an http or https URL"))
        };
        match self {
            TargetConfig::Discord { webhook_url } => {
                let u = http(webhook_url)?;
                if !u.path().contains("/api/webhooks/") {
                    return Err(NotifyError::Invalid("this is not a Discord webhook URL"));
                }
            }
            TargetConfig::Ntfy { server, topic, .. } => {
                http(server)?;
                if topic.trim().is_empty() || topic.contains('/') || topic.len() > 64 {
                    return Err(NotifyError::Invalid(
                        "the topic must be 1 to 64 characters without '/'",
                    ));
                }
            }
            TargetConfig::Gotify { server, token } => {
                http(server)?;
                if token.trim().is_empty() {
                    return Err(NotifyError::Invalid("the application token is required"));
                }
            }
        }
        Ok(())
    }
}

/// How urgent a message is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Priority {
    Normal,
    High,
}

/// A rendered message.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub title: String,
    pub body: String,
    /// Link opened when the notification is clicked.
    pub url: Option<String>,
    pub priority: Priority,
}

#[derive(Debug, thiserror::Error)]
pub enum NotifyError {
    #[error("{0}")]
    Invalid(&'static str),
    #[error("the target refused the message (status {0})")]
    Status(u16),
    #[error("the target could not be reached: {0}")]
    Transport(String),
}

impl From<reqwest::Error> for NotifyError {
    fn from(e: reqwest::Error) -> Self {
        NotifyError::Transport(e.without_url().to_string())
    }
}

/// Sends a message to a target, with two retries for transient failures (network, 429, 5xx).
pub async fn send(http: &reqwest::Client, target: &TargetConfig, msg: &Message) -> Result<(), NotifyError> {
    let mut last = NotifyError::Transport("not attempted".into());
    for attempt in 0..3u64 {
        if attempt > 0 {
            tokio::time::sleep(Duration::from_millis(500 * attempt)).await;
        }
        match send_once(http, target, msg).await {
            Ok(()) => return Ok(()),
            Err(e @ (NotifyError::Transport(_) | NotifyError::Status(429))) => last = e,
            Err(NotifyError::Status(s)) if s >= 500 => last = NotifyError::Status(s),
            Err(e) => return Err(e),
        }
    }
    Err(last)
}

async fn send_once(http: &reqwest::Client, target: &TargetConfig, msg: &Message) -> Result<(), NotifyError> {
    let req = match target {
        TargetConfig::Discord { webhook_url } => {
            let mut embed = serde_json::json!({
                "title": msg.title,
                "description": truncate(&msg.body, 4000),
            });
            if let Some(u) = &msg.url {
                embed["url"] = serde_json::json!(u);
            }
            http.post(webhook_url.trim())
                .json(&serde_json::json!({ "username": "gameshelf", "embeds": [embed] }))
        }
        TargetConfig::Ntfy { server, topic, token } => {
            let url = format!("{}/{}", server.trim().trim_end_matches('/'), topic.trim());
            let mut req = http
                .post(url)
                .header("Title", ascii_header(&msg.title))
                .header("Tags", "video_game")
                .header(
                    "Priority",
                    match msg.priority {
                        Priority::Normal => "default",
                        Priority::High => "high",
                    },
                )
                .body(msg.body.clone());
            if let Some(u) = &msg.url {
                req = req.header("Click", u.as_str());
            }
            if let Some(t) = token.as_deref().filter(|t| !t.trim().is_empty()) {
                req = req.bearer_auth(t.trim());
            }
            req
        }
        TargetConfig::Gotify { server, token } => {
            let url = format!("{}/message", server.trim().trim_end_matches('/'));
            let mut body = serde_json::json!({
                "title": msg.title,
                "message": msg.body,
                "priority": match msg.priority { Priority::Normal => 5, Priority::High => 8 },
            });
            if let Some(u) = &msg.url {
                body["extras"] = serde_json::json!({ "client::notification": { "click": { "url": u } } });
            }
            http.post(url).header("X-Gotify-Key", token.trim()).json(&body)
        }
    };
    let resp = req.timeout(Duration::from_secs(15)).send().await?;
    let status = resp.status();
    if status.is_success() {
        Ok(())
    } else {
        Err(NotifyError::Status(status.as_u16()))
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max - 3).collect();
        out.push_str("...");
        out
    }
}

/// HTTP headers must be ASCII: ntfy accepts RFC 2047 encoded words for anything else.
fn ascii_header(s: &str) -> String {
    if s.is_ascii() {
        return s.to_string();
    }
    let mut encoded = String::from("=?UTF-8?B?");
    encoded.push_str(&base64_encode(s.as_bytes()));
    encoded.push_str("?=");
    encoded
}

fn base64_encode(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            T[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configs_are_validated() {
        assert!(
            TargetConfig::Discord {
                webhook_url: "https://discord.com/api/webhooks/1/abc".into()
            }
            .validate()
            .is_ok()
        );
        assert!(
            TargetConfig::Discord {
                webhook_url: "https://example.com/x".into()
            }
            .validate()
            .is_err()
        );
        assert!(
            TargetConfig::Ntfy {
                server: "https://ntfy.sh".into(),
                topic: "games".into(),
                token: None
            }
            .validate()
            .is_ok()
        );
        assert!(
            TargetConfig::Ntfy {
                server: "ftp://x".into(),
                topic: "games".into(),
                token: None
            }
            .validate()
            .is_err()
        );
        assert!(
            TargetConfig::Ntfy {
                server: "https://ntfy.sh".into(),
                topic: "a/b".into(),
                token: None
            }
            .validate()
            .is_err()
        );
        assert!(
            TargetConfig::Gotify {
                server: "https://gotify.example.com".into(),
                token: "".into()
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn debug_hides_urls_and_tokens() {
        let c = TargetConfig::Ntfy {
            server: "https://ntfy.example.com".into(),
            topic: "t".into(),
            token: Some("SECRET".into()),
        };
        assert!(!format!("{c:?}").contains("SECRET"));
    }

    #[test]
    fn non_ascii_titles_are_encoded() {
        assert_eq!(ascii_header("3 new games"), "3 new games");
        assert_eq!(ascii_header("é"), "=?UTF-8?B?w6k=?=");
    }
}
