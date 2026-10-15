//! Sync runs (FR-022) and their retention.

use crate::{
    Result, Store,
    ids::{TextId, opt_id},
    repo::now,
};
use gameshelf_core::{AccountId, RunId};
use sqlx::Row;
use time::OffsetDateTime;

/// Maximum stored length of an error message.
pub const MAX_ERROR_LEN: usize = 500;

/// Runs older than this many days are purged.
pub const RETENTION_DAYS: i64 = 90;

macro_rules! str_enum {
    ($name:ident { $($variant:ident => $s:literal),* $(,)? }) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum $name { $($variant),* }

        impl $name {
            pub fn as_str(self) -> &'static str {
                match self { $($name::$variant => $s),* }
            }

            pub fn parse(s: &str) -> Option<Self> {
                match s { $($s => Some($name::$variant),)* _ => None }
            }
        }
    };
}

str_enum!(RunCapability {
    Library => "library",
    Playtime => "playtime",
    Achievements => "achievements",
    Enrichment => "enrichment",
    Catalog => "catalog",
    Metadata => "metadata",
});

str_enum!(RunTrigger {
    Schedule => "schedule",
    Manual => "manual",
    Agent => "agent",
    Startup => "startup",
});

str_enum!(RunStatus {
    Running => "running",
    Succeeded => "succeeded",
    Partial => "partial",
    Failed => "failed",
    Skipped => "skipped",
});

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SyncRun {
    pub id: RunId,
    pub account_id: Option<AccountId>,
    pub platform: String,
    pub capability: RunCapability,
    pub trigger: RunTrigger,
    pub status: RunStatus,
    pub complete_listing: bool,
    pub added: u32,
    pub removed: u32,
    pub updated: u32,
    pub error_kind: Option<String>,
    pub error_message: Option<String>,
    pub started_at: OffsetDateTime,
    pub finished_at: Option<OffsetDateTime>,
}

/// Final figures of a run.
#[derive(Clone, Debug, Default)]
pub struct RunResult {
    pub status: Option<RunStatus>,
    pub complete_listing: bool,
    pub added: u32,
    pub removed: u32,
    pub updated: u32,
    pub error_kind: Option<String>,
    pub error_message: Option<String>,
}

fn truncate(msg: &str) -> String {
    if msg.chars().count() <= MAX_ERROR_LEN {
        return msg.to_string();
    }
    let mut out: String = msg.chars().take(MAX_ERROR_LEN - 3).collect();
    out.push_str("...");
    out
}

fn from_row(r: &sqlx::sqlite::SqliteRow) -> Result<SyncRun> {
    let cap: String = r.try_get("capability")?;
    let trig: String = r.try_get("trigger")?;
    let status: String = r.try_get("status")?;
    let n = |col: &str| -> Result<u32> { Ok(u32::try_from(r.try_get::<i64, _>(col)?).unwrap_or(0)) };
    Ok(SyncRun {
        id: RunId::from_txt(&r.try_get::<String, _>("id")?)?,
        account_id: opt_id(r.try_get("account_id")?)?,
        platform: r.try_get("platform")?,
        capability: RunCapability::parse(&cap).unwrap_or(RunCapability::Library),
        trigger: RunTrigger::parse(&trig).unwrap_or(RunTrigger::Schedule),
        status: RunStatus::parse(&status).unwrap_or(RunStatus::Failed),
        complete_listing: r.try_get::<i64, _>("complete_listing")? != 0,
        added: n("added")?,
        removed: n("removed")?,
        updated: n("updated")?,
        error_kind: r.try_get("error_kind")?,
        error_message: r.try_get("error_message")?,
        started_at: r.try_get("started_at")?,
        finished_at: r.try_get("finished_at")?,
    })
}

impl Store {
    pub async fn start_run(
        &self,
        account: Option<AccountId>,
        platform: &str,
        capability: RunCapability,
        trigger: RunTrigger,
    ) -> Result<RunId> {
        let id = RunId::new();
        sqlx::query(
            "INSERT INTO sync_runs (id, account_id, platform, capability, trigger, status, started_at)
             VALUES (?, ?, ?, ?, ?, 'running', ?)",
        )
        .bind(id.txt())
        .bind(account.map(|a| a.txt()))
        .bind(platform)
        .bind(capability.as_str())
        .bind(trigger.as_str())
        .bind(now())
        .execute(self.pool())
        .await?;
        Ok(id)
    }

    /// Closes a run. The error message is truncated; callers must have redacted it already.
    pub async fn finish_run(&self, id: RunId, result: &RunResult) -> Result<()> {
        let status = result.status.unwrap_or(if result.error_kind.is_some() {
            RunStatus::Failed
        } else {
            RunStatus::Succeeded
        });
        sqlx::query(
            "UPDATE sync_runs SET status = ?, complete_listing = ?, added = ?, removed = ?, updated = ?,
                error_kind = ?, error_message = ?, finished_at = ? WHERE id = ?",
        )
        .bind(status.as_str())
        .bind(i64::from(result.complete_listing))
        .bind(i64::from(result.added))
        .bind(i64::from(result.removed))
        .bind(i64::from(result.updated))
        .bind(&result.error_kind)
        .bind(result.error_message.as_deref().map(truncate))
        .bind(now())
        .bind(id.txt())
        .execute(self.pool())
        .await?;
        Ok(())
    }

    pub async fn list_runs(&self, account: AccountId, limit: i64) -> Result<Vec<SyncRun>> {
        let rows =
            sqlx::query("SELECT * FROM sync_runs WHERE account_id = ? ORDER BY started_at DESC LIMIT ?")
                .bind(account.txt())
                .bind(limit)
                .fetch_all(self.pool())
                .await?;
        rows.iter().map(from_row).collect()
    }

    /// Marks runs left `running` by a crash as failed (called at start-up).
    pub async fn fail_interrupted_runs(&self) -> Result<u64> {
        let done = sqlx::query(
            "UPDATE sync_runs SET status = 'failed', error_kind = 'transient',
                error_message = 'interrupted by a restart', finished_at = ? WHERE status = 'running'",
        )
        .bind(now())
        .execute(self.pool())
        .await?;
        Ok(done.rows_affected())
    }

    /// Deletes runs older than the retention period.
    pub async fn purge_old_runs(&self) -> Result<u64> {
        let cutoff = now() - time::Duration::days(RETENTION_DAYS);
        let done = sqlx::query("DELETE FROM sync_runs WHERE started_at < ?")
            .bind(cutoff)
            .execute(self.pool())
            .await?;
        Ok(done.rows_affected())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_support::store;

    #[tokio::test]
    async fn run_lifecycle_and_truncation() {
        let store = store().await;
        let id = store
            .start_run(None, "steam", RunCapability::Catalog, RunTrigger::Manual)
            .await
            .expect("start");
        let long = "x".repeat(800);
        store
            .finish_run(
                id,
                &RunResult {
                    error_kind: Some("parse".into()),
                    error_message: Some(long),
                    ..Default::default()
                },
            )
            .await
            .expect("finish");
        let (status, msg): (String, String) = sqlx::query_as("SELECT status, error_message FROM sync_runs")
            .fetch_one(store.pool())
            .await
            .expect("row");
        assert_eq!(status, "failed");
        assert_eq!(msg.chars().count(), MAX_ERROR_LEN);
    }

    #[tokio::test]
    async fn interrupted_runs_are_closed() {
        let store = store().await;
        store
            .start_run(None, "steam", RunCapability::Catalog, RunTrigger::Startup)
            .await
            .expect("start");
        assert_eq!(store.fail_interrupted_runs().await.expect("fail"), 1);
        assert_eq!(store.fail_interrupted_runs().await.expect("again"), 0);
    }
}
