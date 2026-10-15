//! Ownerships and the snapshot diff of contracts/connector.md section 5 (FR-006, FR-018, FR-025).

use crate::{Result, Store, ids::TextId, repo::now};
use gameshelf_core::{AccountId, Acquisition, OwnershipId, ProductId};
use sqlx::Row;
use std::collections::HashMap;
use time::OffsetDateTime;

/// Where an ownership was learned from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OwnershipSource {
    Connector,
    Agent,
    Manual,
    Import,
}

impl OwnershipSource {
    pub fn as_str(self) -> &'static str {
        match self {
            OwnershipSource::Connector => "connector",
            OwnershipSource::Agent => "agent",
            OwnershipSource::Manual => "manual",
            OwnershipSource::Import => "import",
        }
    }
}

/// One product present in an account's listing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotItem {
    pub product_id: ProductId,
    pub acquisition: Acquisition,
    /// `None` means the platform does not provide it; existing values are then kept.
    pub playtime_minutes: Option<u32>,
    pub last_played_at: Option<OffsetDateTime>,
    pub achievements: Option<(u32, u32)>,
}

/// What a snapshot changed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ApplyOutcome {
    /// Products new to this account.
    pub added: Vec<ProductId>,
    /// Products that were unavailable and came back.
    pub reappeared: Vec<ProductId>,
    /// Products absent from a complete listing, now marked unavailable.
    pub unavailable: Vec<ProductId>,
    /// Products whose playtime, last played or achievements changed.
    pub updated: usize,
}

struct Existing {
    id: OwnershipId,
    acquisition: String,
    unavailable: bool,
    playtime: Option<i64>,
    last_played: Option<OffsetDateTime>,
    achievements: (Option<i64>, Option<i64>),
}

impl Store {
    /// Applies an account's listing in one transaction.
    ///
    /// Products in `items` are created or refreshed. When `complete` is true, ownerships missing from `items`
    /// are marked unavailable; an incomplete listing never removes anything.
    pub async fn apply_snapshot(
        &self,
        account: AccountId,
        items: &[SnapshotItem],
        complete: bool,
        source: OwnershipSource,
    ) -> Result<ApplyOutcome> {
        let now = now();
        let today = now.date().to_string();
        let mut tx = self.pool().begin().await?;

        let rows = sqlx::query(
            "SELECT id, product_id, acquisition, unavailable_since, playtime_minutes, last_played_at,
                achievements_unlocked, achievements_total
             FROM ownerships WHERE account_id = ?",
        )
        .bind(account.txt())
        .fetch_all(&mut *tx)
        .await?;
        let mut existing: HashMap<ProductId, Existing> = HashMap::with_capacity(rows.len());
        for r in &rows {
            let unavailable: Option<OffsetDateTime> = r.try_get("unavailable_since")?;
            existing.insert(
                ProductId::from_txt(&r.try_get::<String, _>("product_id")?)?,
                Existing {
                    id: OwnershipId::from_txt(&r.try_get::<String, _>("id")?)?,
                    acquisition: r.try_get("acquisition")?,
                    unavailable: unavailable.is_some(),
                    playtime: r.try_get("playtime_minutes")?,
                    last_played: r.try_get("last_played_at")?,
                    achievements: (
                        r.try_get("achievements_unlocked")?,
                        r.try_get("achievements_total")?,
                    ),
                },
            );
        }

        let mut out = ApplyOutcome::default();
        let mut seen = std::collections::HashSet::with_capacity(items.len());
        for item in items {
            if !seen.insert(item.product_id) {
                continue;
            }
            let playtime = item.playtime_minutes.map(i64::from);
            let (ach_u, ach_t) = match item.achievements {
                Some((u, t)) => (Some(i64::from(u)), Some(i64::from(t))),
                None => (None, None),
            };
            match existing.get(&item.product_id) {
                // An agent only sees one machine and may lag behind the platform: it never changes how a game
                // was acquired, never brings back a game the platform no longer lists, and never lowers
                // playtime or last played.
                Some(old) if source == OwnershipSource::Agent => {
                    let playtime = playtime.max(old.playtime);
                    let last_played = item.last_played_at.max(old.last_played);
                    if playtime != old.playtime || last_played != old.last_played {
                        out.updated += 1;
                    }
                    sqlx::query(
                        "UPDATE ownerships SET last_seen_at = ?, playtime_minutes = ?, last_played_at = ? WHERE id = ?",
                    )
                    .bind(now)
                    .bind(playtime)
                    .bind(last_played)
                    .bind(old.id.txt())
                    .execute(&mut *tx)
                    .await?;
                    if let Some(minutes) = playtime
                        && Some(minutes) != old.playtime
                    {
                        snapshot_playtime(&mut tx, old.id, &today, minutes).await?;
                    }
                }
                Some(old) => {
                    if old.unavailable {
                        out.reappeared.push(item.product_id);
                    }
                    let changed = (playtime.is_some() && playtime != old.playtime)
                        || (item.last_played_at.is_some() && item.last_played_at != old.last_played)
                        || (ach_u.is_some() && (ach_u, ach_t) != old.achievements)
                        || old.acquisition != item.acquisition.as_str();
                    if changed {
                        out.updated += 1;
                    }
                    sqlx::query(
                        "UPDATE ownerships SET last_seen_at = ?, unavailable_since = NULL, acquisition = ?,
                            playtime_minutes = coalesce(?, playtime_minutes),
                            last_played_at = coalesce(?, last_played_at),
                            achievements_unlocked = coalesce(?, achievements_unlocked),
                            achievements_total = coalesce(?, achievements_total)
                         WHERE id = ?",
                    )
                    .bind(now)
                    .bind(item.acquisition.as_str())
                    .bind(playtime)
                    .bind(item.last_played_at)
                    .bind(ach_u)
                    .bind(ach_t)
                    .bind(old.id.txt())
                    .execute(&mut *tx)
                    .await?;
                    if let Some(minutes) = playtime
                        && Some(minutes) != old.playtime
                    {
                        snapshot_playtime(&mut tx, old.id, &today, minutes).await?;
                    }
                }
                None => {
                    let id = OwnershipId::new();
                    sqlx::query(
                        "INSERT INTO ownerships (id, account_id, product_id, acquisition, first_seen_at, last_seen_at,
                            playtime_minutes, last_played_at, achievements_unlocked, achievements_total, source)
                         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                    )
                    .bind(id.txt())
                    .bind(account.txt())
                    .bind(item.product_id.txt())
                    .bind(item.acquisition.as_str())
                    .bind(now)
                    .bind(now)
                    .bind(playtime)
                    .bind(item.last_played_at)
                    .bind(ach_u)
                    .bind(ach_t)
                    .bind(source.as_str())
                    .execute(&mut *tx)
                    .await?;
                    if let Some(minutes) = playtime {
                        snapshot_playtime(&mut tx, id, &today, minutes).await?;
                    }
                    out.added.push(item.product_id);
                }
            }
        }

        if complete {
            for (product, old) in &existing {
                if !seen.contains(product) && !old.unavailable {
                    sqlx::query("UPDATE ownerships SET unavailable_since = ? WHERE id = ?")
                        .bind(now)
                        .bind(old.id.txt())
                        .execute(&mut *tx)
                        .await?;
                    out.unavailable.push(*product);
                }
            }
        }

        tx.commit().await?;
        Ok(out)
    }

    /// Number of ownerships of an account, available ones only.
    pub async fn count_available_ownerships(&self, account: AccountId) -> Result<i64> {
        Ok(sqlx::query_scalar(
            "SELECT count(*) FROM ownerships WHERE account_id = ? AND unavailable_since IS NULL",
        )
        .bind(account.txt())
        .fetch_one(self.pool())
        .await?)
    }
}

async fn snapshot_playtime(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    ownership: OwnershipId,
    day: &str,
    minutes: i64,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO playtime_snapshots (ownership_id, day, playtime_minutes) VALUES (?, ?, ?)
         ON CONFLICT (ownership_id, day) DO UPDATE SET playtime_minutes = excluded.playtime_minutes",
    )
    .bind(ownership.txt())
    .bind(day)
    .bind(minutes)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{NewAccount, NewProduct, db::test_support::store, repo::accounts::tests::profile};
    use gameshelf_core::{ProductKind, account_status::AccountStatus};

    async fn setup() -> (Store, AccountId, Vec<ProductId>) {
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
        let mut products = Vec::new();
        for (id, title) in [("10", "Alpha"), ("20", "Beta"), ("30", "Gamma")] {
            let prod = store
                .upsert_product(&NewProduct {
                    platform: "steam",
                    platform_game_id: id,
                    title,
                    kind: ProductKind::Game,
                    parent_platform_game_id: None,
                    edition: None,
                    artwork: None,
                    raw: None,
                })
                .await
                .expect("product");
            products.push(prod.id);
        }
        (store, a.id, products)
    }

    fn item(product: ProductId, minutes: Option<u32>) -> SnapshotItem {
        SnapshotItem {
            product_id: product,
            acquisition: Acquisition::Purchase,
            playtime_minutes: minutes,
            last_played_at: None,
            achievements: None,
        }
    }

    #[tokio::test]
    async fn first_snapshot_adds_everything() {
        let (store, account, p) = setup().await;
        let out = store
            .apply_snapshot(
                account,
                &[item(p[0], Some(60)), item(p[1], None)],
                true,
                OwnershipSource::Connector,
            )
            .await
            .expect("apply");
        assert_eq!(out.added, vec![p[0], p[1]]);
        assert!(out.unavailable.is_empty());
        assert_eq!(store.count_available_ownerships(account).await.expect("count"), 2);
    }

    #[tokio::test]
    async fn complete_listing_marks_missing_unavailable_and_they_can_come_back() {
        let (store, account, p) = setup().await;
        store
            .apply_snapshot(
                account,
                &[item(p[0], None), item(p[1], None)],
                true,
                OwnershipSource::Connector,
            )
            .await
            .expect("1");
        let out = store
            .apply_snapshot(account, &[item(p[0], None)], true, OwnershipSource::Connector)
            .await
            .expect("2");
        assert_eq!(out.unavailable, vec![p[1]]);
        assert_eq!(store.count_available_ownerships(account).await.expect("count"), 1);
        let back = store
            .apply_snapshot(
                account,
                &[item(p[0], None), item(p[1], None)],
                true,
                OwnershipSource::Connector,
            )
            .await
            .expect("3");
        assert_eq!(back.reappeared, vec![p[1]]);
        assert!(back.added.is_empty());
        assert_eq!(store.count_available_ownerships(account).await.expect("count"), 2);
    }

    #[tokio::test]
    async fn incomplete_listing_never_removes() {
        let (store, account, p) = setup().await;
        store
            .apply_snapshot(
                account,
                &[item(p[0], None), item(p[1], None)],
                true,
                OwnershipSource::Connector,
            )
            .await
            .expect("1");
        let out = store
            .apply_snapshot(account, &[item(p[0], None)], false, OwnershipSource::Connector)
            .await
            .expect("2");
        assert!(out.unavailable.is_empty());
        assert_eq!(store.count_available_ownerships(account).await.expect("count"), 2);
    }

    #[tokio::test]
    async fn playtime_changes_are_counted_and_snapshotted_and_missing_values_are_kept() {
        let (store, account, p) = setup().await;
        store
            .apply_snapshot(account, &[item(p[0], Some(60))], true, OwnershipSource::Connector)
            .await
            .expect("1");
        let out = store
            .apply_snapshot(account, &[item(p[0], Some(90))], true, OwnershipSource::Connector)
            .await
            .expect("2");
        assert_eq!(out.updated, 1);
        let same = store
            .apply_snapshot(account, &[item(p[0], None)], true, OwnershipSource::Connector)
            .await
            .expect("3");
        assert_eq!(same.updated, 0);
        let minutes: i64 = sqlx::query_scalar("SELECT playtime_minutes FROM ownerships")
            .fetch_one(store.pool())
            .await
            .expect("q");
        assert_eq!(minutes, 90);
        let snaps: i64 = sqlx::query_scalar("SELECT playtime_minutes FROM playtime_snapshots")
            .fetch_one(store.pool())
            .await
            .expect("q");
        assert_eq!(snaps, 90);
    }

    #[tokio::test]
    async fn duplicates_in_a_listing_are_ignored() {
        let (store, account, p) = setup().await;
        let out = store
            .apply_snapshot(
                account,
                &[item(p[2], None), item(p[2], None)],
                true,
                OwnershipSource::Connector,
            )
            .await
            .expect("apply");
        assert_eq!(out.added, vec![p[2]]);
    }
}
