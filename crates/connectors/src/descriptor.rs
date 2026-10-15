//! Static description of a connector, shown before connection (FR-002, contracts/connector.md section 1).

use gameshelf_core::platform::{Capabilities, Tier};
use std::time::Duration;

/// A field the player fills to connect an account with keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct KeyField {
    /// Form field name and payload key.
    pub name: &'static str,
    /// Localization key of the label.
    pub label_key: &'static str,
    /// Hidden input (password-like).
    pub secret: bool,
}

/// Where the result of a browser login appears (contracts/connector.md section 3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CodeCapture {
    /// The code is a query parameter of the redirect URL.
    RedirectQuery { param: &'static str },
    /// The final page prints JSON containing the code.
    JsonPage { field: &'static str },
    /// The platform redirects to a loopback address.
    LocalCallback { port: u16 },
}

impl CodeCapture {
    /// Extracts the authorization code from what the player pasted: the final page's URL, its JSON, or the
    /// bare code. Returns `None` when nothing usable is found.
    pub fn extract(&self, pasted: &str) -> Option<String> {
        let text = pasted.trim();
        if text.is_empty() {
            return None;
        }
        let from_query = |param: &str| -> Option<String> {
            let url = url::Url::parse(text).ok()?;
            url.query_pairs()
                .find(|(k, _)| k == param)
                .map(|(_, v)| v.into_owned())
        };
        let found = match self {
            CodeCapture::RedirectQuery { param } => from_query(param),
            CodeCapture::JsonPage { field } => serde_json::from_str::<serde_json::Value>(text)
                .ok()
                .and_then(|v| v.get(*field).and_then(|c| c.as_str()).map(str::to_string)),
            CodeCapture::LocalCallback { .. } => from_query("code"),
        };
        found.or_else(|| {
            // A bare code: no spaces, quotes or URL punctuation.
            let bare = text
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
            (bare && text.len() >= 8).then(|| text.to_string())
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AuthMethod {
    ApiKey {
        fields: &'static [KeyField],
        help_url: &'static str,
    },
    /// The player signs in on the platform's own page, then the code is captured by the agent or pasted.
    BrowserLogin {
        login_url: &'static str,
        capture: CodeCapture,
    },
    SessionCookie {
        cookie_names: &'static [&'static str],
        help_url: &'static str,
    },
    AgentOnly,
}

/// Requests allowed per period, with a burst.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct RateLimit {
    pub requests: u32,
    pub per: Duration,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_extracted_from_urls_json_or_bare_text() {
        let q = CodeCapture::RedirectQuery { param: "code" };
        assert_eq!(
            q.extract("https://embed.gog.com/on_login_success?origin=client&code=AbC123xyz_-"),
            Some("AbC123xyz_-".into())
        );
        assert_eq!(q.extract("  AbC123xyz456  "), Some("AbC123xyz456".into()));
        assert_eq!(q.extract("https://example.com/?nothing=here"), None);
        assert_eq!(q.extract("short"), None);

        let j = CodeCapture::JsonPage {
            field: "authorizationCode",
        };
        assert_eq!(
            j.extract(r#"{"redirectUrl":"https://x","authorizationCode":"0123456789abcdef","sid":null}"#),
            Some("0123456789abcdef".into())
        );
        assert_eq!(j.extract("0123456789abcdef"), Some("0123456789abcdef".into()));
        assert_eq!(j.extract("not a code at all"), None);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct ConnectorDescriptor {
    pub platform: &'static str,
    pub tier: Tier,
    pub capabilities: Capabilities,
    pub auth: &'static [AuthMethod],
    pub default_rate: RateLimit,
    pub multi_account: bool,
}
