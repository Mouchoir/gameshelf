//! Epic Games Store connector (unofficial API used by the launcher, research section 1).
//!
//! Sign-in uses the launcher's public OAuth client, as open-source launchers do. The player signs in on
//! epicgames.com; the final page prints JSON with an `authorizationCode`, exchanged here for rolling tokens.
//! Credential payload: `{"access_token", "refresh_token", "refresh_expires_at", "account_id", "display_name"}`.

use crate::{
    AccountIdentity, AccountRef, Artwork, AuthMethod, CodeCapture, Connector, ConnectorDescriptor,
    ConnectorError, Context, CredentialPayload, IdSource, LibraryItem, LibrarySnapshot, Refreshed,
    descriptor::RateLimit,
};
use async_trait::async_trait;
use gameshelf_core::{Acquisition, ProductKind, platform};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, HashMap},
    time::Duration,
};
use time::OffsetDateTime;

/// Public client of the Epic Games launcher (also used by open-source launchers).
const CLIENT_ID: &str = "34a02cf8f4414e29b15921876da36f9a";
const CLIENT_SECRET: &str = "daafbccc737745039dffe53d94fc76cf";
const ACCOUNT_BASE: &str = "https://account-public-service-prod03.ol.epicgames.com";
const LIBRARY_BASE: &str = "https://library-service.live.use1a.on.epicgames.com";
const CATALOG_BASE: &str = "https://catalog-public-service-prod06.ol.epicgames.com";
/// Namespaces that hold engine tools and marketplace assets rather than games.
const SKIPPED_NAMESPACES: &[&str] = &["ue"];
/// Catalog categories that are not games.
const SKIPPED_CATEGORIES: &[&str] = &["engines", "plugins", "assets", "digitalextras"];
/// Catalog ids requested per bulk call.
const CATALOG_BATCH: usize = 50;

static AUTH: [AuthMethod; 1] = [AuthMethod::BrowserLogin {
    login_url: "https://www.epicgames.com/id/login?redirectUrl=https%3A%2F%2Fwww.epicgames.com%2Fid%2Fapi%2Fredirect%3FclientId%3D34a02cf8f4414e29b15921876da36f9a%26responseType%3Dcode",
    capture: CodeCapture::JsonPage {
        field: "authorizationCode",
    },
}];

static DESCRIPTOR: std::sync::LazyLock<ConnectorDescriptor> = std::sync::LazyLock::new(|| {
    let info = platform::platform("epic").copied();
    ConnectorDescriptor {
        platform: "epic",
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
pub struct EpicConnector {
    account_base: String,
    library_base: String,
    catalog_base: String,
}

impl Default for EpicConnector {
    fn default() -> Self {
        Self {
            account_base: ACCOUNT_BASE.into(),
            library_base: LIBRARY_BASE.into(),
            catalog_base: CATALOG_BASE.into(),
        }
    }
}

impl EpicConnector {
    /// Uses one base URL for every Epic service, for tests.
    pub fn with_base(base: impl Into<String>) -> Self {
        let base = base.into();
        Self {
            account_base: base.clone(),
            library_base: base.clone(),
            catalog_base: base,
        }
    }

    fn token<'a>(&self, cred: &'a CredentialPayload) -> Result<&'a str, ConnectorError> {
        cred.str("access_token").ok_or(ConnectorError::AuthExpired)
    }

    async fn token_request(&self, cx: &Context, form: &[(&str, &str)]) -> Result<Refreshed, ConnectorError> {
        let url = format!("{}/account/api/oauth/token", self.account_base);
        let resp: TokenResponse = cx
            .json(|| {
                cx.http
                    .post(&url)
                    .basic_auth(CLIENT_ID, Some(CLIENT_SECRET))
                    .form(form)
            })
            .await
            .map_err(|e| match e {
                ConnectorError::Permanent(_) => ConnectorError::AuthExpired,
                other => other,
            })?;
        let refresh_expires_at = resp
            .refresh_expires_at
            .as_deref()
            .and_then(|s| OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339).ok());
        let payload = serde_json::json!({
            "access_token": resp.access_token,
            "refresh_token": resp.refresh_token,
            "refresh_expires_at": resp.refresh_expires_at,
            "account_id": resp.account_id,
            "display_name": resp.display_name,
        });
        Ok(Refreshed {
            payload: CredentialPayload(payload),
            expires_at: refresh_expires_at,
        })
    }

    async fn catalog(
        &self,
        cx: &Context,
        token: &str,
        namespace: &str,
        ids: &[String],
    ) -> Result<HashMap<String, CatalogItem>, ConnectorError> {
        let url = format!(
            "{}/catalog/api/shared/namespace/{namespace}/bulk/items",
            self.catalog_base
        );
        let mut out = HashMap::new();
        for chunk in ids.chunks(CATALOG_BATCH) {
            let mut query: Vec<(&str, &str)> = chunk.iter().map(|id| ("id", id.as_str())).collect();
            query.extend([
                ("includeDLCDetails", "true"),
                ("includeMainGameDetails", "true"),
                ("country", "US"),
                ("locale", "en"),
            ]);
            let items: HashMap<String, CatalogItem> = cx
                .json(|| cx.http.get(&url).bearer_auth(token).query(&query))
                .await?;
            out.extend(items);
        }
        Ok(out)
    }
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    refresh_expires_at: Option<String>,
    account_id: Option<String>,
    #[serde(rename = "displayName")]
    display_name: Option<String>,
}

#[derive(Deserialize)]
struct VerifyResponse {
    account_id: String,
    #[serde(rename = "displayName", alias = "display_name")]
    display_name: Option<String>,
}

#[derive(Deserialize)]
struct LibraryPage {
    #[serde(default)]
    records: Vec<LibraryRecord>,
    #[serde(rename = "responseMetadata")]
    response_metadata: Option<ResponseMetadata>,
}

#[derive(Deserialize)]
struct ResponseMetadata {
    #[serde(rename = "nextCursor")]
    next_cursor: Option<String>,
}

#[derive(Deserialize)]
struct LibraryRecord {
    namespace: String,
    #[serde(rename = "catalogItemId")]
    catalog_item_id: String,
    #[serde(rename = "appName")]
    app_name: Option<String>,
}

#[derive(Clone, Deserialize)]
struct CatalogItem {
    title: Option<String>,
    #[serde(rename = "keyImages", default)]
    key_images: Vec<KeyImage>,
    #[serde(default)]
    categories: Vec<Category>,
    #[serde(rename = "mainGameItem")]
    main_game_item: Option<MainGameItem>,
}

#[derive(Clone, Deserialize)]
struct KeyImage {
    #[serde(rename = "type")]
    kind: String,
    url: String,
}

#[derive(Clone, Deserialize)]
struct Category {
    path: String,
}

#[derive(Clone, Deserialize)]
struct MainGameItem {
    id: String,
}

#[derive(Deserialize)]
struct Playtime {
    #[serde(rename = "artifactId")]
    artifact_id: String,
    #[serde(rename = "totalTime")]
    total_time: u64,
}

fn image(item: &CatalogItem, kinds: &[&str]) -> Option<String> {
    kinds.iter().find_map(|k| {
        item.key_images
            .iter()
            .find(|i| i.kind == *k)
            .map(|i| i.url.clone())
    })
}

#[async_trait]
impl Connector for EpicConnector {
    fn descriptor(&self) -> &'static ConnectorDescriptor {
        &DESCRIPTOR
    }

    async fn exchange_code(&self, cx: &Context, code: &str) -> Result<Refreshed, ConnectorError> {
        self.token_request(
            cx,
            &[
                ("grant_type", "authorization_code"),
                ("code", code),
                ("token_type", "eg1"),
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
        self.token_request(
            cx,
            &[
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh),
                ("token_type", "eg1"),
            ],
        )
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
        let url = format!("{}/account/api/oauth/verify", self.account_base);
        let v: VerifyResponse =
            cx.json(|| cx.http.get(&url).bearer_auth(token))
                .await
                .map_err(|e| match e {
                    ConnectorError::AuthExpired => {
                        ConnectorError::AuthInvalid("Epic refused the sign-in".into())
                    }
                    other => other,
                })?;
        Ok(AccountIdentity {
            external_account_id: v.account_id,
            display_name: v
                .display_name
                .or_else(|| cred.str("display_name").map(str::to_string)),
        })
    }

    async fn library(
        &self,
        cx: &Context,
        account: &AccountRef,
        cred: &CredentialPayload,
    ) -> Result<LibrarySnapshot, ConnectorError> {
        let token = self.token(cred)?;
        let url = format!("{}/library/api/public/items", self.library_base);
        let mut records = Vec::new();
        let mut cursor: Option<String> = None;
        for _page in 0..200 {
            let mut query = vec![
                ("includeMetadata", "true".to_string()),
                ("platform", "Windows".to_string()),
            ];
            if let Some(c) = &cursor {
                query.push(("cursor", c.clone()));
            }
            let page: LibraryPage = cx
                .json(|| cx.http.get(&url).bearer_auth(token).query(&query))
                .await?;
            let empty = page.records.is_empty();
            records.extend(page.records);
            cursor = page
                .response_metadata
                .and_then(|m| m.next_cursor)
                .filter(|c| !c.is_empty());
            if cursor.is_none() || empty {
                break;
            }
        }
        let complete = cursor.is_none();

        let mut by_namespace: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for r in &records {
            if !SKIPPED_NAMESPACES.contains(&r.namespace.as_str()) {
                by_namespace
                    .entry(r.namespace.clone())
                    .or_default()
                    .push(r.catalog_item_id.clone());
            }
        }
        let mut catalog: HashMap<(String, String), CatalogItem> = HashMap::new();
        let mut catalog_complete = true;
        for (ns, ids) in &by_namespace {
            match self.catalog(cx, token, ns, ids).await {
                Ok(items) => catalog.extend(items.into_iter().map(|(id, item)| ((ns.clone(), id), item))),
                Err(e) if e.is_auth() => return Err(e),
                Err(e) => {
                    tracing::debug!(error = %e, "Epic catalog lookup failed for a namespace");
                    catalog_complete = false;
                }
            }
        }

        let playtime_url = format!(
            "{}/library/api/public/playtime/account/{}/all",
            self.library_base, account.external_account_id
        );
        let playtimes: HashMap<String, u32> = match cx
            .json::<Vec<Playtime>, _>(|| cx.http.get(&playtime_url).bearer_auth(token))
            .await
        {
            Ok(list) => list
                .into_iter()
                .map(|p| {
                    (
                        p.artifact_id,
                        u32::try_from(p.total_time / 60).unwrap_or(u32::MAX),
                    )
                })
                .collect(),
            Err(e) if e.is_auth() => return Err(e),
            Err(_) => HashMap::new(),
        };

        let mut items = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for r in records {
            if SKIPPED_NAMESPACES.contains(&r.namespace.as_str()) {
                continue;
            }
            let pgid = format!("{}:{}", r.namespace, r.catalog_item_id);
            if !seen.insert(pgid.clone()) {
                continue;
            }
            let Some(entry) = catalog.get(&(r.namespace.clone(), r.catalog_item_id.clone())) else {
                catalog_complete = false;
                continue;
            };
            if entry
                .categories
                .iter()
                .any(|c| SKIPPED_CATEGORIES.contains(&c.path.as_str()))
            {
                continue;
            }
            let title = entry
                .title
                .clone()
                .or_else(|| r.app_name.clone())
                .unwrap_or_default();
            if title.trim().is_empty() {
                catalog_complete = false;
                continue;
            }
            let is_dlc =
                entry.main_game_item.is_some() || entry.categories.iter().any(|c| c.path == "addons");
            let parent = entry
                .main_game_item
                .as_ref()
                .map(|m| format!("{}:{}", r.namespace, m.id));
            let playtime = r.app_name.as_ref().and_then(|a| playtimes.get(a).copied());
            items.push(LibraryItem {
                platform_game_id: pgid.clone(),
                title,
                kind: if is_dlc {
                    ProductKind::Dlc
                } else {
                    ProductKind::Game
                },
                parent_platform_game_id: if is_dlc { parent } else { None },
                edition: None,
                acquisition: Acquisition::Purchase,
                playtime_minutes: playtime,
                last_played_at: None,
                cross_ids: vec![(IdSource::Epic, pgid)],
                artwork: Artwork {
                    cover: image(entry, &["DieselGameBoxTall", "OfferImageTall"]),
                    header: image(entry, &["DieselGameBox", "OfferImageWide", "Thumbnail"]),
                    hero: None,
                    logo: image(entry, &["DieselGameBoxLogo"]),
                    icon: None,
                },
                raw: serde_json::json!({ "app_name": r.app_name }),
            });
        }
        Ok(LibrarySnapshot {
            complete: complete && catalog_complete,
            items,
        })
    }
}
