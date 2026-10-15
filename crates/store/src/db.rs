//! Database pool, pragmas and migrations.

use crate::{Result, seal::Sealer};
use secrecy::SecretBox;
use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};
use std::{path::Path, str::FromStr, time::Duration};

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// Handle to the database and the operator key.
#[derive(Clone, Debug)]
pub struct Store {
    pool: SqlitePool,
    sealer: Sealer,
}

impl Store {
    /// Opens (or creates) the database file, applies pragmas and runs pending migrations.
    pub async fn open(path: &Path, key: &SecretBox<[u8; 32]>) -> Result<Self> {
        if let Some(dir) = path.parent()
            && !dir.as_os_str().is_empty()
        {
            tokio::fs::create_dir_all(dir)
                .await
                .map_err(|e| crate::StoreError::invalid(format!("cannot create {}: {e}", dir.display())))?;
        }
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .foreign_keys(true)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .max_connections(8)
            .connect_with(options)
            .await?;
        Self::from_pool(pool, key).await
    }

    /// Opens a private in-memory database, for tests.
    pub async fn open_in_memory(key: &SecretBox<[u8; 32]>) -> Result<Self> {
        let options = SqliteConnectOptions::from_str("sqlite::memory:")?.foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await?;
        Self::from_pool(pool, key).await
    }

    async fn from_pool(pool: SqlitePool, key: &SecretBox<[u8; 32]>) -> Result<Self> {
        MIGRATOR.run(&pool).await?;
        Ok(Self {
            pool,
            sealer: Sealer::new(key),
        })
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub fn sealer(&self) -> &Sealer {
        &self.sealer
    }

    /// Readiness probe used by `GET /readyz`.
    pub async fn ready(&self) -> bool {
        sqlx::query_scalar::<_, i64>("SELECT 1")
            .fetch_one(&self.pool)
            .await
            .is_ok()
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::Store;
    use secrecy::SecretBox;

    pub async fn store() -> Store {
        Store::open_in_memory(&SecretBox::new(Box::new([42u8; 32])))
            .await
            .expect("in-memory store")
    }
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn migrations_apply_and_store_is_ready() {
        let store = super::test_support::store().await;
        assert!(store.ready().await);
    }

    #[tokio::test]
    async fn file_database_is_created_with_wal() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("nested").join("gameshelf.db");
        let store = super::Store::open(&path, &secrecy::SecretBox::new(Box::new([1u8; 32])))
            .await
            .expect("open");
        assert!(store.ready().await);
        let mode: String = sqlx::query_scalar("PRAGMA journal_mode")
            .fetch_one(store.pool())
            .await
            .expect("pragma");
        assert_eq!(mode.to_lowercase(), "wal");
        let fk: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
            .fetch_one(store.pool())
            .await
            .expect("pragma");
        assert_eq!(fk, 1);
    }
}
