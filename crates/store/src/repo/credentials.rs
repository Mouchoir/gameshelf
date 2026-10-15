//! Sealed platform credentials (FR-041). Plaintext never leaves this module except as zeroizing bytes handed
//! to a connector.

use crate::{Result, Store, StoreError, ids::TextId, repo::now, seal::Sealed};
use gameshelf_core::AccountId;
use sqlx::Row;
use time::OffsetDateTime;
use zeroize::Zeroizing;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialKind {
    ApiKey,
    Oauth,
    SessionCookie,
    Token,
}

impl CredentialKind {
    pub fn as_str(self) -> &'static str {
        match self {
            CredentialKind::ApiKey => "api_key",
            CredentialKind::Oauth => "oauth",
            CredentialKind::SessionCookie => "session_cookie",
            CredentialKind::Token => "token",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s {
            "api_key" => Some(CredentialKind::ApiKey),
            "oauth" => Some(CredentialKind::Oauth),
            "session_cookie" => Some(CredentialKind::SessionCookie),
            "token" => Some(CredentialKind::Token),
            _ => None,
        }
    }
}

/// Non-secret facts about a credential, safe to show in the UI.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CredentialMeta {
    pub kind: CredentialKind,
    pub expires_at: Option<OffsetDateTime>,
    pub refreshed_at: Option<OffsetDateTime>,
    pub created_at: OffsetDateTime,
}

/// An opened credential: JSON payload bytes, wiped from memory when dropped.
pub struct OpenedCredential {
    pub kind: CredentialKind,
    pub json: Zeroizing<Vec<u8>>,
    pub expires_at: Option<OffsetDateTime>,
}

impl std::fmt::Debug for OpenedCredential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenedCredential")
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}

impl Store {
    /// Stores (or replaces) the credential of an account. `json` is the connector's payload.
    pub async fn put_credential(
        &self,
        account: AccountId,
        kind: CredentialKind,
        json: &[u8],
        expires_at: Option<OffsetDateTime>,
    ) -> Result<()> {
        let sealed = self.sealer().seal(json)?;
        let now = now();
        sqlx::query(
            "INSERT INTO credentials (id, account_id, kind, sealed, nonce, key_id, expires_at, refreshed_at, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, NULL, ?)
             ON CONFLICT (account_id) DO UPDATE SET kind = excluded.kind, sealed = excluded.sealed,
                nonce = excluded.nonce, key_id = excluded.key_id, expires_at = excluded.expires_at,
                refreshed_at = ?, created_at = credentials.created_at",
        )
        .bind(uuid::Uuid::now_v7().hyphenated().to_string())
        .bind(account.txt())
        .bind(kind.as_str())
        .bind(&sealed.ciphertext)
        .bind(&sealed.nonce)
        .bind(&sealed.key_id)
        .bind(expires_at)
        .bind(now)
        .bind(now)
        .execute(self.pool())
        .await?;
        Ok(())
    }

    /// Opens the credential of an account for a connector.
    pub async fn open_credential(&self, account: AccountId) -> Result<Option<OpenedCredential>> {
        let row = sqlx::query(
            "SELECT kind, sealed, nonce, key_id, expires_at FROM credentials WHERE account_id = ?",
        )
        .bind(account.txt())
        .fetch_optional(self.pool())
        .await?;
        let Some(row) = row else { return Ok(None) };
        let kind: String = row.try_get("kind")?;
        let sealed = Sealed {
            ciphertext: row.try_get("sealed")?,
            nonce: row.try_get("nonce")?,
            key_id: row.try_get("key_id")?,
        };
        let json = self.sealer().open(&sealed)?;
        Ok(Some(OpenedCredential {
            kind: CredentialKind::parse(&kind)
                .ok_or_else(|| StoreError::Corrupt(format!("kind {kind:?}")))?,
            json,
            expires_at: row.try_get("expires_at")?,
        }))
    }

    pub async fn credential_meta(&self, account: AccountId) -> Result<Option<CredentialMeta>> {
        let row = sqlx::query(
            "SELECT kind, expires_at, refreshed_at, created_at FROM credentials WHERE account_id = ?",
        )
        .bind(account.txt())
        .fetch_optional(self.pool())
        .await?;
        let Some(row) = row else { return Ok(None) };
        let kind: String = row.try_get("kind")?;
        Ok(Some(CredentialMeta {
            kind: CredentialKind::parse(&kind)
                .ok_or_else(|| StoreError::Corrupt(format!("kind {kind:?}")))?,
            expires_at: row.try_get("expires_at")?,
            refreshed_at: row.try_get("refreshed_at")?,
            created_at: row.try_get("created_at")?,
        }))
    }

    pub async fn delete_credential(&self, account: AccountId) -> Result<()> {
        sqlx::query("DELETE FROM credentials WHERE account_id = ?")
            .bind(account.txt())
            .execute(self.pool())
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{NewAccount, db::test_support::store, repo::accounts::tests::profile};
    use gameshelf_core::account_status::AccountStatus;

    #[tokio::test]
    async fn credentials_are_sealed_at_rest_and_replaceable() {
        let store = store().await;
        let p = profile(&store, "player-one", "Player One").await;
        let a = store
            .create_account(NewAccount {
                profile_id: p,
                platform: "steam",
                external_account_id: "76561190000000001",
                display_name: None,
                label: None,
                status: AccountStatus::Active,
            })
            .await
            .expect("account");
        store
            .put_credential(a.id, CredentialKind::ApiKey, br#"{"key":"PLAINTEXT-KEY"}"#, None)
            .await
            .expect("put");

        let raw: Vec<u8> = sqlx::query_scalar("SELECT sealed FROM credentials")
            .fetch_one(store.pool())
            .await
            .expect("raw");
        assert!(!String::from_utf8_lossy(&raw).contains("PLAINTEXT-KEY"));

        let opened = store.open_credential(a.id).await.expect("open").expect("present");
        assert_eq!(opened.kind, CredentialKind::ApiKey);
        assert_eq!(opened.json.as_slice(), br#"{"key":"PLAINTEXT-KEY"}"#);
        assert!(!format!("{opened:?}").contains("PLAINTEXT"));

        store
            .put_credential(a.id, CredentialKind::ApiKey, br#"{"key":"NEW"}"#, None)
            .await
            .expect("replace");
        let meta = store.credential_meta(a.id).await.expect("meta").expect("present");
        assert!(meta.refreshed_at.is_some());
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM credentials")
            .fetch_one(store.pool())
            .await
            .expect("n");
        assert_eq!(count, 1);

        store.delete_credential(a.id).await.expect("delete");
        assert!(store.open_credential(a.id).await.expect("open").is_none());
    }
}
