//! Applies agent reports (contracts/agent-api.md, FR-026 to FR-030).
//!
//! Each reported account is matched to the player's accounts by its platform id; games a launcher keeps for
//! the machine go to the player's only account on that platform, or to an account named after the agent.
//! Products the report leaves untitled are named from the platform's public data. Reports never remove games.

use crate::{ImportReport, Importer, RunOutcome, SyncError};
use gameshelf_connectors::{Artwork, Context, LibraryItem, LibrarySnapshot, Registry, descriptor::RateLimit};
use gameshelf_core::{
    Acquisition,
    account_status::AccountStatus,
    agent::{Report, ReportItem, SOURCES, source_platform},
    platform,
};
use gameshelf_store::{
    Account, ActivityKind, Agent, NewAccount, NewInstall, OwnershipSource, RunCapability, RunResult,
    RunStatus, RunTrigger, StoreError,
};
use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
    time::Duration,
};

/// Public lookups are slow on purpose: store APIs throttle anonymous callers.
const LOOKUP_RATE: RateLimit = RateLimit {
    requests: 1,
    per: Duration::from_millis(1500),
};
/// Items accepted in one report.
pub const MAX_REPORT_ITEMS: usize = 50_000;

/// Why a report is refused.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ReportRejection {
    #[error("unknown source or platform")]
    Unknown,
    #[error("this source is turned off for this agent")]
    SourceDisabled,
    #[error("the report is too large")]
    TooLarge,
}

/// Whether a report needs applying.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReportCheck {
    Unchanged,
    Accepted,
}

/// What one report changed.
#[derive(Clone, Debug, Default)]
pub struct AppliedReport {
    /// One outcome per account that received games, for notifications.
    pub runs: Vec<(gameshelf_core::AccountId, RunOutcome)>,
    pub items: usize,
    /// Items that could not be named and were left out.
    pub unresolved: usize,
}

/// Applies agent reports, one at a time.
#[derive(Clone, Debug)]
pub struct AgentImporter {
    importer: Importer,
    registry: Registry,
    serial: Arc<tokio::sync::Mutex<()>>,
    /// Products the platform said it does not know, not asked again until restart.
    unknown: Arc<Mutex<HashSet<(String, String)>>>,
    /// Shared so every lookup goes through the same rate limiter.
    lookup_cx: Arc<Context>,
}

impl AgentImporter {
    pub fn new(importer: Importer, registry: Registry) -> Self {
        Self {
            importer,
            registry,
            serial: Arc::default(),
            unknown: Arc::default(),
            lookup_cx: Arc::new(Context::new(gameshelf_connectors::http::client(), LOOKUP_RATE)),
        }
    }

    /// Validates a report and tells whether it differs from the last one of its source.
    pub async fn check(
        &self,
        agent: &Agent,
        report: &Report,
    ) -> Result<Result<ReportCheck, ReportRejection>, SyncError> {
        let known_source = SOURCES.iter().any(|(s, _)| *s == report.source);
        let platform_ok = match source_platform(&report.source) {
            Some(p) => p == report.platform,
            None => platform::platform(&report.platform).is_some_and(|p| !p.console && p.key != "manual"),
        };
        if !known_source || !platform_ok {
            return Ok(Err(ReportRejection::Unknown));
        }
        if !agent.source_enabled(&report.source) {
            return Ok(Err(ReportRejection::SourceDisabled));
        }
        let count = report.machine_items.len() + report.accounts.iter().map(|a| a.items.len()).sum::<usize>();
        if count > MAX_REPORT_ITEMS {
            return Ok(Err(ReportRejection::TooLarge));
        }
        let last = self
            .importer
            .store()
            .last_report_hash(agent.id, &report.source, &report.platform)
            .await?;
        Ok(Ok(if last.as_deref() == Some(report.content_hash.as_str()) {
            ReportCheck::Unchanged
        } else {
            ReportCheck::Accepted
        }))
    }

    /// Applies a checked report.
    pub async fn apply(&self, agent: &Agent, report: &Report) -> Result<AppliedReport, SyncError> {
        let _guard = self.serial.lock().await;
        let store = self.importer.store();
        let platform = report.platform.as_str();
        let mut out = AppliedReport::default();
        let mut lookup_failed = false;
        let mut installs = Vec::new();

        let mut groups: Vec<(Option<Account>, Option<String>, &[ReportItem])> = Vec::new();
        for acc in &report.accounts {
            let target = self
                .reported_account(
                    agent,
                    platform,
                    &acc.external_account_id,
                    acc.display_name.as_deref(),
                )
                .await?;
            groups.push((target, Some(acc.external_account_id.clone()), &acc.items));
        }
        if !report.machine_items.is_empty() {
            let target = self.machine_account(agent, platform).await?;
            groups.push((Some(target), None, &report.machine_items));
        }

        for (target, external, items) in groups {
            for item in items.iter().filter(|i| i.installed) {
                installs.push(NewInstall {
                    platform_game_id: item.platform_game_id.clone(),
                    account_external_id: external.clone(),
                    install_path: item.install_path.clone().filter(|_| agent.send_install_paths),
                    size_bytes: item.size_bytes,
                });
            }
            let Some(account) = target else { continue };
            let mut snapshot = LibrarySnapshot {
                complete: false,
                items: Vec::with_capacity(items.len()),
            };
            for item in items {
                match self.library_item(platform, item).await {
                    Ok(Some(li)) => snapshot.items.push(li),
                    Ok(None) => out.unresolved += 1,
                    Err(()) => {
                        out.unresolved += 1;
                        lookup_failed = true;
                    }
                }
            }
            out.items += snapshot.items.len();
            if snapshot.items.is_empty() {
                continue;
            }
            let run = store
                .start_run(
                    Some(account.id),
                    platform,
                    RunCapability::Library,
                    RunTrigger::Agent,
                )
                .await?;
            let report_out: ImportReport = self
                .importer
                .import(&account, &snapshot, OwnershipSource::Agent)
                .await?;
            store
                .finish_run(
                    run,
                    &RunResult {
                        status: Some(RunStatus::Partial),
                        complete_listing: false,
                        added: u32::try_from(report_out.outcome.added.len()).unwrap_or(u32::MAX),
                        removed: 0,
                        updated: u32::try_from(report_out.outcome.updated).unwrap_or(u32::MAX),
                        error_kind: None,
                        error_message: None,
                    },
                )
                .await?;
            if account.agent_id.is_some() {
                store.mark_account_synced(account.id, true).await?;
            }
            for game in &report_out.added_games {
                store
                    .add_activity(
                        account.profile_id,
                        ActivityKind::GameAdded,
                        Some(*game),
                        Some(account.id),
                        &serde_json::json!({ "agent": agent.name }),
                        Some(run),
                    )
                    .await?;
            }
            out.runs.push((
                account.id,
                RunOutcome::Imported {
                    report: report_out,
                    run,
                },
            ));
        }

        store.replace_installs(agent.id, platform, &installs).await?;
        // A report with failed lookups is applied again next time instead of being skipped as unchanged.
        let hash = if lookup_failed {
            format!("retry:{}", report.content_hash)
        } else {
            report.content_hash.clone()
        };
        store
            .record_report(
                agent.id,
                &report.source,
                platform,
                &hash,
                &serde_json::json!({
                    "accounts": report.accounts.len(),
                    "items": out.items,
                    "unresolved": out.unresolved,
                    "installed": installs.len(),
                }),
            )
            .await?;
        drop(_guard);

        if let Err(e) = self.importer.resolve_metadata(500).await {
            tracing::warn!(error = %e, "metadata resolution failed");
        }
        if let Err(e) = self.importer.fetch_missing_covers(200).await {
            tracing::warn!(error = %e, "cover download failed");
        }
        Ok(out)
    }

    /// The account a reported account id belongs to, created for the agent's owner when unknown. `None` when
    /// another profile holds it: its games are not imported for this profile.
    async fn reported_account(
        &self,
        agent: &Agent,
        platform: &str,
        external: &str,
        display_name: Option<&str>,
    ) -> Result<Option<Account>, SyncError> {
        let store = self.importer.store();
        let external = external.trim();
        if external.is_empty() || external.starts_with("agent:") {
            return Ok(Some(self.machine_account(agent, platform).await?));
        }
        if let Some(a) = store.find_account_by_external(platform, external).await? {
            return Ok((a.profile_id == agent.profile_id).then_some(a));
        }
        let created = store
            .create_agent_account(
                NewAccount {
                    profile_id: agent.profile_id,
                    platform,
                    external_account_id: external,
                    display_name,
                    label: display_name.or(Some(external)),
                    status: AccountStatus::Active,
                },
                agent.id,
            )
            .await;
        match created {
            Ok(a) => Ok(Some(a)),
            Err(StoreError::AccountTaken { .. }) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Where games a launcher keeps for the machine go: the player's only account on that platform when there
    /// is exactly one, otherwise an account named after the agent.
    async fn machine_account(&self, agent: &Agent, platform: &str) -> Result<Account, SyncError> {
        let store = self.importer.store();
        let own = |accounts: &[Account]| -> Vec<Account> {
            accounts
                .iter()
                .filter(|a| a.platform == platform && !a.external_account_id.starts_with("agent:"))
                .cloned()
                .collect()
        };
        let candidates = own(&store.list_accounts(agent.profile_id).await?);
        if let [only] = candidates.as_slice() {
            return Ok(only.clone());
        }
        let external = format!("agent:{}", agent.id);
        if let Some(a) = store.find_account_by_external(platform, &external).await? {
            return Ok(a);
        }
        Ok(store
            .create_agent_account(
                NewAccount {
                    profile_id: agent.profile_id,
                    platform,
                    external_account_id: &external,
                    display_name: None,
                    label: Some(&agent.name),
                    status: AccountStatus::Active,
                },
                agent.id,
            )
            .await?)
    }

    /// Turns a reported item into a listing item: known products keep their catalog title and kind, unknown
    /// ones take the reported title or are looked up. `Err` when a lookup failed and should be retried.
    async fn library_item(&self, platform: &str, item: &ReportItem) -> Result<Option<LibraryItem>, ()> {
        let store = self.importer.store();
        let pgid = item.platform_game_id.trim();
        if pgid.is_empty() || pgid.len() > 200 {
            return Ok(None);
        }
        let base = |title: String, kind, parent: Option<String>| LibraryItem {
            platform_game_id: pgid.to_string(),
            title,
            kind,
            parent_platform_game_id: parent,
            edition: None,
            acquisition: Acquisition::Purchase,
            playtime_minutes: item.playtime_minutes,
            last_played_at: item.last_played_at,
            cross_ids: Vec::new(),
            artwork: Artwork::default(),
            raw: serde_json::Value::Null,
        };
        if let Ok(Some(p)) = store.find_product(platform, pgid).await {
            return Ok(Some(base(p.title, p.kind, p.parent_platform_game_id)));
        }
        if let Some(title) = item.title.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
            let title: String = title.chars().take(300).collect();
            return Ok(Some(base(title, item.kind, None)));
        }
        let key = (platform.to_string(), pgid.to_string());
        if self.unknown.lock().map(|u| u.contains(&key)).unwrap_or(false) {
            return Ok(None);
        }
        let Some(connector) = self.registry.get(platform) else {
            return Ok(None);
        };
        match connector.lookup(&self.lookup_cx, pgid).await {
            Ok(Some(mut found)) => {
                found.playtime_minutes = item.playtime_minutes;
                found.last_played_at = item.last_played_at;
                Ok(Some(found))
            }
            Ok(None) => {
                if let Ok(mut u) = self.unknown.lock() {
                    u.insert(key);
                }
                Ok(None)
            }
            Err(e) => {
                tracing::debug!(platform, error = %e, "product lookup failed");
                Err(())
            }
        }
    }
}
