//! Steam connector (official Web API, research section 1).
//!
//! Credential payload: `{"steam_id": "<SteamID64, profile URL or custom URL name>", "api_key": "<Web API key>"}`.
//! A private profile can only be read with that account's own key.

use crate::{
    AccountIdentity, AccountRef, AchievementItem, Artwork, AuthMethod, Connector, ConnectorDescriptor,
    ConnectorError, Context, CredentialPayload, EnrichmentItem, IdSource, KeyField, LibraryItem,
    LibrarySnapshot, descriptor::RateLimit,
};
use async_trait::async_trait;
use gameshelf_core::{Acquisition, ProductKind, platform};
use serde::Deserialize;
use std::time::Duration;
use time::OffsetDateTime;

const API_BASE: &str = "https://api.steampowered.com";
const CDN_BASE: &str = "https://cdn.cloudflare.steamstatic.com/steam/apps";

/// Lowest valid SteamID64 for individual accounts.
const STEAMID64_BASE: u64 = 76_561_197_960_265_728;

static FIELDS: [KeyField; 2] = [
    KeyField {
        name: "steam_id",
        label_key: "connect.steam.steam_id",
        secret: false,
    },
    KeyField {
        name: "api_key",
        label_key: "connect.steam.api_key",
        secret: true,
    },
];

static AUTH: [AuthMethod; 1] = [AuthMethod::ApiKey {
    fields: &FIELDS,
    help_url: "https://steamcommunity.com/dev/apikey",
}];

static DESCRIPTOR: std::sync::LazyLock<ConnectorDescriptor> = std::sync::LazyLock::new(|| {
    let info = platform::platform("steam").copied();
    ConnectorDescriptor {
        platform: "steam",
        tier: info.map(|p| p.tier).unwrap_or(platform::Tier::Official),
        capabilities: info.map(|p| p.capabilities).unwrap_or_default(),
        auth: &AUTH,
        default_rate: RateLimit {
            requests: 10,
            per: Duration::from_secs(5),
        },
        multi_account: true,
    }
});

/// The Steam connector. Base URLs are configurable for tests.
#[derive(Clone, Debug)]
pub struct SteamConnector {
    api_base: String,
    cdn_base: String,
}

impl Default for SteamConnector {
    fn default() -> Self {
        Self {
            api_base: API_BASE.into(),
            cdn_base: CDN_BASE.into(),
        }
    }
}

impl SteamConnector {
    pub fn with_bases(api_base: impl Into<String>, cdn_base: impl Into<String>) -> Self {
        Self {
            api_base: api_base.into(),
            cdn_base: cdn_base.into(),
        }
    }

    fn key<'a>(&self, cred: &'a CredentialPayload) -> Result<&'a str, ConnectorError> {
        cred.str("api_key")
            .ok_or_else(|| ConnectorError::AuthInvalid("missing Web API key".into()))
    }

    /// Resolves the `steam_id` field (SteamID64, profile URL or custom URL name) to a SteamID64.
    async fn resolve_id(&self, cx: &Context, cred: &CredentialPayload) -> Result<String, ConnectorError> {
        let raw = cred
            .str("steam_id")
            .ok_or_else(|| ConnectorError::AuthInvalid("missing Steam account".into()))?;
        match parse_steam_ref(raw) {
            SteamRef::Id(id) => Ok(id),
            SteamRef::Vanity(name) => {
                let key = self.key(cred)?;
                let url = format!("{}/ISteamUser/ResolveVanityURL/v1/", self.api_base);
                let resp: Envelope<VanityResponse> = cx
                    .json(|| {
                        cx.http
                            .get(&url)
                            .query(&[("key", key), ("vanityurl", name.as_str())])
                    })
                    .await?;
                match (resp.response.success, resp.response.steamid) {
                    (1, Some(id)) => Ok(id),
                    _ => Err(ConnectorError::AuthInvalid(
                        "this Steam profile name does not exist".into(),
                    )),
                }
            }
            SteamRef::Invalid => Err(ConnectorError::AuthInvalid(
                "not a Steam account id or profile URL".into(),
            )),
        }
    }

    fn artwork(&self, appid: u64) -> Artwork {
        let base = &self.cdn_base;
        Artwork {
            cover: Some(format!("{base}/{appid}/library_600x900_2x.jpg")),
            header: Some(format!("{base}/{appid}/header.jpg")),
            hero: Some(format!("{base}/{appid}/library_hero.jpg")),
            logo: Some(format!("{base}/{appid}/logo.png")),
            icon: None,
        }
    }
}

/// What the player typed in the "Steam account" field.
#[derive(Debug, PartialEq, Eq)]
enum SteamRef {
    Id(String),
    Vanity(String),
    Invalid,
}

fn parse_steam_ref(input: &str) -> SteamRef {
    let s = input.trim().trim_end_matches('/');
    let tail = |marker: &str| {
        s.split(marker)
            .nth(1)
            .map(|t| t.split('/').next().unwrap_or(t).to_string())
    };
    if let Some(id) = tail("/profiles/") {
        return if is_steamid64(&id) {
            SteamRef::Id(id)
        } else {
            SteamRef::Invalid
        };
    }
    if let Some(name) = tail("/id/") {
        return if is_vanity(&name) {
            SteamRef::Vanity(name)
        } else {
            SteamRef::Invalid
        };
    }
    if is_steamid64(s) {
        SteamRef::Id(s.to_string())
    } else if is_vanity(s) {
        SteamRef::Vanity(s.to_string())
    } else {
        SteamRef::Invalid
    }
}

fn is_steamid64(s: &str) -> bool {
    s.len() == 17 && s.parse::<u64>().is_ok_and(|v| v >= STEAMID64_BASE)
}

fn is_vanity(s: &str) -> bool {
    (2..=32).contains(&s.len())
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

#[derive(Deserialize)]
struct Envelope<T> {
    response: T,
}

#[derive(Deserialize)]
struct VanityResponse {
    success: i32,
    steamid: Option<String>,
}

#[derive(Deserialize)]
struct SummariesResponse {
    #[serde(default)]
    players: Vec<Player>,
}

#[derive(Deserialize)]
struct Player {
    steamid: String,
    personaname: Option<String>,
}

#[derive(Deserialize)]
struct OwnedGamesResponse {
    game_count: Option<u32>,
    #[serde(default)]
    games: Vec<OwnedGame>,
}

#[derive(Deserialize)]
struct OwnedGame {
    appid: u64,
    name: Option<String>,
    #[serde(default)]
    playtime_forever: u32,
    #[serde(default)]
    rtime_last_played: i64,
}

#[derive(Deserialize)]
struct LevelResponse {
    player_level: Option<u32>,
}

#[derive(Deserialize)]
struct PlayerStatsEnvelope {
    playerstats: PlayerStats,
}

#[derive(Deserialize)]
struct PlayerStats {
    #[serde(default)]
    success: bool,
    #[serde(default)]
    achievements: Vec<PlayerAchievement>,
}

#[derive(Deserialize)]
struct PlayerAchievement {
    achieved: u8,
}

#[async_trait]
impl Connector for SteamConnector {
    fn descriptor(&self) -> &'static ConnectorDescriptor {
        &DESCRIPTOR
    }

    async fn validate(
        &self,
        cx: &Context,
        cred: &CredentialPayload,
    ) -> Result<AccountIdentity, ConnectorError> {
        let key = self.key(cred)?;
        let steam_id = self.resolve_id(cx, cred).await?;
        let url = format!("{}/ISteamUser/GetPlayerSummaries/v2/", self.api_base);
        let resp: Envelope<SummariesResponse> = cx
            .json(|| {
                cx.http
                    .get(&url)
                    .query(&[("key", key), ("steamids", steam_id.as_str())])
            })
            .await
            .map_err(|e| match e {
                ConnectorError::AuthExpired => {
                    ConnectorError::AuthInvalid("the Web API key was refused".into())
                }
                other => other,
            })?;
        let player = resp
            .response
            .players
            .into_iter()
            .find(|p| p.steamid == steam_id)
            .ok_or_else(|| ConnectorError::AuthInvalid("no Steam account with this id".into()))?;
        Ok(AccountIdentity {
            external_account_id: steam_id,
            display_name: player.personaname,
        })
    }

    async fn library(
        &self,
        cx: &Context,
        account: &AccountRef,
        cred: &CredentialPayload,
    ) -> Result<LibrarySnapshot, ConnectorError> {
        let key = self.key(cred)?;
        let url = format!("{}/IPlayerService/GetOwnedGames/v1/", self.api_base);
        let steam_id = account.external_account_id.as_str();
        let resp: Envelope<OwnedGamesResponse> = cx
            .json(|| {
                cx.http.get(&url).query(&[
                    ("key", key),
                    ("steamid", steam_id),
                    ("include_appinfo", "1"),
                    ("include_played_free_games", "1"),
                    ("include_free_sub", "1"),
                    ("skip_unvetted_apps", "0"),
                ])
            })
            .await?;
        if resp.response.game_count.is_none() {
            return Err(ConnectorError::Permanent(
                "the library is not visible: make this profile's game details public or use this account's own \
                 Web API key"
                    .into(),
            ));
        }
        let mut items = Vec::with_capacity(resp.response.games.len());
        let mut complete = true;
        for game in resp.response.games {
            let Some(title) = game.name.filter(|n| !n.trim().is_empty()) else {
                complete = false;
                continue;
            };
            let last_played = (game.rtime_last_played > 0)
                .then(|| OffsetDateTime::from_unix_timestamp(game.rtime_last_played).ok())
                .flatten();
            items.push(LibraryItem {
                platform_game_id: game.appid.to_string(),
                title,
                kind: ProductKind::Game,
                parent_platform_game_id: None,
                edition: None,
                acquisition: Acquisition::Purchase,
                playtime_minutes: Some(game.playtime_forever),
                last_played_at: last_played,
                cross_ids: vec![(IdSource::Steam, game.appid.to_string())],
                artwork: self.artwork(game.appid),
                raw: serde_json::json!({ "appid": game.appid }),
            });
        }
        Ok(LibrarySnapshot { complete, items })
    }

    async fn achievements(
        &self,
        cx: &Context,
        account: &AccountRef,
        cred: &CredentialPayload,
        ids: &[String],
    ) -> Result<Vec<AchievementItem>, ConnectorError> {
        let key = self.key(cred)?;
        let url = format!("{}/ISteamUserStats/GetPlayerAchievements/v1/", self.api_base);
        let mut out = Vec::new();
        for appid in ids {
            let result: Result<PlayerStatsEnvelope, ConnectorError> = cx
                .json(|| {
                    cx.http.get(&url).query(&[
                        ("key", key),
                        ("steamid", account.external_account_id.as_str()),
                        ("appid", appid.as_str()),
                    ])
                })
                .await;
            match result {
                Ok(stats) if stats.playerstats.success && !stats.playerstats.achievements.is_empty() => {
                    let total = u32::try_from(stats.playerstats.achievements.len()).unwrap_or(u32::MAX);
                    let unlocked = u32::try_from(
                        stats
                            .playerstats
                            .achievements
                            .iter()
                            .filter(|a| a.achieved == 1)
                            .count(),
                    )
                    .unwrap_or(0);
                    out.push(AchievementItem {
                        platform_game_id: appid.clone(),
                        unlocked,
                        total,
                    });
                }
                // Games without stats answer 400: skip them, they have no achievements.
                Ok(_) | Err(ConnectorError::Permanent(_)) => {}
                Err(e) => return Err(e),
            }
        }
        Ok(out)
    }

    async fn enrich(
        &self,
        cx: &Context,
        account: &AccountRef,
        cred: &CredentialPayload,
    ) -> Result<Vec<EnrichmentItem>, ConnectorError> {
        let key = self.key(cred)?;
        let url = format!("{}/IPlayerService/GetSteamLevel/v1/", self.api_base);
        let resp: Envelope<LevelResponse> = cx
            .json(|| {
                cx.http
                    .get(&url)
                    .query(&[("key", key), ("steamid", account.external_account_id.as_str())])
            })
            .await?;
        Ok(resp
            .response
            .player_level
            .map(|level| EnrichmentItem {
                platform_game_id: None,
                key: "platform_level".into(),
                value: serde_json::json!(level),
                unit: None,
            })
            .into_iter()
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steam_references_are_parsed() {
        assert_eq!(
            parse_steam_ref("76561202255233001"),
            SteamRef::Id("76561202255233001".into())
        );
        assert_eq!(
            parse_steam_ref("https://steamcommunity.com/profiles/76561202255233001/"),
            SteamRef::Id("76561202255233001".into())
        );
        assert_eq!(
            parse_steam_ref("https://steamcommunity.com/id/example-user/"),
            SteamRef::Vanity("example-user".into())
        );
        assert_eq!(
            parse_steam_ref("example_user"),
            SteamRef::Vanity("example_user".into())
        );
        assert_eq!(parse_steam_ref("12345"), SteamRef::Vanity("12345".into()));
        assert_eq!(parse_steam_ref("not a profile!"), SteamRef::Invalid);
        assert_eq!(
            parse_steam_ref("https://steamcommunity.com/profiles/123/"),
            SteamRef::Invalid
        );
    }
}
