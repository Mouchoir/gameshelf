//! Platform accounts (FR-001, FR-004).

use crate::{Result, Store, StoreError, ids::TextId, repo::now};
use gameshelf_core::{AccountId, AgentId, ProfileId, account_status::AccountStatus, platform};
use sqlx::Row;
use time::OffsetDateTime;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Account {
    pub id: AccountId,
    pub profile_id: ProfileId,
    pub platform: String,
    pub external_account_id: String,
    pub label: String,
    pub display_name: Option<String>,
    pub status: AccountStatus,
    pub sync_enabled: bool,
    pub schedule: Option<String>,
    pub consecutive_failures: u32,
    pub last_sync_at: Option<OffsetDateTime>,
    pub last_success_at: Option<OffsetDateTime>,
    pub created_at: OffsetDateTime,
    /// Set when an agent created the account because no connected account matched; such accounts are fed
    /// by the agent only.
    pub agent_id: Option<AgentId>,
}

#[derive(Debug)]
pub struct NewAccount<'a> {
    pub profile_id: ProfileId,
    pub platform: &'a str,
    pub external_account_id: &'a str,
    pub display_name: Option<&'a str>,
    /// Defaults to the display name, then to the external id.
    pub label: Option<&'a str>,
    pub status: AccountStatus,
}

const COLUMNS: &str = "id, profile_id, platform, external_account_id, label, display_name, status, sync_enabled, \
    schedule, consecutive_failures, last_sync_at, last_success_at, created_at, agent_id";

fn from_row(row: &sqlx::sqlite::SqliteRow) -> Result<Account> {
    let status: String = row.try_get("status")?;
    Ok(Account {
        id: AccountId::from_txt(&row.try_get::<String, _>("id")?)?,
        profile_id: ProfileId::from_txt(&row.try_get::<String, _>("profile_id")?)?,
        platform: row.try_get("platform")?,
        external_account_id: row.try_get("external_account_id")?,
        label: row.try_get("label")?,
        display_name: row.try_get("display_name")?,
        status: AccountStatus::parse(&status)
            .ok_or_else(|| StoreError::Corrupt(format!("status {status:?}")))?,
        sync_enabled: row.try_get::<i64, _>("sync_enabled")? != 0,
        schedule: row.try_get("schedule")?,
        consecutive_failures: u32::try_from(row.try_get::<i64, _>("consecutive_failures")?).unwrap_or(0),
        last_sync_at: row.try_get("last_sync_at")?,
        last_success_at: row.try_get("last_success_at")?,
        created_at: row.try_get("created_at")?,
        agent_id: crate::ids::opt_id(row.try_get("agent_id")?)?,
    })
}

fn clamp_label(s: &str) -> String {
    let trimmed = s.trim();
    let mut out: String = trimmed.chars().take(64).collect();
    if out.is_empty() {
        out = "Account".to_string();
    }
    out
}

impl Store {
    /// Connects an account. Refuses an account already connected anywhere on the instance and names the profile
    /// holding it (spec edge cases).
    pub async fn create_account(&self, new: NewAccount<'_>) -> Result<Account> {
        if !platform::is_known(new.platform) {
            return Err(StoreError::invalid(format!(
                "unknown platform {:?}",
                new.platform
            )));
        }
        if new.external_account_id.trim().is_empty() {
            return Err(StoreError::invalid("the platform account id is empty"));
        }
        if let Some(existing) = self
            .find_account_by_external(new.platform, new.external_account_id)
            .await?
        {
            let holder = self
                .get_profile(existing.profile_id)
                .await?
                .map(|p| p.display_name)
                .unwrap_or_else(|| "another profile".to_string());
            return Err(StoreError::AccountTaken { holder });
        }
        let label = clamp_label(new.label.or(new.display_name).unwrap_or(new.external_account_id));
        let id = AccountId::new();
        let now = now();
        sqlx::query(
            "INSERT INTO platform_accounts (id, profile_id, platform, external_account_id, label, display_name, status,
                created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id.txt())
        .bind(new.profile_id.txt())
        .bind(new.platform)
        .bind(new.external_account_id)
        .bind(&label)
        .bind(new.display_name)
        .bind(new.status.as_str())
        .bind(now)
        .bind(now)
        .execute(self.pool())
        .await?;
        self.get_account(id).await?.ok_or(StoreError::NotFound)
    }

    pub async fn get_account(&self, id: AccountId) -> Result<Option<Account>> {
        let row = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT {COLUMNS} FROM platform_accounts WHERE id = ?"
        )))
        .bind(id.txt())
        .fetch_optional(self.pool())
        .await?;
        row.as_ref().map(from_row).transpose()
    }

    pub async fn find_account_by_external(&self, platform: &str, external: &str) -> Result<Option<Account>> {
        let row = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT {COLUMNS} FROM platform_accounts WHERE platform = ? AND external_account_id = ?"
        )))
        .bind(platform)
        .bind(external)
        .fetch_optional(self.pool())
        .await?;
        row.as_ref().map(from_row).transpose()
    }

    /// Creates an account fed by an agent: the scheduler leaves it alone because it has no credential.
    pub async fn create_agent_account(&self, new: NewAccount<'_>, agent: AgentId) -> Result<Account> {
        let account = self.create_account(new).await?;
        sqlx::query("UPDATE platform_accounts SET agent_id = ?, sync_enabled = 0 WHERE id = ?")
            .bind(agent.txt())
            .bind(account.id.txt())
            .execute(self.pool())
            .await?;
        self.get_account(account.id).await?.ok_or(StoreError::NotFound)
    }

    /// Turns an agent-created account into a regular one once the player connects it.
    pub async fn release_from_agent(&self, id: AccountId) -> Result<()> {
        sqlx::query(
            "UPDATE platform_accounts SET agent_id = NULL, sync_enabled = 1, updated_at = ?
             WHERE id = ? AND agent_id IS NOT NULL",
        )
        .bind(now())
        .bind(id.txt())
        .execute(self.pool())
        .await?;
        Ok(())
    }

    /// Accounts of one profile, by platform then label.
    pub async fn list_accounts(&self, profile: ProfileId) -> Result<Vec<Account>> {
        let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT {COLUMNS} FROM platform_accounts WHERE profile_id = ? ORDER BY platform, label COLLATE NOCASE"
        )))
        .bind(profile.txt())
        .fetch_all(self.pool())
        .await?;
        rows.iter().map(from_row).collect()
    }

    /// Every account the scheduler may sync.
    pub async fn list_syncable_accounts(&self) -> Result<Vec<Account>> {
        let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT {COLUMNS} FROM platform_accounts
             WHERE sync_enabled = 1 AND status IN ('active', 'degraded') ORDER BY platform"
        )))
        .fetch_all(self.pool())
        .await?;
        rows.iter().map(from_row).collect()
    }

    /// Renames an account; only its owner may do it.
    pub async fn set_account_label(&self, id: AccountId, owner: ProfileId, label: &str) -> Result<()> {
        let done = sqlx::query(
            "UPDATE platform_accounts SET label = ?, updated_at = ? WHERE id = ? AND profile_id = ?",
        )
        .bind(clamp_label(label))
        .bind(now())
        .bind(id.txt())
        .bind(owner.txt())
        .execute(self.pool())
        .await?;
        if done.rows_affected() == 0 {
            return Err(StoreError::NotFound);
        }
        Ok(())
    }

    pub async fn set_account_status(
        &self,
        id: AccountId,
        status: AccountStatus,
        failures: u32,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE platform_accounts SET status = ?, consecutive_failures = ?, updated_at = ? WHERE id = ?",
        )
        .bind(status.as_str())
        .bind(i64::from(failures))
        .bind(now())
        .bind(id.txt())
        .execute(self.pool())
        .await?;
        Ok(())
    }

    /// Records the end of a sync attempt.
    pub async fn mark_account_synced(&self, id: AccountId, success: bool) -> Result<()> {
        let now = now();
        let sql = if success {
            "UPDATE platform_accounts SET last_sync_at = ?, last_success_at = ?, updated_at = ? WHERE id = ?"
        } else {
            "UPDATE platform_accounts SET last_sync_at = ?, last_success_at = last_success_at, updated_at = ? WHERE id = ?"
        };
        let mut q = sqlx::query(sql).bind(now);
        if success {
            q = q.bind(now);
        }
        q.bind(now).bind(id.txt()).execute(self.pool()).await?;
        Ok(())
    }

    /// Disconnects an account (FR-004): the credential is always deleted; the ownership data is deleted with the
    /// account when `delete_data` is true, otherwise kept and the account is disabled.
    pub async fn disconnect_account(&self, id: AccountId, owner: ProfileId, delete_data: bool) -> Result<()> {
        let account = self.get_account(id).await?.ok_or(StoreError::NotFound)?;
        if account.profile_id != owner {
            return Err(StoreError::NotFound);
        }
        if delete_data {
            sqlx::query("DELETE FROM platform_accounts WHERE id = ?")
                .bind(id.txt())
                .execute(self.pool())
                .await?;
        } else {
            self.delete_credential(id).await?;
            self.set_account_status(id, AccountStatus::Disabled, 0).await?;
            sqlx::query("UPDATE platform_accounts SET sync_enabled = 0 WHERE id = ?")
                .bind(id.txt())
                .execute(self.pool())
                .await?;
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::{NewProfile, db::test_support::store};
    use gameshelf_core::Role;
    use secrecy::SecretString;

    pub(crate) async fn profile(store: &Store, username: &str, display: &str) -> ProfileId {
        let password = SecretString::from("a long enough password".to_string());
        store
            .create_profile(NewProfile {
                username,
                display_name: display,
                email: None,
                password: &password,
                role: Role::Admin,
            })
            .await
            .expect("profile")
            .id
    }

    #[tokio::test]
    async fn create_list_and_label() {
        let store = store().await;
        let p = profile(&store, "player-one", "Player One").await;
        let a = store
            .create_account(NewAccount {
                profile_id: p,
                platform: "steam",
                external_account_id: "76561190000000001",
                display_name: Some("example-user"),
                label: None,
                status: AccountStatus::Active,
            })
            .await
            .expect("create");
        assert_eq!(a.label, "example-user");
        store.set_account_label(a.id, p, "Main").await.expect("label");
        let list = store.list_accounts(p).await.expect("list");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].label, "Main");
    }

    #[tokio::test]
    async fn the_same_account_cannot_be_connected_twice() {
        let store = store().await;
        let p1 = profile(&store, "player-one", "Player One").await;
        let p2 = profile(&store, "player-two", "Player Two").await;
        let new = |p| NewAccount {
            profile_id: p,
            platform: "steam",
            external_account_id: "76561190000000001",
            display_name: None,
            label: None,
            status: AccountStatus::Active,
        };
        store.create_account(new(p1)).await.expect("first");
        match store.create_account(new(p2)).await {
            Err(StoreError::AccountTaken { holder }) => assert_eq!(holder, "Player One"),
            other => panic!("expected AccountTaken, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn unknown_platforms_are_refused() {
        let store = store().await;
        let p = profile(&store, "player-one", "Player One").await;
        let err = store
            .create_account(NewAccount {
                profile_id: p,
                platform: "nonexistent",
                external_account_id: "x",
                display_name: None,
                label: None,
                status: AccountStatus::Active,
            })
            .await
            .expect_err("must fail");
        assert!(matches!(err, StoreError::Invalid(_)));
    }

    #[tokio::test]
    async fn disconnect_keeps_or_deletes_data() {
        let store = store().await;
        let p = profile(&store, "player-one", "Player One").await;
        let mk = |ext: &'static str| NewAccount {
            profile_id: p,
            platform: "epic",
            external_account_id: ext,
            display_name: None,
            label: None,
            status: AccountStatus::Active,
        };
        let keep = store.create_account(mk("acct-a")).await.expect("a");
        let drop = store.create_account(mk("acct-b")).await.expect("b");
        store.disconnect_account(keep.id, p, false).await.expect("keep");
        store.disconnect_account(drop.id, p, true).await.expect("drop");
        let kept = store
            .get_account(keep.id)
            .await
            .expect("get")
            .expect("still there");
        assert_eq!(kept.status, AccountStatus::Disabled);
        assert!(!kept.sync_enabled);
        assert!(store.get_account(drop.id).await.expect("get").is_none());
    }
}
