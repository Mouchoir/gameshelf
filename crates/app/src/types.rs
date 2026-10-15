//! Data exchanged between server functions and the browser.

use gameshelf_core::{
    AccountId, GameId, LocalePref, ProfileId, Role,
    account_status::AccountStatus,
    platform::Tier,
    view::{GameSummary, LibraryQuery, VisibleOwnership},
};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

/// The signed-in profile.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Me {
    pub id: ProfileId,
    pub username: String,
    pub display_name: String,
    pub role: Role,
    pub locale: LocalePref,
}

/// What the shell needs to decide where to send the visitor.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthState {
    pub me: Option<Me>,
    pub needs_setup: bool,
}

/// A form field of a key-based connection.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldView {
    pub name: String,
    pub label_key: String,
    pub secret: bool,
}

/// A platform as shown in the platform picker (FR-002).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlatformView {
    pub key: String,
    pub name: String,
    pub tier: Tier,
    pub icon: String,
    /// A connector is available on this instance.
    pub connectable: bool,
    /// Only the agent can read it.
    pub agent_only: bool,
    pub fields: Vec<FieldView>,
    pub help_url: Option<String>,
    /// `api_key`, `browser_login` or `none`.
    pub auth_kind: String,
    /// Sign-in page of a browser login.
    pub login_url: Option<String>,
    /// What the player pastes after a browser login: `json_page` or `redirect_query`.
    pub capture: Option<String>,
}

/// A connected account as shown on the Accounts page.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountView {
    pub id: AccountId,
    pub platform: String,
    pub platform_name: String,
    pub label: String,
    pub display_name: Option<String>,
    pub status: AccountStatus,
    pub games: i64,
    pub last_sync_at: Option<OffsetDateTime>,
    pub next_sync_at: Option<OffsetDateTime>,
    /// When the stored credential stops working, when the platform says so.
    pub credential_expires_at: Option<OffsetDateTime>,
    /// The credential expires within 7 days (FR-023).
    pub credential_expiring: bool,
    pub last_error: Option<String>,
    pub syncing: bool,
}

/// One row of a game's ownership list, with the owner's name when it is another profile.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnershipRow {
    pub ownership: VisibleOwnership,
    pub owner_name: Option<String>,
}

/// A per-account value shown in a game's detail.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EnrichmentRow {
    pub account_id: AccountId,
    pub key: String,
    pub value: String,
    pub refreshed_at: OffsetDateTime,
}

/// Everything the game page shows.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GameDetail {
    /// The viewer hides this game from the profiles it shares with.
    pub hidden_from_family: bool,
    pub summary: GameSummary,
    pub ownerships: Vec<OwnershipRow>,
    pub dlc: Vec<GameSummary>,
    pub enrichment: Vec<EnrichmentRow>,
}

/// A library request: filters plus an optional search.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryRequest {
    pub query: LibraryQuery,
    pub search: String,
}

/// One activity feed entry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActivityRow {
    pub kind: String,
    pub game_id: Option<GameId>,
    pub game_title: Option<String>,
    pub account_label: Option<String>,
    pub platform: Option<String>,
    pub created_at: OffsetDateTime,
}

/// A notification target as shown on the Notifications page (secrets never leave the server).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetView {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub events: Vec<String>,
    pub enabled: bool,
    pub last_sent_at: Option<OffsetDateTime>,
    pub last_error: Option<String>,
}

/// Result of a connection attempt.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConnectResult {
    Connected(AccountId),
    Rejected(String),
    Taken(String),
    Unknown,
    NoCode,
}

/// A profile as shown to administrators.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileAdminView {
    pub id: ProfileId,
    pub username: String,
    pub display_name: String,
    pub role: Role,
    pub disabled: bool,
}

/// Another profile on the Sharing page.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudienceView {
    pub id: ProfileId,
    pub display_name: String,
    pub shared: bool,
}

/// A platform or account that can be hidden from shared views.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HideablePlatform {
    pub key: String,
    pub name: String,
    pub hidden: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HideableAccount {
    pub id: AccountId,
    pub platform: String,
    pub label: String,
    pub hidden: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HiddenGame {
    pub id: GameId,
    pub title: String,
}

/// Everything the Sharing page shows.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharingView {
    pub others: Vec<AudienceView>,
    pub platforms: Vec<HideablePlatform>,
    pub accounts: Vec<HideableAccount>,
    pub hidden_games: Vec<HiddenGame>,
}
