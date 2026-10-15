//! Normalized items produced by connectors and local sources (contracts/connector.md section 2).

use gameshelf_core::{Acquisition, ProductKind};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

/// A connector's credential payload (key, tokens, cookies) as JSON. Never printed.
#[derive(Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CredentialPayload(pub serde_json::Value);

impl std::fmt::Debug for CredentialPayload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CredentialPayload(..)")
    }
}

impl CredentialPayload {
    pub fn from_json(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(bytes).map(Self)
    }

    /// A string field of the payload.
    pub fn str(&self, key: &str) -> Option<&str> {
        self.0
            .get(key)
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
    }
}

/// The account a credential belongs to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountIdentity {
    pub external_account_id: String,
    pub display_name: Option<String>,
}

/// The account being read.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountRef {
    pub external_account_id: String,
    pub display_name: Option<String>,
}

/// A rotated credential to persist before any other call.
#[derive(Clone, Debug)]
pub struct Refreshed {
    pub payload: CredentialPayload,
    pub expires_at: Option<OffsetDateTime>,
}

/// URLs offered by the platform for a product.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Artwork {
    pub cover: Option<String>,
    pub header: Option<String>,
    pub hero: Option<String>,
    pub logo: Option<String>,
    pub icon: Option<String>,
}

impl Artwork {
    pub fn is_empty(&self) -> bool {
        self.cover.is_none()
            && self.header.is_none()
            && self.hero.is_none()
            && self.logo.is_none()
            && self.icon.is_none()
    }
}

/// Identifier sources for cross-platform matching.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdSource {
    Steam,
    Gog,
    Epic,
    Xbox,
    Itch,
    Igdb,
    Wikidata,
}

impl IdSource {
    pub fn as_str(self) -> &'static str {
        match self {
            IdSource::Steam => "steam",
            IdSource::Gog => "gog",
            IdSource::Epic => "epic",
            IdSource::Xbox => "xbox",
            IdSource::Itch => "itch",
            IdSource::Igdb => "igdb",
            IdSource::Wikidata => "wikidata",
        }
    }
}

/// One product of an account's library.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryItem {
    pub platform_game_id: String,
    pub title: String,
    pub kind: ProductKind,
    pub parent_platform_game_id: Option<String>,
    pub edition: Option<String>,
    pub acquisition: Acquisition,
    /// `None` means not provided; never `Some(0)` as a placeholder.
    pub playtime_minutes: Option<u32>,
    pub last_played_at: Option<OffsetDateTime>,
    pub cross_ids: Vec<(IdSource, String)>,
    pub artwork: Artwork,
    /// Trimmed payload kept for re-matching; never personal data.
    pub raw: serde_json::Value,
}

/// An account's library.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibrarySnapshot {
    /// True only if every page was read without error.
    pub complete: bool,
    pub items: Vec<LibraryItem>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AchievementItem {
    pub platform_game_id: String,
    pub unlocked: u32,
    pub total: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EnrichmentItem {
    /// `None` for platform-level values such as an account level.
    pub platform_game_id: Option<String>,
    pub key: String,
    pub value: serde_json::Value,
    pub unit: Option<String>,
}
