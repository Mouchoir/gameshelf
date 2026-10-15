//! GOG connector (unofficial API used by GOG Galaxy, research section 1).
//!
//! Sign-in uses Galaxy's public OAuth client, as open-source launchers do. The player signs in on gog.com and
//! lands on `embed.gog.com/on_login_success?code=...`; the code is exchanged here for rolling tokens.
//! Credential payload: `{"access_token", "refresh_token", "user_id"}`.

use crate::{
    AccountIdentity, AccountRef, Artwork, AuthMethod, CodeCapture, Connector, ConnectorDescriptor,
    ConnectorError, Context, CredentialPayload, IdSource, LibraryItem, LibrarySnapshot, Refreshed,
    descriptor::RateLimit,
};
use async_trait::async_trait;
use gameshelf_core::{Acquisition, ProductKind, platform};
use serde::Deserialize;
use std::{collections::HashMap, time::Duration};

/// Public client of GOG Galaxy (also used by open-source launchers).
const CLIENT_ID: &str = "46899977096215655";
const CLIENT_SECRET: &str = "9d85c43b1482497dbbce61f6e4aa173a433796eeae2ca8c5f6129f2dc4de46d9";
const REDIRECT_URI: &str = "https://embed.gog.com/on_login_success?origin=client";
const AUTH_BASE: &str = "https://auth.gog.com";
const EMBED_BASE: &str = "https://embed.gog.com";
const API_BASE: &str = "https://api.gog.com";
/// Product ids requested per call.
const PRODUCT_BATCH: usize = 50;

static AUTH: [AuthMethod; 1] = [AuthMethod::BrowserLogin {
    login_url: "https://auth.gog.com/auth?client_id=46899977096215655&redirect_uri=https%3A%2F%2Fembed.gog.com%2Fon_login_success%3Forigin%3Dclient&response_type=code&layout=client2",
    capture: CodeCapture::RedirectQuery { param: "code" },
}];

static DESCRIPTOR: std::sync::LazyLock<ConnectorDescriptor> = std::sync::LazyLock::new(|| {
    let info = platform::platform("gog").copied();
    ConnectorDescriptor {
        platform: "gog",
        tier: info.map(|p| p.tier).unwrap_or(platform::Tier::Unofficial),
        capabilities: info.map(|p| p.capabilities).unwrap_or_default(),
        auth: &AUTH,
        default_rate: RateLimit {
            requests: 10,
            per: Duration::from_secs(2),
        },
        multi_account: true,
    }
});

#[derive(Clone, Debug)]
pub struct GogConnector {
    auth_base: String,
    embed_base: String,
    api_base: String,
}

impl Default for GogConnector {
    fn default() -> Self {
        Self {
            auth_base: AUTH_BASE.into(),
            embed_base: EMBED_BASE.into(),
            api_base: API_BASE.into(),
        }
    }
}

impl GogConnector {
    /// Uses one base URL for every GOG service, for tests.
    pub fn with_base(base: impl Into<String>) -> Self {
        let base = base.into();
        Self {
            auth_base: base.clone(),
            embed_base: base.clone(),
            api_base: base,
        }
    }

    fn token<'a>(&self, cred: &'a CredentialPayload) -> Result<&'a str, ConnectorError> {
        cred.str("access_token").ok_or(ConnectorError::AuthExpired)
    }

    async fn token_request(&self, cx: &Context, grant: &[(&str, &str)]) -> Result<Refreshed, ConnectorError> {
        let url = format!("{}/token", self.auth_base);
        let mut query: Vec<(&str, &str)> = vec![("client_id", CLIENT_ID), ("client_secret", CLIENT_SECRET)];
        query.extend_from_slice(grant);
        let resp: TokenResponse = cx
            .json(|| cx.http.get(&url).query(&query))
            .await
            .map_err(|e| match e {
                ConnectorError::Permanent(_) => ConnectorError::AuthExpired,
                other => other,
            })?;
        Ok(Refreshed {
            payload: CredentialPayload(serde_json::json!({
                "access_token": resp.access_token,
                "refresh_token": resp.refresh_token,
                "user_id": resp.user_id,
            })),
            expires_at: None,
        })
    }
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    user_id: Option<String>,
}

#[derive(Deserialize)]
struct UserData {
    #[serde(rename = "isLoggedIn", default)]
    is_logged_in: bool,
    #[serde(rename = "userId")]
    user_id: Option<String>,
    username: Option<String>,
}

#[derive(Deserialize)]
struct Owned {
    #[serde(default)]
    owned: Vec<u64>,
}

#[derive(Deserialize)]
struct Product {
    id: u64,
    title: Option<String>,
    game_type: Option<String>,
    images: Option<Images>,
}

#[derive(Deserialize)]
struct Images {
    background: Option<String>,
    logo2x: Option<String>,
    icon: Option<String>,
}

fn absolute(url: Option<String>) -> Option<String> {
    url.filter(|u| !u.is_empty()).map(|u| {
        if u.starts_with("//") {
            format!("https:{u}")
        } else {
            u
        }
    })
}

#[async_trait]
impl Connector for GogConnector {
    fn descriptor(&self) -> &'static ConnectorDescriptor {
        &DESCRIPTOR
    }

    async fn exchange_code(&self, cx: &Context, code: &str) -> Result<Refreshed, ConnectorError> {
        self.token_request(
            cx,
            &[
                ("grant_type", "authorization_code"),
                ("code", code),
                ("redirect_uri", REDIRECT_URI),
            ],
        )
        .await
        .map_err(|e| match e {
            ConnectorError::AuthExpired => {
                ConnectorError::AuthInvalid("the code was refused or has expired".into())
            }
            other => other,
        })
    }

    async fn refresh(
        &self,
        cx: &Context,
        cred: &CredentialPayload,
    ) -> Result<Option<Refreshed>, ConnectorError> {
        let refresh = cred.str("refresh_token").ok_or(ConnectorError::AuthExpired)?;
        self.token_request(cx, &[("grant_type", "refresh_token"), ("refresh_token", refresh)])
            .await
            .map(Some)
    }

    async fn validate(
        &self,
        cx: &Context,
        cred: &CredentialPayload,
    ) -> Result<AccountIdentity, ConnectorError> {
        let token = self
            .token(cred)
            .map_err(|_| ConnectorError::AuthInvalid("missing access token".into()))?;
        let url = format!("{}/userData.json", self.embed_base);
        let user: UserData = cx
            .json(|| cx.http.get(&url).bearer_auth(token))
            .await
            .map_err(|e| match e {
                ConnectorError::AuthExpired => ConnectorError::AuthInvalid("GOG refused the sign-in".into()),
                other => other,
            })?;
        match (
            user.is_logged_in,
            user.user_id.or_else(|| cred.str("user_id").map(str::to_string)),
        ) {
            (true, Some(id)) => Ok(AccountIdentity {
                external_account_id: id,
                display_name: user.username,
            }),
            _ => Err(ConnectorError::AuthInvalid("GOG refused the sign-in".into())),
        }
    }

    async fn library(
        &self,
        cx: &Context,
        _account: &AccountRef,
        cred: &CredentialPayload,
    ) -> Result<LibrarySnapshot, ConnectorError> {
        let token = self.token(cred)?;
        let url = format!("{}/user/data/games", self.embed_base);
        let owned: Owned = cx.json(|| cx.http.get(&url).bearer_auth(token)).await?;

        let products_url = format!("{}/products", self.api_base);
        let mut products: HashMap<u64, Product> = HashMap::new();
        let mut complete = true;
        for chunk in owned.owned.chunks(PRODUCT_BATCH) {
            let ids = chunk.iter().map(u64::to_string).collect::<Vec<_>>().join(",");
            match cx
                .json::<Vec<Product>, _>(|| cx.http.get(&products_url).query(&[("ids", ids.as_str())]))
                .await
            {
                Ok(list) => products.extend(list.into_iter().map(|p| (p.id, p))),
                Err(e) => {
                    tracing::debug!(error = %e, "GOG product lookup failed");
                    complete = false;
                }
            }
        }

        let mut items = Vec::with_capacity(owned.owned.len());
        for id in owned.owned {
            // Delisted products have no public page: keep the listing incomplete instead of losing them.
            let Some(p) = products.remove(&id) else {
                complete = false;
                continue;
            };
            let Some(title) = p.title.filter(|t| !t.trim().is_empty()) else {
                complete = false;
                continue;
            };
            let kind = match p.game_type.as_deref() {
                Some("dlc") => ProductKind::Dlc,
                Some("pack") => ProductKind::Bundle,
                _ => ProductKind::Game,
            };
            let images = p.images;
            items.push(LibraryItem {
                platform_game_id: id.to_string(),
                title,
                kind,
                parent_platform_game_id: None,
                edition: None,
                acquisition: Acquisition::Purchase,
                playtime_minutes: None,
                last_played_at: None,
                cross_ids: vec![(IdSource::Gog, id.to_string())],
                artwork: Artwork {
                    cover: None,
                    header: images.as_ref().and_then(|i| absolute(i.logo2x.clone())),
                    hero: images.as_ref().and_then(|i| absolute(i.background.clone())),
                    logo: None,
                    icon: images.and_then(|i| absolute(i.icon)),
                },
                raw: serde_json::Value::Null,
            });
        }
        Ok(LibrarySnapshot { complete, items })
    }
}
