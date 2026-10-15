//! SQLite persistence for gameshelf (research D3).
//!
//! Every ownership read by the web layer goes through [`visibility`] (research D8).

mod db;
mod error;
mod ids;
pub mod password;
pub mod repo;
pub mod seal;
pub mod session_store;
pub mod visibility;

pub use visibility::{LibraryGame, LibraryPage, LibraryQuery, Sort, VisibleOwnership};

pub use db::Store;
pub use error::StoreError;
pub use repo::{
    accounts::{Account, NewAccount},
    activity::{ActivityEvent, ActivityKind},
    catalog::{GameSummary, NewProduct, Product},
    credentials::{CredentialKind, CredentialMeta, OpenedCredential},
    enrichment::EnrichmentValue,
    notify::NotificationTarget,
    ownership::{ApplyOutcome, OwnershipSource, SnapshotItem},
    profiles::{NewProfile, Profile},
    runs::{RunCapability, RunResult, RunStatus, RunTrigger, SyncRun},
};
pub use session_store::SqliteSessionStore;

/// Result alias for store operations.
pub type Result<T, E = StoreError> = std::result::Result<T, E>;
