//! Turns a connector's listing into products, games and ownerships (contracts/connector.md section 5).

use gameshelf_connectors::{LibraryItem, LibrarySnapshot};
use gameshelf_core::{GameId, ProductKind, matching};
use gameshelf_metadata::{ArtworkCache, Igdb};
use gameshelf_store::{Account, ApplyOutcome, NewProduct, OwnershipSource, SnapshotItem, Store, StoreError};
use std::collections::{HashMap, HashSet};

#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// What an import changed, at the game level.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImportReport {
    pub added_games: Vec<GameId>,
    pub unavailable_games: Vec<GameId>,
    pub outcome: ApplyOutcome,
}

/// Shared import machinery. Metadata providers are optional.
#[derive(Clone, Debug)]
pub struct Importer {
    store: Store,
    igdb: Option<Igdb>,
    artwork: Option<ArtworkCache>,
}

impl Importer {
    pub fn new(store: Store, igdb: Option<Igdb>, artwork: Option<ArtworkCache>) -> Self {
        Self { store, igdb, artwork }
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    /// Imports one account's listing.
    pub async fn import(
        &self,
        account: &Account,
        snapshot: &LibrarySnapshot,
        source: OwnershipSource,
    ) -> Result<ImportReport, SyncError> {
        let mut items = Vec::with_capacity(snapshot.items.len());
        let mut product_games: HashMap<gameshelf_core::ProductId, GameId> = HashMap::new();
        // DLC are attached to their parents once every product of the listing exists.
        let mut dlc: Vec<(GameId, String)> = Vec::new();

        for item in &snapshot.items {
            let (product, game) = self.upsert_and_match(&account.platform, item).await?;
            if item.kind == ProductKind::Dlc
                && let Some(parent) = &item.parent_platform_game_id
            {
                dlc.push((game, parent.clone()));
            }
            product_games.insert(product, game);
            items.push(SnapshotItem {
                product_id: product,
                acquisition: item.acquisition,
                playtime_minutes: item.playtime_minutes,
                last_played_at: item.last_played_at,
                achievements: None,
            });
        }

        for (game, parent_pgid) in dlc {
            if let Some(parent) = self.store.find_product(&account.platform, &parent_pgid).await?
                && let Some(parent_game) = parent.game_id
                && parent_game != game
            {
                self.store.set_game_parent(game, Some(parent_game)).await?;
            }
        }

        let outcome = self
            .store
            .apply_snapshot(account.id, &items, snapshot.complete, source)
            .await?;
        let mut report = ImportReport::default();
        let mut seen = HashSet::new();
        for p in outcome.added.iter().chain(&outcome.reappeared) {
            if let Some(g) = product_games.get(p)
                && seen.insert(*g)
            {
                report.added_games.push(*g);
            }
        }
        for p in &outcome.unavailable {
            if let Some(product) = self.store.get_product(*p).await?
                && let Some(g) = product.game_id
            {
                report.unavailable_games.push(g);
            }
        }
        report.outcome = outcome;
        Ok(report)
    }

    /// Creates or refreshes the product and makes sure it belongs to a game.
    async fn upsert_and_match(
        &self,
        platform: &str,
        item: &LibraryItem,
    ) -> Result<(gameshelf_core::ProductId, GameId), SyncError> {
        let (base_title, edition) = matching::split_edition(&item.title);
        let edition = item.edition.clone().or(edition);
        let artwork =
            (!item.artwork.is_empty()).then(|| serde_json::to_value(&item.artwork).unwrap_or_default());
        let raw = (!item.raw.is_null()).then_some(&item.raw);
        let product = self
            .store
            .upsert_product(&NewProduct {
                platform,
                platform_game_id: &item.platform_game_id,
                title: &item.title,
                kind: item.kind,
                parent_platform_game_id: item.parent_platform_game_id.as_deref(),
                edition: edition.as_deref(),
                artwork: artwork.as_ref(),
                raw,
            })
            .await?;
        if let Some(game) = product.game_id {
            return Ok((product.id, game));
        }
        if let Some(game) = self.store.match_decision_for(product.id).await? {
            self.store.set_product_game(product.id, Some(game)).await?;
            return Ok((product.id, game));
        }

        let mut by_id = Vec::new();
        for (source, id) in &item.cross_ids {
            if let Some(g) = self.store.find_game_by_external(source.as_str(), id).await? {
                by_id.push(g);
            }
        }
        let by_title: Vec<matching::TitleCandidate> = if item.kind == ProductKind::Game {
            self.store
                .find_games_by_search_key(&matching::match_key(&item.title))
                .await?
                .into_iter()
                .map(|(game, release_year)| matching::TitleCandidate { game, release_year })
                .collect()
        } else {
            Vec::new()
        };
        let game = match matching::choose(&by_id, &by_title, None) {
            Some(g) => g,
            None => {
                self.store
                    .create_game(&base_title, item.kind, None, None, "platform")
                    .await?
            }
        };
        self.store
            .add_game_external_id(game, platform, &item.platform_game_id)
            .await?;
        for (source, id) in &item.cross_ids {
            self.store.add_game_external_id(game, source.as_str(), id).await?;
        }
        self.store.set_product_game(product.id, Some(game)).await?;
        Ok((product.id, game))
    }

    /// Resolves games without an IGDB id: records the id, merges games that turn out to be the same, and takes
    /// the canonical title, year and cover from IGDB. Returns the number of games resolved.
    pub async fn resolve_metadata(&self, limit: i64) -> Result<usize, SyncError> {
        let Some(igdb) = &self.igdb else { return Ok(0) };
        let pending = self.store.games_missing_igdb(limit).await?;
        if pending.is_empty() {
            return Ok(0);
        }
        let mut by_platform: HashMap<String, Vec<(GameId, String)>> = HashMap::new();
        for (game, platform, pgid) in pending.iter().cloned() {
            by_platform.entry(platform).or_default().push((game, pgid));
        }
        let mut resolved: HashMap<GameId, u64> = HashMap::new();
        for (platform, items) in &by_platform {
            let uids: Vec<String> = items.iter().map(|(_, id)| id.clone()).collect();
            let map = match igdb.games_by_external(platform, &uids).await {
                Ok(m) => m,
                Err(e) => {
                    tracing::warn!(error = %e, platform, "IGDB lookup failed");
                    continue;
                }
            };
            for (game, pgid) in items {
                if let Some(id) = map.get(pgid) {
                    resolved.entry(*game).or_insert(*id);
                }
            }
        }
        // Games IGDB does not know are remembered so they are not looked up on every run.
        let looked_up: HashSet<GameId> = pending
            .iter()
            .filter(|(_, p, _)| gameshelf_metadata::igdb::source_id(p).is_some())
            .map(|(g, _, _)| *g)
            .collect();
        for game in looked_up.iter().filter(|g| !resolved.contains_key(g)) {
            self.store
                .add_game_external_id(*game, "igdb-miss", &game.to_string())
                .await?;
        }
        for game in pending
            .iter()
            .map(|(g, _, _)| *g)
            .filter(|g| !looked_up.contains(g))
        {
            self.store
                .add_game_external_id(game, "igdb-miss", &game.to_string())
                .await?;
        }

        let ids: Vec<u64> = resolved
            .values()
            .copied()
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        let details = igdb.games(&ids).await.unwrap_or_default();
        let details: HashMap<u64, gameshelf_metadata::IgdbGame> =
            details.into_iter().map(|g| (g.id, g)).collect();

        let mut count = 0;
        for (game, igdb_id) in resolved {
            let key = igdb_id.to_string();
            let target = match self.store.find_game_by_external("igdb", &key).await? {
                Some(existing) if existing != game => {
                    self.store.merge_games(game, existing).await?;
                    existing
                }
                _ => {
                    self.store.add_game_external_id(game, "igdb", &key).await?;
                    game
                }
            };
            if let Some(info) = details.get(&igdb_id) {
                if !info.name.trim().is_empty() {
                    self.store
                        .set_game_metadata(target, &info.name, info.release_year, "igdb")
                        .await?;
                }
                if let Some(parent) = info.parent
                    && let Some(parent_game) = self
                        .store
                        .find_game_by_external("igdb", &parent.to_string())
                        .await?
                    && parent_game != target
                {
                    self.store.set_game_parent(target, Some(parent_game)).await?;
                }
                if let (Some(url), Some(cache)) = (&info.cover_url, &self.artwork)
                    && let Ok(img) = cache.fetch(url).await
                {
                    let asset = self
                        .store
                        .put_asset(&img.sha256, img.mime, img.width, img.height, "igdb", Some(url))
                        .await?;
                    self.store.set_game_cover(target, &asset).await?;
                }
            }
            count += 1;
        }
        Ok(count)
    }

    /// Downloads platform artwork for games without a cover. Returns the number of covers set.
    pub async fn fetch_missing_covers(&self, limit: i64) -> Result<usize, SyncError> {
        let Some(cache) = &self.artwork else { return Ok(0) };
        let mut set = 0;
        for (game, urls) in self.store.games_without_cover(limit).await? {
            for url in urls {
                match cache.fetch(&url).await {
                    Ok(img) => {
                        let asset = self
                            .store
                            .put_asset(
                                &img.sha256,
                                img.mime,
                                img.width,
                                img.height,
                                "platform",
                                Some(&url),
                            )
                            .await?;
                        self.store.set_game_cover(game, &asset).await?;
                        set += 1;
                        break;
                    }
                    Err(e) => tracing::debug!(error = %e, "artwork candidate failed"),
                }
            }
        }
        Ok(set)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gameshelf_connectors::{IdSource, fake};
    use gameshelf_core::{ProfileId, Role, account_status::AccountStatus};
    use gameshelf_store::{NewAccount, NewProfile};
    use secrecy::{SecretBox, SecretString};

    async fn setup() -> (Importer, ProfileId) {
        let store = Store::open_in_memory(&SecretBox::new(Box::new([9u8; 32])))
            .await
            .expect("store");
        let pw = SecretString::from("a long enough password".to_string());
        let p = store
            .create_profile(NewProfile {
                username: "player-one",
                display_name: "Player One",
                email: None,
                password: &pw,
                role: Role::Admin,
            })
            .await
            .expect("profile");
        (Importer::new(store, None, None), p.id)
    }

    async fn account(imp: &Importer, profile: ProfileId, platform: &str, ext: &str) -> Account {
        imp.store()
            .create_account(NewAccount {
                profile_id: profile,
                platform,
                external_account_id: ext,
                display_name: None,
                label: Some(ext),
                status: AccountStatus::Active,
            })
            .await
            .expect("account")
    }

    fn snapshot(items: Vec<LibraryItem>) -> LibrarySnapshot {
        LibrarySnapshot {
            complete: true,
            items,
        }
    }

    #[tokio::test]
    async fn same_game_on_two_accounts_is_one_game_with_two_ownerships() {
        let (imp, p) = setup().await;
        let a = account(&imp, p, "steam", "acct-a").await;
        let b = account(&imp, p, "steam", "acct-b").await;
        let lib = snapshot(vec![fake::item("steam", "620", "Starlight Harbor", Some(10))]);
        let ra = imp.import(&a, &lib, OwnershipSource::Connector).await.expect("a");
        let rb = imp.import(&b, &lib, OwnershipSource::Connector).await.expect("b");
        assert_eq!(ra.added_games, rb.added_games);
        let page = imp
            .store()
            .library(p, &Default::default())
            .await
            .expect("library");
        assert_eq!(page.total, 1);
        assert_eq!(page.games[0].account_count, 2);
        assert_eq!(page.games[0].platforms, vec!["steam".to_string()]);
    }

    #[tokio::test]
    async fn same_title_on_two_platforms_groups_and_keeps_both_badges() {
        let (imp, p) = setup().await;
        let s = account(&imp, p, "steam", "acct-s").await;
        let e = account(&imp, p, "epic", "acct-e").await;
        imp.import(
            &s,
            &snapshot(vec![fake::item("steam", "1", "Lantern Run", None)]),
            OwnershipSource::Connector,
        )
        .await
        .expect("steam");
        imp.import(
            &e,
            &snapshot(vec![fake::item(
                "epic",
                "ns:abc",
                "Lantern Run - Deluxe Edition",
                None,
            )]),
            OwnershipSource::Connector,
        )
        .await
        .expect("epic");
        let page = imp
            .store()
            .library(p, &Default::default())
            .await
            .expect("library");
        assert_eq!(page.total, 1, "{:?}", page.games);
        let mut platforms = page.games[0].platforms.clone();
        platforms.sort();
        assert_eq!(platforms, vec!["epic".to_string(), "steam".to_string()]);
        let owns = imp
            .store()
            .game_ownerships(p, page.games[0].game_id)
            .await
            .expect("detail");
        assert_eq!(owns.len(), 2);
        assert!(
            owns.iter()
                .any(|o| o.edition.as_deref() == Some("Deluxe Edition"))
        );
    }

    #[tokio::test]
    async fn cross_ids_match_across_platforms() {
        let (imp, p) = setup().await;
        let s = account(&imp, p, "steam", "acct-s").await;
        let g = account(&imp, p, "gog", "acct-g").await;
        imp.import(
            &s,
            &snapshot(vec![fake::item("steam", "42", "Cobalt Orchard", None)]),
            OwnershipSource::Connector,
        )
        .await
        .expect("steam");
        let mut item = fake::item("gog", "777", "Cobalt Orchard: Director's Cut", None);
        item.cross_ids.push((IdSource::Steam, "42".into()));
        imp.import(&g, &snapshot(vec![item]), OwnershipSource::Connector)
            .await
            .expect("gog");
        let page = imp
            .store()
            .library(p, &Default::default())
            .await
            .expect("library");
        assert_eq!(page.total, 1);
    }

    #[tokio::test]
    async fn dlc_attach_to_their_parent_and_stay_out_of_the_grid() {
        let (imp, p) = setup().await;
        let a = account(&imp, p, "steam", "acct-a").await;
        let mut dlc = fake::item("steam", "101", "Ember Atlas: Frost Expansion", None);
        dlc.kind = ProductKind::Dlc;
        dlc.parent_platform_game_id = Some("100".into());
        let lib = snapshot(vec![fake::item("steam", "100", "Ember Atlas", None), dlc]);
        imp.import(&a, &lib, OwnershipSource::Connector)
            .await
            .expect("import");
        let page = imp
            .store()
            .library(p, &Default::default())
            .await
            .expect("library");
        assert_eq!(page.total, 1);
        assert_eq!(page.games[0].title, "Ember Atlas");
        let dlcs = imp.store().dlc_of(page.games[0].game_id).await.expect("dlc");
        assert_eq!(dlcs.len(), 1);
    }

    #[tokio::test]
    async fn removed_games_are_reported_and_hidden() {
        let (imp, p) = setup().await;
        let a = account(&imp, p, "steam", "acct-a").await;
        let two = snapshot(vec![
            fake::item("steam", "1", "Paper Comets", None),
            fake::item("steam", "2", "Signal Lost", None),
        ]);
        imp.import(&a, &two, OwnershipSource::Connector)
            .await
            .expect("first");
        let one = snapshot(vec![fake::item("steam", "1", "Paper Comets", None)]);
        let r = imp
            .import(&a, &one, OwnershipSource::Connector)
            .await
            .expect("second");
        assert_eq!(r.unavailable_games.len(), 1);
        assert_eq!(
            imp.store()
                .library(p, &Default::default())
                .await
                .expect("library")
                .total,
            1
        );
        let with_hidden = imp
            .store()
            .library(
                p,
                &gameshelf_store::LibraryQuery {
                    include_unavailable: true,
                    ..Default::default()
                },
            )
            .await
            .expect("library");
        assert_eq!(with_hidden.total, 2);
    }

    #[tokio::test]
    async fn a_merge_decision_survives_reimport() {
        let (imp, p) = setup().await;
        let a = account(&imp, p, "steam", "acct-a").await;
        let lib = snapshot(vec![
            fake::item("steam", "1", "Orbit Bakery", None),
            fake::item("steam", "2", "Orbit Bakery Remix", None),
        ]);
        imp.import(&a, &lib, OwnershipSource::Connector)
            .await
            .expect("import");
        let page = imp
            .store()
            .library(p, &Default::default())
            .await
            .expect("library");
        assert_eq!(page.total, 2);
        let target = page
            .games
            .iter()
            .find(|g| g.title == "Orbit Bakery")
            .expect("target")
            .game_id;
        let other = imp
            .store()
            .find_product("steam", "2")
            .await
            .expect("q")
            .expect("product");
        imp.store()
            .record_match_decision(other.id, Some(target), Some(p))
            .await
            .expect("merge");
        imp.import(&a, &lib, OwnershipSource::Connector)
            .await
            .expect("reimport");
        assert_eq!(
            imp.store()
                .library(p, &Default::default())
                .await
                .expect("library")
                .total,
            1
        );
    }
}
