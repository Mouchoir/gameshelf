//! Platform connectors (contracts/connector.md).
//!
//! A connector reads one platform for one account. Connectors only read: no request may change platform state
//! (Constitution, Principle IV).

pub mod descriptor;
pub mod epic;
pub mod error;
pub mod fake;
pub mod gog;
pub mod http;
pub mod model;
pub mod steam;

use async_trait::async_trait;
pub use descriptor::{AuthMethod, CodeCapture, ConnectorDescriptor, KeyField};
pub use error::ConnectorError;
pub use http::Context;
pub use model::*;
use std::{collections::HashMap, sync::Arc};

/// One platform connector.
#[async_trait]
pub trait Connector: Send + Sync {
    fn descriptor(&self) -> &'static ConnectorDescriptor;

    /// Checks a credential and returns the account it belongs to. Must not write anything anywhere.
    async fn validate(
        &self,
        cx: &Context,
        cred: &CredentialPayload,
    ) -> Result<AccountIdentity, ConnectorError>;

    /// Exchanges the authorization code of a browser login for a credential (contracts/connector.md section 3).
    async fn exchange_code(&self, _cx: &Context, _code: &str) -> Result<Refreshed, ConnectorError> {
        Err(ConnectorError::Unsupported)
    }

    /// Refreshes a rolling credential. `Ok(None)` means the credential does not need refreshing.
    async fn refresh(
        &self,
        _cx: &Context,
        _cred: &CredentialPayload,
    ) -> Result<Option<Refreshed>, ConnectorError> {
        Ok(None)
    }

    /// Lists the account's library; the snapshot says whether the listing is complete.
    async fn library(
        &self,
        cx: &Context,
        account: &AccountRef,
        cred: &CredentialPayload,
    ) -> Result<LibrarySnapshot, ConnectorError>;

    /// Achievements progress for the given platform game ids.
    async fn achievements(
        &self,
        _cx: &Context,
        _account: &AccountRef,
        _cred: &CredentialPayload,
        _ids: &[String],
    ) -> Result<Vec<AchievementItem>, ConnectorError> {
        Err(ConnectorError::Unsupported)
    }

    /// Per-account enrichment values (platform level, ranks...).
    async fn enrich(
        &self,
        _cx: &Context,
        _account: &AccountRef,
        _cred: &CredentialPayload,
    ) -> Result<Vec<EnrichmentItem>, ConnectorError> {
        Err(ConnectorError::Unsupported)
    }
}

/// The connectors available on this instance, by platform key.
#[derive(Clone, Default)]
pub struct Registry {
    connectors: HashMap<&'static str, Arc<dyn Connector>>,
}

impl std::fmt::Debug for Registry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Registry")
            .field("platforms", &self.connectors.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl Registry {
    /// Builds the registry with every built-in remote connector.
    pub fn builtin() -> Self {
        let mut r = Self::default();
        r.register(Arc::new(steam::SteamConnector::default()));
        r.register(Arc::new(epic::EpicConnector::default()));
        r.register(Arc::new(gog::GogConnector::default()));
        r
    }

    pub fn register(&mut self, connector: Arc<dyn Connector>) {
        self.connectors.insert(connector.descriptor().platform, connector);
    }

    pub fn get(&self, platform: &str) -> Option<Arc<dyn Connector>> {
        self.connectors.get(platform).cloned()
    }

    pub fn platforms(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.connectors.keys().copied()
    }
}
