//! Per-profile preferences such as the last view, sort and filters (FR-015).

use crate::{Result, Store, ids::TextId};
use gameshelf_core::ProfileId;

impl Store {
    pub async fn get_pref(&self, profile: ProfileId, key: &str) -> Result<Option<serde_json::Value>> {
        let value: Option<String> =
            sqlx::query_scalar("SELECT value FROM preferences WHERE profile_id = ? AND key = ?")
                .bind(profile.txt())
                .bind(key)
                .fetch_optional(self.pool())
                .await?;
        Ok(value.and_then(|v| serde_json::from_str(&v).ok()))
    }

    pub async fn set_pref(&self, profile: ProfileId, key: &str, value: &serde_json::Value) -> Result<()> {
        sqlx::query(
            "INSERT INTO preferences (profile_id, key, value) VALUES (?, ?, ?)
             ON CONFLICT (profile_id, key) DO UPDATE SET value = excluded.value",
        )
        .bind(profile.txt())
        .bind(key)
        .bind(value.to_string())
        .execute(self.pool())
        .await?;
        Ok(())
    }
}
