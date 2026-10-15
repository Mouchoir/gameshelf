//! State shared by every request: store, sync engine, search index and sign-in limiter.

use axum::extract::FromRef;
use gameshelf_core::{AccountId, search::SearchIndex};
use gameshelf_store::{RunTrigger, Store};
use gameshelf_sync::{AgentImporter, Syncer};
use leptos::prelude::LeptosOptions;
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{Arc, Mutex, RwLock},
    time::{Duration, Instant},
};

/// Sign-in attempts allowed per window, per address and per username (contracts/http-api.md section 3).
pub const LOGIN_ATTEMPTS: usize = 10;
pub const LOGIN_WINDOW: Duration = Duration::from_secs(15 * 60);
/// Pairing attempts allowed per window and per address (contracts/agent-api.md).
pub const PAIR_ATTEMPTS: usize = 5;
pub const PAIR_WINDOW: Duration = Duration::from_secs(10 * 60);
/// Accounts synced at the same time (research D6).
pub const MAX_PARALLEL_SYNCS: usize = 3;

#[derive(Clone)]
pub struct AppState {
    pub leptos: LeptosOptions,
    pub store: Store,
    pub syncer: Syncer,
    /// Applies agent reports.
    pub agents: AgentImporter,
    pub search: Arc<RwLock<SearchIndex>>,
    pub assets_dir: PathBuf,
    pub https: bool,
    /// Public URL used in notification links.
    pub base_url: Option<String>,
    pub http: reqwest::Client,
    login_attempts: Arc<Mutex<HashMap<String, Vec<Instant>>>>,
    syncing: Arc<Mutex<HashSet<AccountId>>>,
    sync_permits: Arc<tokio::sync::Semaphore>,
    next_scheduled: Arc<RwLock<Option<time::OffsetDateTime>>>,
}

impl std::fmt::Debug for AppState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppState").finish_non_exhaustive()
    }
}

impl FromRef<AppState> for LeptosOptions {
    fn from_ref(state: &AppState) -> Self {
        state.leptos.clone()
    }
}

impl AppState {
    pub fn new(
        leptos: LeptosOptions,
        store: Store,
        syncer: Syncer,
        assets_dir: PathBuf,
        https: bool,
    ) -> Self {
        Self {
            leptos,
            store,
            agents: AgentImporter::new(syncer.importer().clone(), syncer.registry().clone()),
            syncer,
            search: Arc::default(),
            assets_dir,
            https,
            base_url: None,
            http: reqwest::Client::new(),
            login_attempts: Arc::default(),
            syncing: Arc::default(),
            sync_permits: Arc::new(tokio::sync::Semaphore::new(MAX_PARALLEL_SYNCS)),
            next_scheduled: Arc::default(),
        }
    }

    /// Rebuilds the search index from every game of the instance.
    pub async fn rebuild_search(&self) {
        match self.store.all_game_keys().await {
            Ok(keys) => {
                let index = SearchIndex::build(keys);
                if let Ok(mut guard) = self.search.write() {
                    *guard = index;
                }
            }
            Err(e) => tracing::warn!(error = %e, "cannot rebuild the search index"),
        }
    }

    /// Records a sign-in attempt for each key and returns false when any key is over the limit.
    pub fn allow_login_attempt(&self, keys: &[String]) -> bool {
        self.allow_attempts(keys, LOGIN_ATTEMPTS, LOGIN_WINDOW)
    }

    /// Records an agent pairing attempt from `client` and returns false when it is over the limit.
    pub fn allow_pair_attempt(&self, client: &str) -> bool {
        self.allow_attempts(&[format!("pair:{client}")], PAIR_ATTEMPTS, PAIR_WINDOW)
    }

    fn allow_attempts(&self, keys: &[String], max: usize, window: Duration) -> bool {
        let Ok(mut map) = self.login_attempts.lock() else {
            return false;
        };
        let now = Instant::now();
        let mut allowed = true;
        for key in keys {
            let entry = map.entry(key.clone()).or_default();
            entry.retain(|t| now.duration_since(*t) < window);
            if entry.len() >= max {
                allowed = false;
            }
            entry.push(now);
        }
        allowed
    }

    /// Sets the public URL used in notification links and the HTTP client used to send them.
    pub fn with_notifications(mut self, base_url: Option<String>, http: reqwest::Client) -> Self {
        self.base_url = base_url;
        self.http = http;
        self
    }

    /// When the next scheduled sync of every account runs.
    pub fn next_scheduled(&self) -> Option<time::OffsetDateTime> {
        self.next_scheduled.read().ok().and_then(|g| *g)
    }

    pub fn set_next_scheduled(&self, at: Option<time::OffsetDateTime>) {
        if let Ok(mut g) = self.next_scheduled.write() {
            *g = at;
        }
    }

    pub fn is_syncing(&self, account: AccountId) -> bool {
        self.syncing.lock().map(|s| s.contains(&account)).unwrap_or(false)
    }

    /// Starts a sync in the background unless one is already running for this account.
    pub fn spawn_sync(&self, account: AccountId, trigger: RunTrigger) -> bool {
        {
            let Ok(mut running) = self.syncing.lock() else {
                return false;
            };
            if !running.insert(account) {
                return false;
            }
        }
        let state = self.clone();
        tokio::spawn(async move {
            let _permit = state.sync_permits.clone().acquire_owned().await;
            match state.syncer.run_account(account, trigger).await {
                Ok(outcome) => {
                    tracing::info!(%account, ?outcome, "sync finished");
                    super::notifications::after_run(&state, account, &outcome).await;
                }
                Err(e) => tracing::error!(%account, error = %e, "sync failed"),
            }
            state.rebuild_search().await;
            if let Ok(mut running) = state.syncing.lock() {
                running.remove(&account);
            }
        });
        true
    }
}
