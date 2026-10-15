//! Achievements progress and per-account enrichment values (FR-016, FR-017).

use crate::{
    Result, Store,
    ids::{TextId, opt_id},
    repo::now,
};
use gameshelf_core::{AccountId, GameId};
use sqlx::Row;
use time::OffsetDateTime;

/// One enrichment value as shown in a game's detail.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EnrichmentValue {
    pub account_id: AccountId,
    pub game_id: Option<GameId>,
    pub provider: String,
    pub key: String,
    pub value: serde_json::Value,
    pub unit: Option<String>,
    pub refreshed_at: OffsetDateTime,
}

impl Store {
    /// Records achievements progress for one product of an account.
    pub async fn set_achievements(
        &self,
        account: AccountId,
        platform: &str,
        platform_game_id: &str,
        unlocked: u32,
        total: u32,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE ownerships SET achievements_unlocked = ?, achievements_total = ?
             WHERE account_id = ? AND product_id = (SELECT id FROM platform_products WHERE platform = ? AND platform_game_id = ?)",
        )
        .bind(i64::from(unlocked))
        .bind(i64::from(total))
        .bind(account.txt())
        .bind(platform)
        .bind(platform_game_id)
        .execute(self.pool())
        .await?;
        Ok(())
    }

    /// Stores an enrichment value, replacing the previous one with the same key.
    #[allow(clippy::too_many_arguments)]
    pub async fn put_enrichment(
        &self,
        account: AccountId,
        platform: &str,
        platform_game_id: Option<&str>,
        provider: &str,
        key: &str,
        value: &serde_json::Value,
        unit: Option<&str>,
    ) -> Result<()> {
        let game: Option<String> = match platform_game_id {
            Some(pgid) => sqlx::query_scalar(
                "SELECT game_id FROM platform_products WHERE platform = ? AND platform_game_id = ?",
            )
            .bind(platform)
            .bind(pgid)
            .fetch_optional(self.pool())
            .await?
            .flatten(),
            None => None,
        };
        let mut tx = self.pool().begin().await?;
        sqlx::query(
            "DELETE FROM enrichment_values WHERE account_id = ? AND coalesce(game_id, '') = coalesce(?, '')
               AND provider = ? AND key = ?",
        )
        .bind(account.txt())
        .bind(&game)
        .bind(provider)
        .bind(key)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO enrichment_values (id, account_id, game_id, provider, key, value, unit, refreshed_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(uuid::Uuid::now_v7().hyphenated().to_string())
        .bind(account.txt())
        .bind(&game)
        .bind(provider)
        .bind(key)
        .bind(value.to_string())
        .bind(unit)
        .bind(now())
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Enrichment values of the given accounts, for one game plus platform-level values.
    pub async fn enrichment_for(&self, accounts: &[AccountId], game: GameId) -> Result<Vec<EnrichmentValue>> {
        let mut out = Vec::new();
        for account in accounts {
            let rows = sqlx::query(
                "SELECT account_id, game_id, provider, key, value, unit, refreshed_at FROM enrichment_values
                 WHERE account_id = ? AND (game_id = ? OR game_id IS NULL) ORDER BY game_id IS NULL, key",
            )
            .bind(account.txt())
            .bind(game.txt())
            .fetch_all(self.pool())
            .await?;
            for r in &rows {
                let value: String = r.try_get("value")?;
                out.push(EnrichmentValue {
                    account_id: AccountId::from_txt(&r.try_get::<String, _>("account_id")?)?,
                    game_id: opt_id(r.try_get("game_id")?)?,
                    provider: r.try_get("provider")?,
                    key: r.try_get("key")?,
                    value: serde_json::from_str(&value).unwrap_or(serde_json::Value::Null),
                    unit: r.try_get("unit")?,
                    refreshed_at: r.try_get("refreshed_at")?,
                });
            }
        }
        Ok(out)
    }
}
