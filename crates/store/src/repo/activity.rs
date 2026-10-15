//! Activity feed (FR-024).

use crate::{
    Result, Store,
    ids::{TextId, opt_id},
    repo::now,
};
use gameshelf_core::{AccountId, GameId, ProfileId, RunId};
use sqlx::Row;
use time::OffsetDateTime;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityKind {
    GameAdded,
    GameUnavailable,
    AccountNeedsReconnect,
    AccountDegraded,
    AgentSilent,
    SyncFailed,
}

impl ActivityKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ActivityKind::GameAdded => "game_added",
            ActivityKind::GameUnavailable => "game_unavailable",
            ActivityKind::AccountNeedsReconnect => "account_needs_reconnect",
            ActivityKind::AccountDegraded => "account_degraded",
            ActivityKind::AgentSilent => "agent_silent",
            ActivityKind::SyncFailed => "sync_failed",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [
            ActivityKind::GameAdded,
            ActivityKind::GameUnavailable,
            ActivityKind::AccountNeedsReconnect,
            ActivityKind::AccountDegraded,
            ActivityKind::AgentSilent,
            ActivityKind::SyncFailed,
        ]
        .into_iter()
        .find(|k| k.as_str() == s)
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ActivityEvent {
    pub id: String,
    pub profile_id: ProfileId,
    pub kind: ActivityKind,
    pub game_id: Option<GameId>,
    pub account_id: Option<AccountId>,
    pub payload: serde_json::Value,
    pub run_id: Option<RunId>,
    pub created_at: OffsetDateTime,
    pub read: bool,
}

impl Store {
    pub async fn add_activity(
        &self,
        profile: ProfileId,
        kind: ActivityKind,
        game: Option<GameId>,
        account: Option<AccountId>,
        payload: &serde_json::Value,
        run: Option<RunId>,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO activity_events (id, profile_id, kind, game_id, account_id, payload, run_id, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(uuid::Uuid::now_v7().hyphenated().to_string())
        .bind(profile.txt())
        .bind(kind.as_str())
        .bind(game.map(|g| g.txt()))
        .bind(account.map(|a| a.txt()))
        .bind(payload.to_string())
        .bind(run.map(|r| r.txt()))
        .bind(now())
        .execute(self.pool())
        .await?;
        Ok(())
    }

    /// Latest events of a profile, newest first.
    pub async fn list_activity(&self, profile: ProfileId, limit: i64) -> Result<Vec<ActivityEvent>> {
        let rows = sqlx::query(
            "SELECT id, profile_id, kind, game_id, account_id, payload, run_id, created_at, read_at
             FROM activity_events WHERE profile_id = ? ORDER BY created_at DESC, id DESC LIMIT ?",
        )
        .bind(profile.txt())
        .bind(limit)
        .fetch_all(self.pool())
        .await?;
        rows.iter()
            .filter_map(|r| {
                let kind: String = r.try_get("kind").ok()?;
                Some((r, ActivityKind::parse(&kind)?))
            })
            .map(|(r, kind)| {
                let payload: String = r.try_get("payload")?;
                let read_at: Option<OffsetDateTime> = r.try_get("read_at")?;
                Ok(ActivityEvent {
                    id: r.try_get("id")?,
                    profile_id: ProfileId::from_txt(&r.try_get::<String, _>("profile_id")?)?,
                    kind,
                    game_id: opt_id(r.try_get("game_id")?)?,
                    account_id: opt_id(r.try_get("account_id")?)?,
                    payload: serde_json::from_str(&payload).unwrap_or(serde_json::Value::Null),
                    run_id: opt_id(r.try_get("run_id")?)?,
                    created_at: r.try_get("created_at")?,
                    read: read_at.is_some(),
                })
            })
            .collect()
    }

    pub async fn unread_activity_count(&self, profile: ProfileId) -> Result<i64> {
        Ok(sqlx::query_scalar(
            "SELECT count(*) FROM activity_events WHERE profile_id = ? AND read_at IS NULL",
        )
        .bind(profile.txt())
        .fetch_one(self.pool())
        .await?)
    }

    pub async fn mark_activity_read(&self, profile: ProfileId) -> Result<()> {
        sqlx::query("UPDATE activity_events SET read_at = ? WHERE profile_id = ? AND read_at IS NULL")
            .bind(now())
            .bind(profile.txt())
            .execute(self.pool())
            .await?;
        Ok(())
    }
}
