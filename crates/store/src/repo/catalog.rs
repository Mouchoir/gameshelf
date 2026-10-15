//! Games, platform products, external ids and artwork assets (FR-007 to FR-010).

use crate::{
    Result, Store, StoreError,
    ids::{TextId, opt_id},
    repo::now,
};
use gameshelf_core::{GameId, ProductId, ProductKind, normalize};
use sqlx::Row;

/// A product as listed by one platform.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Product {
    pub id: ProductId,
    pub platform: String,
    pub platform_game_id: String,
    pub title: String,
    pub kind: ProductKind,
    pub parent_platform_game_id: Option<String>,
    pub game_id: Option<GameId>,
    pub edition: Option<String>,
}

/// Input for [`Store::upsert_product`].
#[derive(Debug)]
pub struct NewProduct<'a> {
    pub platform: &'a str,
    pub platform_game_id: &'a str,
    pub title: &'a str,
    pub kind: ProductKind,
    pub parent_platform_game_id: Option<&'a str>,
    pub edition: Option<&'a str>,
    pub artwork: Option<&'a serde_json::Value>,
    pub raw: Option<&'a serde_json::Value>,
}

/// Kinds a canonical game can have (bundles and unknown products are grouped as games).
fn game_kind(kind: ProductKind) -> &'static str {
    match kind {
        ProductKind::Dlc => "dlc",
        ProductKind::Tool => "tool",
        _ => "game",
    }
}

fn product_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<Product> {
    let kind: String = row.try_get("kind")?;
    Ok(Product {
        id: ProductId::from_txt(&row.try_get::<String, _>("id")?)?,
        platform: row.try_get("platform")?,
        platform_game_id: row.try_get("platform_game_id")?,
        title: row.try_get("title")?,
        kind: ProductKind::parse(&kind).unwrap_or(ProductKind::Unknown),
        parent_platform_game_id: row.try_get("parent_platform_game_id")?,
        game_id: opt_id(row.try_get("game_id")?)?,
        edition: row.try_get("edition")?,
    })
}

const PRODUCT_COLUMNS: &str =
    "id, platform, platform_game_id, title, kind, parent_platform_game_id, game_id, edition";

impl Store {
    /// Inserts or updates a product keyed by `(platform, platform_game_id)` and returns it.
    pub async fn upsert_product(&self, new: &NewProduct<'_>) -> Result<Product> {
        let title = new.title.trim();
        if title.is_empty() || new.platform_game_id.trim().is_empty() {
            return Err(StoreError::invalid("a product needs a title and a platform id"));
        }
        sqlx::query(
            "INSERT INTO platform_products (id, platform, platform_game_id, title, kind, parent_platform_game_id,
                edition, platform_artwork, raw, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT (platform, platform_game_id) DO UPDATE SET title = excluded.title, kind = excluded.kind,
                parent_platform_game_id = coalesce(excluded.parent_platform_game_id, platform_products.parent_platform_game_id),
                edition = coalesce(excluded.edition, platform_products.edition),
                platform_artwork = coalesce(excluded.platform_artwork, platform_products.platform_artwork),
                raw = coalesce(excluded.raw, platform_products.raw), updated_at = excluded.updated_at",
        )
        .bind(ProductId::new().txt())
        .bind(new.platform)
        .bind(new.platform_game_id)
        .bind(title)
        .bind(new.kind.as_str())
        .bind(new.parent_platform_game_id)
        .bind(new.edition)
        .bind(new.artwork.map(|v| v.to_string()))
        .bind(new.raw.map(|v| v.to_string()))
        .bind(now())
        .execute(self.pool())
        .await?;
        self.find_product(new.platform, new.platform_game_id)
            .await?
            .ok_or(StoreError::NotFound)
    }

    pub async fn find_product(&self, platform: &str, platform_game_id: &str) -> Result<Option<Product>> {
        let row = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT {PRODUCT_COLUMNS} FROM platform_products WHERE platform = ? AND platform_game_id = ?"
        )))
        .bind(platform)
        .bind(platform_game_id)
        .fetch_optional(self.pool())
        .await?;
        row.as_ref().map(product_from_row).transpose()
    }

    pub async fn get_product(&self, id: ProductId) -> Result<Option<Product>> {
        let row = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT {PRODUCT_COLUMNS} FROM platform_products WHERE id = ?"
        )))
        .bind(id.txt())
        .fetch_optional(self.pool())
        .await?;
        row.as_ref().map(product_from_row).transpose()
    }

    /// Products not yet attached to a game.
    pub async fn unmatched_products(&self, limit: i64) -> Result<Vec<Product>> {
        let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT {PRODUCT_COLUMNS} FROM platform_products WHERE game_id IS NULL ORDER BY updated_at LIMIT ?"
        )))
        .bind(limit)
        .fetch_all(self.pool())
        .await?;
        rows.iter().map(product_from_row).collect()
    }

    pub async fn products_of_game(&self, game: GameId) -> Result<Vec<Product>> {
        let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT {PRODUCT_COLUMNS} FROM platform_products WHERE game_id = ? ORDER BY platform, title"
        )))
        .bind(game.txt())
        .fetch_all(self.pool())
        .await?;
        rows.iter().map(product_from_row).collect()
    }

    /// Creates a canonical game with its normalized title keys.
    pub async fn create_game(
        &self,
        title: &str,
        kind: ProductKind,
        parent: Option<GameId>,
        release_year: Option<i32>,
        metadata_source: &str,
    ) -> Result<GameId> {
        let id = GameId::new();
        let now = now();
        sqlx::query(
            "INSERT INTO games (id, title, sort_title, search_key, release_year, kind, parent_game_id, metadata_source,
                created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id.txt())
        .bind(title.trim())
        .bind(normalize::sort_title(title))
        .bind(normalize::search_key(title))
        .bind(release_year)
        .bind(game_kind(kind))
        .bind(parent.map(|p| p.txt()))
        .bind(metadata_source)
        .bind(now)
        .bind(now)
        .execute(self.pool())
        .await?;
        Ok(id)
    }

    /// Updates a game's display title and its derived keys.
    pub async fn set_game_title(&self, game: GameId, title: &str) -> Result<()> {
        sqlx::query(
            "UPDATE games SET title = ?, sort_title = ?, search_key = ?, updated_at = ? WHERE id = ?",
        )
        .bind(title.trim())
        .bind(normalize::sort_title(title))
        .bind(normalize::search_key(title))
        .bind(now())
        .bind(game.txt())
        .execute(self.pool())
        .await?;
        Ok(())
    }

    pub async fn set_game_parent(&self, game: GameId, parent: Option<GameId>) -> Result<()> {
        sqlx::query(
            "UPDATE games SET parent_game_id = ?, kind = CASE WHEN ? IS NULL THEN kind ELSE 'dlc' END,
                updated_at = ? WHERE id = ?",
        )
        .bind(parent.map(|p| p.txt()))
        .bind(parent.map(|p| p.txt()))
        .bind(now())
        .bind(game.txt())
        .execute(self.pool())
        .await?;
        Ok(())
    }

    /// Attaches a product to a game.
    pub async fn set_product_game(&self, product: ProductId, game: Option<GameId>) -> Result<()> {
        sqlx::query("UPDATE platform_products SET game_id = ? WHERE id = ?")
            .bind(game.map(|g| g.txt()))
            .bind(product.txt())
            .execute(self.pool())
            .await?;
        Ok(())
    }

    /// Records an identifier of a game in another database; an id already bound to a game is left untouched.
    pub async fn add_game_external_id(&self, game: GameId, source: &str, external_id: &str) -> Result<()> {
        sqlx::query(
            "INSERT OR IGNORE INTO game_external_ids (game_id, source, external_id) VALUES (?, ?, ?)",
        )
        .bind(game.txt())
        .bind(source)
        .bind(external_id)
        .execute(self.pool())
        .await?;
        Ok(())
    }

    pub async fn find_game_by_external(&self, source: &str, external_id: &str) -> Result<Option<GameId>> {
        let id: Option<String> =
            sqlx::query_scalar("SELECT game_id FROM game_external_ids WHERE source = ? AND external_id = ?")
                .bind(source)
                .bind(external_id)
                .fetch_optional(self.pool())
                .await?;
        opt_id(id)
    }

    /// Finds games with exactly this normalized title (fallback matching).
    pub async fn find_games_by_search_key(&self, key: &str) -> Result<Vec<(GameId, Option<i32>)>> {
        let rows = sqlx::query("SELECT id, release_year FROM games WHERE search_key = ? AND kind = 'game'")
            .bind(key)
            .fetch_all(self.pool())
            .await?;
        rows.iter()
            .map(|r| {
                Ok((
                    GameId::from_txt(&r.try_get::<String, _>("id")?)?,
                    r.try_get("release_year")?,
                ))
            })
            .collect()
    }

    /// Deletes games that no product refers to anymore and that are not a DLC parent.
    pub async fn delete_orphan_games(&self) -> Result<u64> {
        let done = sqlx::query(
            "DELETE FROM games WHERE id NOT IN (SELECT game_id FROM platform_products WHERE game_id IS NOT NULL)
               AND id NOT IN (SELECT parent_game_id FROM games WHERE parent_game_id IS NOT NULL)",
        )
        .execute(self.pool())
        .await?;
        Ok(done.rows_affected())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_support::store;

    fn product<'a>(id: &'a str, title: &'a str) -> NewProduct<'a> {
        NewProduct {
            platform: "steam",
            platform_game_id: id,
            title,
            kind: ProductKind::Game,
            parent_platform_game_id: None,
            edition: None,
            artwork: None,
            raw: None,
        }
    }

    #[tokio::test]
    async fn upsert_is_idempotent_and_updates_title() {
        let store = store().await;
        let a = store
            .upsert_product(&product("730", "Counter-Strike: Global Offensive"))
            .await
            .expect("insert");
        let b = store
            .upsert_product(&product("730", "Counter-Strike 2"))
            .await
            .expect("update");
        assert_eq!(a.id, b.id);
        assert_eq!(b.title, "Counter-Strike 2");
    }

    #[tokio::test]
    async fn games_get_normalized_keys_and_external_ids() {
        let store = store().await;
        let g = store
            .create_game(
                "The Witcher 3: Wild Hunt",
                ProductKind::Game,
                None,
                Some(2015),
                "platform",
            )
            .await
            .expect("game");
        let key: String = sqlx::query_scalar("SELECT search_key FROM games")
            .fetch_one(store.pool())
            .await
            .expect("key");
        assert_eq!(key, "witcher 3 wild hunt");
        store.add_game_external_id(g, "igdb", "1942").await.expect("ext");
        store
            .add_game_external_id(g, "igdb", "1942")
            .await
            .expect("ext twice");
        assert_eq!(
            store.find_game_by_external("igdb", "1942").await.expect("find"),
            Some(g)
        );
        assert_eq!(
            store
                .find_games_by_search_key("witcher 3 wild hunt")
                .await
                .expect("find")
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn orphan_games_are_removed() {
        let store = store().await;
        let p = store.upsert_product(&product("1", "Kept")).await.expect("p");
        let kept = store
            .create_game("Kept", ProductKind::Game, None, None, "platform")
            .await
            .expect("g");
        store.set_product_game(p.id, Some(kept)).await.expect("attach");
        store
            .create_game("Orphan", ProductKind::Game, None, None, "platform")
            .await
            .expect("g");
        assert_eq!(store.delete_orphan_games().await.expect("delete"), 1);
        assert_eq!(store.products_of_game(kept).await.expect("products").len(), 1);
    }
}
