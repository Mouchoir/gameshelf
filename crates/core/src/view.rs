//! Read models shared by the server and the browser: library tiles, query, game detail rows.

use crate::{AccountId, Acquisition, GameId, OwnershipId, ProfileId};
use time::OffsetDateTime;

/// Sort orders of the library (FR-014).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sort {
    #[default]
    Title,
    Added,
    LastPlayed,
    Playtime,
}

/// Filters and paging of the library (FR-014). Empty lists mean "no filter".
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LibraryQuery {
    pub platforms: Vec<String>,
    pub accounts: Vec<AccountId>,
    pub owners: Vec<ProfileId>,
    pub acquisitions: Vec<Acquisition>,
    /// Restrict to these games (search results).
    pub games: Option<Vec<GameId>>,
    pub include_subscription_only: bool,
    pub include_unavailable: bool,
    pub include_free_to_play: bool,
    pub include_tools: bool,
    pub installed_only: bool,
    pub tag: Option<String>,
    pub sort: Sort,
    pub descending: bool,
    pub offset: u32,
    pub limit: u32,
}

impl Default for LibraryQuery {
    fn default() -> Self {
        Self {
            platforms: Vec::new(),
            accounts: Vec::new(),
            owners: Vec::new(),
            acquisitions: Vec::new(),
            games: None,
            include_subscription_only: false,
            include_unavailable: false,
            include_free_to_play: true,
            include_tools: false,
            installed_only: false,
            tag: None,
            sort: Sort::Title,
            descending: false,
            offset: 0,
            limit: 120,
        }
    }
}

/// One tile of the library grid.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LibraryGame {
    pub game_id: GameId,
    pub title: String,
    pub cover: Option<String>,
    pub platforms: Vec<String>,
    /// Visible accounts holding the game (all profiles).
    pub account_count: u32,
    /// Accounts of the viewer holding the game.
    pub own_account_count: u32,
    /// Other profiles whose shared libraries hold the game.
    pub other_owners: Vec<ProfileId>,
    pub acquisitions: Vec<Acquisition>,
    /// True when at least one visible ownership is a real copy (not subscription or family sharing).
    pub owned: bool,
    pub added_at: Option<OffsetDateTime>,
    pub last_played_at: Option<OffsetDateTime>,
    pub playtime_minutes: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LibraryPage {
    pub games: Vec<LibraryGame>,
    pub total: u32,
}

/// One ownership as shown in a game's detail.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct VisibleOwnership {
    pub id: OwnershipId,
    pub account_id: AccountId,
    pub account_label: String,
    pub platform: String,
    pub owner_profile_id: ProfileId,
    pub mine: bool,
    pub product_title: String,
    pub platform_game_id: String,
    pub product_kind: String,
    pub acquisition: Acquisition,
    pub edition: Option<String>,
    pub first_seen_at: OffsetDateTime,
    pub unavailable_since: Option<OffsetDateTime>,
    pub playtime_minutes: Option<u32>,
    pub last_played_at: Option<OffsetDateTime>,
    pub achievements: Option<(u32, u32)>,
}

/// Facts about a game shown on its detail page.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct GameSummary {
    pub id: GameId,
    pub title: String,
    pub release_year: Option<i32>,
    pub kind: String,
    pub parent: Option<GameId>,
    pub description: Option<String>,
    pub cover: Option<String>,
}
