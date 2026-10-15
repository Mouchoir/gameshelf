//! A connector serving synthetic libraries, for tests and development (`GAMESHELF_DEV_FAKE_CONNECTOR`).
//!
//! It never makes network requests. Titles are invented so that no real library leaks into fixtures.

use crate::{
    AccountIdentity, AccountRef, Artwork, AuthMethod, Connector, ConnectorDescriptor, ConnectorError,
    Context, CredentialPayload, IdSource, KeyField, LibraryItem, LibrarySnapshot, descriptor::RateLimit,
};
use async_trait::async_trait;
use gameshelf_core::{
    Acquisition, ProductKind,
    platform::{Capabilities, Tier},
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

/// Invented titles used for generated libraries.
pub const SAMPLE_TITLES: &[&str] = &[
    "Starlight Harbor",
    "The Quiet Foundry",
    "Moss & Mortar",
    "Ninefold Depths",
    "Lantern Run",
    "Cobalt Orchard",
    "A Tale of Two Gears",
    "Paper Comets",
    "Glacier Postal Service",
    "Hollow Crown Tactics",
    "Driftwood Kart",
    "Ember Atlas",
    "Velvet Circuit",
    "Brass Lighthouse",
    "Tidewater Tales",
    "Signal Lost",
    "Orbit Bakery",
    "Thornfield Rally",
    "Clockwork Meadow",
    "Night Market Detective",
];

static FIELDS: [KeyField; 1] = [KeyField {
    name: "account",
    label_key: "connect.fake.account",
    secret: false,
}];
static AUTH: [AuthMethod; 1] = [AuthMethod::ApiKey {
    fields: &FIELDS,
    help_url: "https://example.com",
}];

/// What the fake connector returns for an account.
#[derive(Clone, Debug)]
pub enum Script {
    Library(LibrarySnapshot),
    Fail(ConnectorError),
}

#[derive(Clone)]
pub struct FakeConnector {
    descriptor: &'static ConnectorDescriptor,
    scripts: Arc<Mutex<HashMap<String, Script>>>,
}

impl std::fmt::Debug for FakeConnector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FakeConnector")
            .field("platform", &self.descriptor.platform)
            .finish_non_exhaustive()
    }
}

impl FakeConnector {
    /// A fake connector impersonating `platform` (which must be a registry key).
    pub fn new(platform: &'static str) -> Self {
        let descriptor = Box::leak(Box::new(ConnectorDescriptor {
            platform,
            tier: Tier::Official,
            capabilities: Capabilities {
                library: true,
                playtime: true,
                last_played: true,
                ..Default::default()
            },
            auth: &AUTH,
            default_rate: RateLimit {
                requests: 1000,
                per: Duration::from_secs(1),
            },
            multi_account: true,
        }));
        Self {
            descriptor,
            scripts: Arc::default(),
        }
    }

    /// Sets what an account returns on the next calls.
    pub fn script(&self, account: &str, script: Script) {
        if let Ok(mut s) = self.scripts.lock() {
            s.insert(account.to_string(), script);
        }
    }

    /// A generated library: a deterministic subset of [`SAMPLE_TITLES`] depending on the account id.
    pub fn generated(account: &str, platform: &str) -> LibrarySnapshot {
        let seed: u32 = account.bytes().map(u32::from).sum();
        let items = SAMPLE_TITLES
            .iter()
            .enumerate()
            .filter(|(i, _)| (u32::try_from(*i).unwrap_or(0) + seed) % 3 != 0)
            .map(|(i, title)| {
                item(
                    platform,
                    &format!("{}", 1000 + i),
                    title,
                    Some(((seed as usize + i) % 50) as u32 * 37),
                )
            })
            .collect();
        LibrarySnapshot {
            complete: true,
            items,
        }
    }
}

/// Builds a library item.
pub fn item(platform: &str, id: &str, title: &str, playtime: Option<u32>) -> LibraryItem {
    let cross = match platform {
        "steam" => vec![(IdSource::Steam, id.to_string())],
        "gog" => vec![(IdSource::Gog, id.to_string())],
        "epic" => vec![(IdSource::Epic, id.to_string())],
        _ => Vec::new(),
    };
    LibraryItem {
        platform_game_id: id.to_string(),
        title: title.to_string(),
        kind: ProductKind::Game,
        parent_platform_game_id: None,
        edition: None,
        acquisition: Acquisition::Purchase,
        playtime_minutes: playtime,
        last_played_at: None,
        cross_ids: cross,
        artwork: Artwork::default(),
        raw: serde_json::Value::Null,
    }
}

#[async_trait]
impl Connector for FakeConnector {
    fn descriptor(&self) -> &'static ConnectorDescriptor {
        self.descriptor
    }

    async fn validate(
        &self,
        _cx: &Context,
        cred: &CredentialPayload,
    ) -> Result<AccountIdentity, ConnectorError> {
        let account = cred
            .str("account")
            .ok_or_else(|| ConnectorError::AuthInvalid("missing account".into()))?;
        Ok(AccountIdentity {
            external_account_id: account.to_string(),
            display_name: Some(format!("Fake {account}")),
        })
    }

    async fn library(
        &self,
        _cx: &Context,
        account: &AccountRef,
        _cred: &CredentialPayload,
    ) -> Result<LibrarySnapshot, ConnectorError> {
        let script = self
            .scripts
            .lock()
            .ok()
            .and_then(|s| s.get(&account.external_account_id).cloned());
        match script {
            Some(Script::Library(lib)) => Ok(lib),
            Some(Script::Fail(e)) => Err(e),
            None => Ok(Self::generated(
                &account.external_account_id,
                self.descriptor.platform,
            )),
        }
    }
}
