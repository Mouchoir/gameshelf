//! Notification targets (sealed) and the reminder ledger (FR-034 to FR-036).

use crate::{Result, Store, StoreError, ids::TextId, repo::now, seal::Sealed};
use gameshelf_core::ProfileId;
use sqlx::Row;
use time::OffsetDateTime;
use zeroize::Zeroizing;

/// A target as shown in the UI: its secret configuration stays sealed.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NotificationTarget {
    pub id: String,
    pub profile_id: ProfileId,
    pub kind: String,
    pub name: String,
    pub events: Vec<String>,
    pub enabled: bool,
    pub last_sent_at: Option<OffsetDateTime>,
    pub last_error: Option<String>,
}

fn from_row(r: &sqlx::sqlite::SqliteRow) -> Result<NotificationTarget> {
    let events: String = r.try_get("events")?;
    Ok(NotificationTarget {
        id: r.try_get("id")?,
        profile_id: ProfileId::from_txt(&r.try_get::<String, _>("profile_id")?)?,
        kind: r.try_get("kind")?,
        name: r.try_get("name")?,
        events: serde_json::from_str(&events).unwrap_or_default(),
        enabled: r.try_get::<i64, _>("enabled")? != 0,
        last_sent_at: r.try_get("last_sent_at")?,
        last_error: r.try_get("last_error")?,
    })
}

const COLUMNS: &str = "id, profile_id, kind, name, events, enabled, last_sent_at, last_error";

impl Store {
    /// Creates a target; `config` is the sender's JSON configuration, sealed at rest.
    pub async fn create_target(
        &self,
        profile: ProfileId,
        kind: &str,
        name: &str,
        config: &[u8],
        events: &[String],
    ) -> Result<String> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 64 {
            return Err(StoreError::invalid("the name must have 1 to 64 characters"));
        }
        let sealed = self.sealer().seal(config)?;
        let id = uuid::Uuid::now_v7().hyphenated().to_string();
        sqlx::query(
            "INSERT INTO notification_targets (id, profile_id, kind, name, sealed_config, nonce, key_id, events, enabled)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, 1)",
        )
        .bind(&id)
        .bind(profile.txt())
        .bind(kind)
        .bind(name)
        .bind(&sealed.ciphertext)
        .bind(&sealed.nonce)
        .bind(&sealed.key_id)
        .bind(serde_json::to_string(events).unwrap_or_else(|_| "[]".into()))
        .execute(self.pool())
        .await?;
        Ok(id)
    }

    pub async fn list_targets(&self, profile: ProfileId) -> Result<Vec<NotificationTarget>> {
        let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT {COLUMNS} FROM notification_targets WHERE profile_id = ? ORDER BY name COLLATE NOCASE"
        )))
        .bind(profile.txt())
        .fetch_all(self.pool())
        .await?;
        rows.iter().map(from_row).collect()
    }

    /// Opens the sealed configuration of a target owned by `profile`.
    pub async fn open_target(
        &self,
        profile: ProfileId,
        id: &str,
    ) -> Result<Option<(NotificationTarget, Zeroizing<Vec<u8>>)>> {
        let row = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT {COLUMNS}, sealed_config, nonce, key_id FROM notification_targets WHERE id = ? AND profile_id = ?"
        )))
        .bind(id)
        .bind(profile.txt())
        .fetch_optional(self.pool())
        .await?;
        let Some(r) = row else { return Ok(None) };
        let sealed = Sealed {
            ciphertext: r.try_get("sealed_config")?,
            nonce: r.try_get("nonce")?,
            key_id: r.try_get("key_id")?,
        };
        Ok(Some((from_row(&r)?, self.sealer().open(&sealed)?)))
    }

    pub async fn set_target_enabled(&self, profile: ProfileId, id: &str, enabled: bool) -> Result<()> {
        sqlx::query("UPDATE notification_targets SET enabled = ? WHERE id = ? AND profile_id = ?")
            .bind(i64::from(enabled))
            .bind(id)
            .bind(profile.txt())
            .execute(self.pool())
            .await?;
        Ok(())
    }

    pub async fn set_target_events(&self, profile: ProfileId, id: &str, events: &[String]) -> Result<()> {
        sqlx::query("UPDATE notification_targets SET events = ? WHERE id = ? AND profile_id = ?")
            .bind(serde_json::to_string(events).unwrap_or_else(|_| "[]".into()))
            .bind(id)
            .bind(profile.txt())
            .execute(self.pool())
            .await?;
        Ok(())
    }

    pub async fn delete_target(&self, profile: ProfileId, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM notification_targets WHERE id = ? AND profile_id = ?")
            .bind(id)
            .bind(profile.txt())
            .execute(self.pool())
            .await?;
        Ok(())
    }

    /// Records the outcome of a delivery. The error must already be free of URLs and tokens.
    pub async fn record_delivery(&self, id: &str, error: Option<&str>) -> Result<()> {
        match error {
            Some(e) => {
                sqlx::query("UPDATE notification_targets SET last_error = ? WHERE id = ?")
                    .bind(e.chars().take(300).collect::<String>())
                    .bind(id)
                    .execute(self.pool())
                    .await?;
            }
            None => {
                sqlx::query(
                    "UPDATE notification_targets SET last_sent_at = ?, last_error = NULL WHERE id = ?",
                )
                .bind(now())
                .bind(id)
                .execute(self.pool())
                .await?;
            }
        }
        Ok(())
    }

    /// Claims today's reminder for an issue on a target: true at most once per day (FR-035).
    pub async fn claim_reminder(&self, target: &str, issue: &str) -> Result<bool> {
        let today = now().date().to_string();
        let done = sqlx::query(
            "INSERT INTO reminder_ledger (target_id, issue_key, last_sent_on) VALUES (?, ?, ?)
             ON CONFLICT (target_id, issue_key) DO UPDATE SET last_sent_on = excluded.last_sent_on
             WHERE reminder_ledger.last_sent_on <> excluded.last_sent_on",
        )
        .bind(target)
        .bind(issue)
        .bind(&today)
        .execute(self.pool())
        .await?;
        Ok(done.rows_affected() > 0)
    }
}

#[cfg(test)]
mod tests {
    use crate::{db::test_support::store, repo::accounts::tests::profile};

    #[tokio::test]
    async fn targets_are_sealed_owned_and_reminders_daily() {
        let store = store().await;
        let p = profile(&store, "player-one", "Player One").await;
        let other = profile(&store, "player-two", "Player Two").await;
        let config =
            br#"{"kind":"ntfy","server":"https://ntfy.example.com","topic":"t","token":"SECRET-TOKEN"}"#;
        let id = store
            .create_target(p, "ntfy", "Phone", config, &["game_added".into()])
            .await
            .expect("create");

        let raw: Vec<u8> = sqlx::query_scalar("SELECT sealed_config FROM notification_targets")
            .fetch_one(store.pool())
            .await
            .expect("raw");
        assert!(!String::from_utf8_lossy(&raw).contains("SECRET-TOKEN"));

        let (target, cfg) = store.open_target(p, &id).await.expect("open").expect("present");
        assert_eq!(target.events, vec!["game_added".to_string()]);
        assert_eq!(cfg.as_slice(), config);
        assert!(
            store.open_target(other, &id).await.expect("open").is_none(),
            "another profile cannot read it"
        );

        assert!(store.claim_reminder(&id, "reconnect:a").await.expect("claim"));
        assert!(
            !store
                .claim_reminder(&id, "reconnect:a")
                .await
                .expect("claim again")
        );
        assert!(
            store
                .claim_reminder(&id, "reconnect:b")
                .await
                .expect("other issue")
        );

        store
            .record_delivery(&id, Some("the target refused the message (status 401)"))
            .await
            .expect("record");
        let listed = store.list_targets(p).await.expect("list");
        assert_eq!(
            listed[0].last_error.as_deref(),
            Some("the target refused the message (status 401)")
        );
        store.delete_target(p, &id).await.expect("delete");
        assert!(store.list_targets(p).await.expect("list").is_empty());
    }
}
