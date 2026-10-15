//! A `tower-sessions` store on sqlx 0.9 (research D4).

use crate::Store;
use async_trait::async_trait;
use sqlx::Row;
use time::OffsetDateTime;
use tower_sessions::{
    SessionStore,
    session::{Id, Record},
    session_store,
};

/// Session data key holding the signed-in profile id.
pub const PROFILE_KEY: &str = "profile_id";

#[derive(Clone, Debug)]
pub struct SqliteSessionStore {
    store: Store,
}

impl SqliteSessionStore {
    pub fn new(store: Store) -> Self {
        Self { store }
    }

    /// Deletes expired sessions; run periodically.
    pub async fn purge_expired(&self) -> Result<u64, crate::StoreError> {
        let done = sqlx::query("DELETE FROM sessions WHERE expires_at < ?")
            .bind(OffsetDateTime::now_utc())
            .execute(self.store.pool())
            .await?;
        Ok(done.rows_affected())
    }
}

fn backend(e: impl std::fmt::Display) -> session_store::Error {
    session_store::Error::Backend(e.to_string())
}

#[async_trait]
impl SessionStore for SqliteSessionStore {
    async fn save(&self, record: &Record) -> session_store::Result<()> {
        let data =
            serde_json::to_vec(&record.data).map_err(|e| session_store::Error::Encode(e.to_string()))?;
        let profile = record
            .data
            .get(PROFILE_KEY)
            .and_then(|v| v.as_str())
            .map(str::to_string);
        sqlx::query(
            "INSERT INTO sessions (id, profile_id, data, expires_at) VALUES (?, ?, ?, ?)
             ON CONFLICT (id) DO UPDATE SET profile_id = excluded.profile_id, data = excluded.data,
                expires_at = excluded.expires_at",
        )
        .bind(record.id.to_string())
        .bind(profile)
        .bind(data)
        .bind(record.expiry_date)
        .execute(self.store.pool())
        .await
        .map_err(backend)?;
        Ok(())
    }

    async fn load(&self, id: &Id) -> session_store::Result<Option<Record>> {
        let row = sqlx::query("SELECT data, expires_at FROM sessions WHERE id = ? AND expires_at > ?")
            .bind(id.to_string())
            .bind(OffsetDateTime::now_utc())
            .fetch_optional(self.store.pool())
            .await
            .map_err(backend)?;
        let Some(row) = row else { return Ok(None) };
        let data: Vec<u8> = row.try_get("data").map_err(backend)?;
        let expiry_date: OffsetDateTime = row.try_get("expires_at").map_err(backend)?;
        let data = serde_json::from_slice(&data).map_err(|e| session_store::Error::Decode(e.to_string()))?;
        Ok(Some(Record {
            id: *id,
            data,
            expiry_date,
        }))
    }

    async fn delete(&self, id: &Id) -> session_store::Result<()> {
        sqlx::query("DELETE FROM sessions WHERE id = ?")
            .bind(id.to_string())
            .execute(self.store.pool())
            .await
            .map_err(backend)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_support::store;
    use std::collections::HashMap;

    #[tokio::test]
    async fn save_load_delete_and_expiry() {
        let store = store().await;
        let sessions = SqliteSessionStore::new(store.clone());
        let mut data = HashMap::new();
        data.insert("hello".to_string(), serde_json::json!("world"));
        let record = Record {
            id: Id::default(),
            data,
            expiry_date: OffsetDateTime::now_utc() + time::Duration::hours(1),
        };
        sessions.save(&record).await.expect("save");
        let loaded = sessions.load(&record.id).await.expect("load").expect("present");
        assert_eq!(loaded.data.get("hello"), Some(&serde_json::json!("world")));
        sessions.delete(&record.id).await.expect("delete");
        assert!(sessions.load(&record.id).await.expect("load").is_none());

        let expired = Record {
            id: Id::default(),
            data: HashMap::new(),
            expiry_date: OffsetDateTime::now_utc() - time::Duration::hours(1),
        };
        sessions.save(&expired).await.expect("save");
        assert!(sessions.load(&expired.id).await.expect("load").is_none());
        assert_eq!(sessions.purge_expired().await.expect("purge"), 1);
    }
}
