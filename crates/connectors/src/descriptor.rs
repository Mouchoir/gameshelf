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

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AuthMethod {
    ApiKey {
        fields: &'static [KeyField],
        help_url: &'static str,
    },
    BrowserLogin {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct ConnectorDescriptor {
    pub platform: &'static str,
    pub tier: Tier,
    pub capabilities: Capabilities,
    pub auth: &'static [AuthMethod],
    pub default_rate: RateLimit,
    pub multi_account: bool,
}
