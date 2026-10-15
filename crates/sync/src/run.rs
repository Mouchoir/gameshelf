//! One sync of one account: credential refresh, listing, import, run record, health and activity
//! (FR-019 to FR-025, research D6).

use crate::{ImportReport, Importer, SyncError};
use gameshelf_connectors::{AccountRef, Connector, ConnectorError, Context, CredentialPayload, Registry};
use gameshelf_core::{
    AccountId,
    account_status::{AccountEvent, AccountStatus, transition},
};
use gameshelf_store::{
    ActivityKind, CredentialKind, OwnershipSource, RunCapability, RunResult, RunStatus, RunTrigger,
};

/// Outcome of [`Syncer::run_account`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunOutcome {
    Imported(ImportReport),
    /// The account cannot sync now (disabled, needs reconnect, no connector).
    Skipped(&'static str),
    Failed {
        kind: String,
        message: String,
    },
}

/// Runs syncs with the connectors of the instance.
#[derive(Clone, Debug)]
pub struct Syncer {
    importer: Importer,
    registry: Registry,
}

impl Syncer {
    pub fn new(importer: Importer, registry: Registry) -> Self {
        Self { importer, registry }
    }

    pub fn importer(&self) -> &Importer {
        &self.importer
    }

    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    fn context(&self, connector: &dyn Connector) -> Context {
        Context::new(
            gameshelf_connectors::http::client(),
            connector.descriptor().default_rate,
        )
    }

    /// Syncs one account now.
    pub async fn run_account(
        &self,
        account_id: AccountId,
        trigger: RunTrigger,
    ) -> Result<RunOutcome, SyncError> {
        let store = self.importer.store();
        let Some(account) = store.get_account(account_id).await? else {
            return Ok(RunOutcome::Skipped("unknown account"));
        };
        if !account.status.syncs() && account.status != AccountStatus::Pending {
            return Ok(RunOutcome::Skipped("account not active"));
        }
        let Some(connector) = self.registry.get(&account.platform) else {
            return Ok(RunOutcome::Skipped("no connector for this platform"));
        };
        let Some(opened) = store.open_credential(account.id).await? else {
            self.on_auth_failure(&account, None).await?;
            return Ok(RunOutcome::Skipped("no credential"));
        };
        let mut cred = match CredentialPayload::from_json(&opened.json) {
            Ok(c) => c,
            Err(_) => {
                self.on_auth_failure(&account, None).await?;
                return Ok(RunOutcome::Skipped("unreadable credential"));
            }
        };

        let run = store
            .start_run(
                Some(account.id),
                &account.platform,
                RunCapability::Library,
                trigger,
            )
            .await?;
        let cx = self.context(connector.as_ref());

        // Rolling credentials are refreshed and persisted before anything else (contracts/connector.md).
        match connector.refresh(&cx, &cred).await {
            Ok(Some(refreshed)) => {
                let bytes = serde_json::to_vec(&refreshed.payload).unwrap_or_default();
                store
                    .put_credential(account.id, opened.kind, &bytes, refreshed.expires_at)
                    .await?;
                cred = refreshed.payload;
            }
            Ok(None) => {}
            Err(e) => return self.fail(&account, run, e).await,
        }

        let account_ref = AccountRef {
            external_account_id: account.external_account_id.clone(),
            display_name: account.display_name.clone(),
        };
        let snapshot = match connector.library(&cx, &account_ref, &cred).await {
            Ok(s) => s,
            Err(e) => return self.fail(&account, run, e).await,
        };
        let source = if trigger == RunTrigger::Agent {
            OwnershipSource::Agent
        } else {
            OwnershipSource::Connector
        };
        let report = self.importer.import(&account, &snapshot, source).await?;

        // Achievements and enrichment are best effort: failures never fail the library run.
        if connector.descriptor().capabilities.achievements {
            let played: Vec<String> = snapshot
                .items
                .iter()
                .filter(|i| i.playtime_minutes.unwrap_or(0) > 0)
                .map(|i| i.platform_game_id.clone())
                .collect();
            if let Ok(items) = connector.achievements(&cx, &account_ref, &cred, &played).await {
                for a in items {
                    store
                        .set_achievements(
                            account.id,
                            &account.platform,
                            &a.platform_game_id,
                            a.unlocked,
                            a.total,
                        )
                        .await?;
                }
            }
        }
        if connector.descriptor().capabilities.enrichment
            && let Ok(values) = connector.enrich(&cx, &account_ref, &cred).await
        {
            for v in values {
                store
                    .put_enrichment(
                        account.id,
                        &account.platform,
                        v.platform_game_id.as_deref(),
                        connector.descriptor().platform,
                        &v.key,
                        &v.value,
                        v.unit.as_deref(),
                    )
                    .await?;
            }
        }

        let result = RunResult {
            status: Some(if snapshot.complete {
                RunStatus::Succeeded
            } else {
                RunStatus::Partial
            }),
            complete_listing: snapshot.complete,
            added: u32::try_from(report.outcome.added.len() + report.outcome.reappeared.len())
                .unwrap_or(u32::MAX),
            removed: u32::try_from(report.outcome.unavailable.len()).unwrap_or(u32::MAX),
            updated: u32::try_from(report.outcome.updated).unwrap_or(u32::MAX),
            error_kind: None,
            error_message: None,
        };
        store.finish_run(run, &result).await?;
        let next = transition(account.status, AccountEvent::RunSucceeded);
        store.set_account_status(account.id, next, 0).await?;
        store.mark_account_synced(account.id, true).await?;

        for game in &report.added_games {
            store
                .add_activity(
                    account.profile_id,
                    ActivityKind::GameAdded,
                    Some(*game),
                    Some(account.id),
                    &serde_json::json!({}),
                    Some(run),
                )
                .await?;
        }
        for game in &report.unavailable_games {
            store
                .add_activity(
                    account.profile_id,
                    ActivityKind::GameUnavailable,
                    Some(*game),
                    Some(account.id),
                    &serde_json::json!({}),
                    Some(run),
                )
                .await?;
        }

        // Metadata and artwork after the library is safely stored.
        if let Err(e) = self.importer.resolve_metadata(500).await {
            tracing::warn!(error = %e, "metadata resolution failed");
        }
        if let Err(e) = self.importer.fetch_missing_covers(200).await {
            tracing::warn!(error = %e, "cover download failed");
        }
        Ok(RunOutcome::Imported(report))
    }

    async fn fail(
        &self,
        account: &gameshelf_store::Account,
        run: gameshelf_core::RunId,
        err: ConnectorError,
    ) -> Result<RunOutcome, SyncError> {
        let store = self.importer.store();
        let result = RunResult {
            status: Some(RunStatus::Failed),
            error_kind: Some(err.kind().to_string()),
            error_message: Some(err.to_string()),
            ..Default::default()
        };
        store.finish_run(run, &result).await?;
        store.mark_account_synced(account.id, false).await?;
        if err.is_auth() {
            self.on_auth_failure(account, Some(run)).await?;
        } else {
            let failures = account.consecutive_failures + 1;
            let next = transition(
                account.status,
                AccountEvent::RunFailed {
                    consecutive: failures,
                },
            );
            store.set_account_status(account.id, next, failures).await?;
            if next == AccountStatus::Degraded && account.status != AccountStatus::Degraded {
                store
                    .add_activity(
                        account.profile_id,
                        ActivityKind::AccountDegraded,
                        None,
                        Some(account.id),
                        &serde_json::json!({}),
                        Some(run),
                    )
                    .await?;
            }
        }
        Ok(RunOutcome::Failed {
            kind: err.kind().to_string(),
            message: err.to_string(),
        })
    }

    async fn on_auth_failure(
        &self,
        account: &gameshelf_store::Account,
        run: Option<gameshelf_core::RunId>,
    ) -> Result<(), SyncError> {
        let store = self.importer.store();
        let next = transition(account.status, AccountEvent::AuthFailed);
        store
            .set_account_status(account.id, next, account.consecutive_failures)
            .await?;
        if account.status != AccountStatus::NeedsReconnect {
            store
                .add_activity(
                    account.profile_id,
                    ActivityKind::AccountNeedsReconnect,
                    None,
                    Some(account.id),
                    &serde_json::json!({}),
                    run,
                )
                .await?;
        }
        Ok(())
    }

    /// Validates a credential with the platform and connects the account (FR-003). Returns the account id.
    pub async fn connect(
        &self,
        profile: gameshelf_core::ProfileId,
        platform: &str,
        payload: CredentialPayload,
        label: Option<&str>,
    ) -> Result<Result<AccountId, ConnectError>, SyncError> {
        let Some(connector) = self.registry.get(platform) else {
            return Ok(Err(ConnectError::UnknownPlatform));
        };
        let cx = self.context(connector.as_ref());
        let identity = match connector.validate(&cx, &payload).await {
            Ok(id) => id,
            Err(e) => return Ok(Err(ConnectError::Rejected(e))),
        };
        let store = self.importer.store();
        if let Some(existing) = store
            .find_account_by_external(platform, &identity.external_account_id)
            .await?
        {
            if existing.profile_id != profile {
                let holder = store
                    .get_profile(existing.profile_id)
                    .await?
                    .map(|p| p.display_name)
                    .unwrap_or_default();
                return Ok(Err(ConnectError::Taken(holder)));
            }
            // Reconnecting an existing account: replace its credential and reactivate it.
            let bytes = serde_json::to_vec(&payload).unwrap_or_default();
            store
                .put_credential(existing.id, CredentialKind::ApiKey, &bytes, None)
                .await?;
            store
                .set_account_status(
                    existing.id,
                    transition(existing.status, AccountEvent::CredentialValidated),
                    0,
                )
                .await?;
            return Ok(Ok(existing.id));
        }
        let account = match store
            .create_account(gameshelf_store::NewAccount {
                profile_id: profile,
                platform,
                external_account_id: &identity.external_account_id,
                display_name: identity.display_name.as_deref(),
                label,
                status: AccountStatus::Active,
            })
            .await
        {
            Ok(a) => a,
            Err(gameshelf_store::StoreError::AccountTaken { holder }) => {
                return Ok(Err(ConnectError::Taken(holder)));
            }
            Err(e) => return Err(e.into()),
        };
        let bytes = serde_json::to_vec(&payload).unwrap_or_default();
        store
            .put_credential(account.id, CredentialKind::ApiKey, &bytes, None)
            .await?;
        Ok(Ok(account.id))
    }
}

/// Why a connection attempt failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConnectError {
    UnknownPlatform,
    Rejected(ConnectorError),
    /// Already connected by another profile (its display name).
    Taken(String),
}
