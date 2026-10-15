//! Sharing rules and exclusions (FR-032, FR-033). Sharing is off by default.

use crate::{Result, Store, StoreError, ids::TextId, repo::now};
use gameshelf_core::ProfileId;
use sqlx::Row;

/// What an exclusion hides.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExclusionKind {
    Game,
    Platform,
    Account,
}

impl ExclusionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ExclusionKind::Game => "game",
            ExclusionKind::Platform => "platform",
            ExclusionKind::Account => "account",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "game" => Some(ExclusionKind::Game),
            "platform" => Some(ExclusionKind::Platform),
            "account" => Some(ExclusionKind::Account),
            _ => None,
        }
    }
}

/// An owner's exclusion; `audience` `None` applies to everyone the owner shares with.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Exclusion {
    pub id: String,
    pub audience: Option<ProfileId>,
    pub kind: ExclusionKind,
    pub reference: String,
}

impl Store {
    /// Shares (or stops sharing) `owner`'s library with `audience`.
    pub async fn set_sharing(&self, owner: ProfileId, audience: ProfileId, shared: bool) -> Result<()> {
        if owner == audience {
            return Err(StoreError::invalid("a profile cannot share with itself"));
        }
        if shared {
            sqlx::query(
                "INSERT OR IGNORE INTO sharing_rules (owner_profile_id, audience_profile_id, created_at) VALUES (?, ?, ?)",
            )
            .bind(owner.txt())
            .bind(audience.txt())
            .bind(now())
            .execute(self.pool())
            .await?;
        } else {
            sqlx::query("DELETE FROM sharing_rules WHERE owner_profile_id = ? AND audience_profile_id = ?")
                .bind(owner.txt())
                .bind(audience.txt())
                .execute(self.pool())
                .await?;
        }
        Ok(())
    }

    /// Profiles `owner` shares with.
    pub async fn shared_with(&self, owner: ProfileId) -> Result<Vec<ProfileId>> {
        let rows: Vec<String> =
            sqlx::query_scalar("SELECT audience_profile_id FROM sharing_rules WHERE owner_profile_id = ?")
                .bind(owner.txt())
                .fetch_all(self.pool())
                .await?;
        rows.iter().map(|s| ProfileId::from_txt(s)).collect()
    }

    /// Profiles sharing with `viewer` (whose games `viewer` may see).
    pub async fn sharing_with_me(&self, viewer: ProfileId) -> Result<Vec<ProfileId>> {
        let rows: Vec<String> =
            sqlx::query_scalar("SELECT owner_profile_id FROM sharing_rules WHERE audience_profile_id = ?")
                .bind(viewer.txt())
                .fetch_all(self.pool())
                .await?;
        rows.iter().map(|s| ProfileId::from_txt(s)).collect()
    }

    pub async fn add_exclusion(
        &self,
        owner: ProfileId,
        audience: Option<ProfileId>,
        kind: ExclusionKind,
        reference: &str,
    ) -> Result<()> {
        let exists: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM sharing_exclusions WHERE owner_profile_id = ? AND coalesce(audience_profile_id, '') = coalesce(?, '')
               AND kind = ? AND ref = ?",
        )
        .bind(owner.txt())
        .bind(audience.map(|a| a.txt()))
        .bind(kind.as_str())
        .bind(reference)
        .fetch_one(self.pool())
        .await?;
        if exists > 0 {
            return Ok(());
        }
        sqlx::query(
            "INSERT INTO sharing_exclusions (id, owner_profile_id, audience_profile_id, kind, ref) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(uuid::Uuid::now_v7().hyphenated().to_string())
        .bind(owner.txt())
        .bind(audience.map(|a| a.txt()))
        .bind(kind.as_str())
        .bind(reference)
        .execute(self.pool())
        .await?;
        Ok(())
    }

    pub async fn remove_exclusion(
        &self,
        owner: ProfileId,
        kind: ExclusionKind,
        reference: &str,
        audience: Option<ProfileId>,
    ) -> Result<()> {
        sqlx::query(
            "DELETE FROM sharing_exclusions WHERE owner_profile_id = ? AND kind = ? AND ref = ?
               AND coalesce(audience_profile_id, '') = coalesce(?, '')",
        )
        .bind(owner.txt())
        .bind(kind.as_str())
        .bind(reference)
        .bind(audience.map(|a| a.txt()))
        .execute(self.pool())
        .await?;
        Ok(())
    }

    pub async fn list_exclusions(&self, owner: ProfileId) -> Result<Vec<Exclusion>> {
        let rows = sqlx::query(
            "SELECT id, audience_profile_id, kind, ref FROM sharing_exclusions WHERE owner_profile_id = ? ORDER BY kind, ref",
        )
        .bind(owner.txt())
        .fetch_all(self.pool())
        .await?;
        rows.iter()
            .filter_map(|r| {
                let kind: String = r.try_get("kind").ok()?;
                Some((r, ExclusionKind::parse(&kind)?))
            })
            .map(|(r, kind)| {
                let audience: Option<String> = r.try_get("audience_profile_id")?;
                Ok(Exclusion {
                    id: r.try_get("id")?,
                    audience: audience.as_deref().map(ProfileId::from_txt).transpose()?,
                    kind,
                    reference: r.try_get("ref")?,
                })
            })
            .collect()
    }
}
