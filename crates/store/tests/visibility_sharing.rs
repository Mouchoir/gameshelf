//! Visibility and sharing: nothing excluded may leak through any read path (tasks T110, SC-011).

use gameshelf_core::{
    Acquisition, ProductKind, ProfileId, Role, account_status::AccountStatus, view::LibraryQuery,
};
use gameshelf_store::{
    NewAccount, NewProduct, NewProfile, OwnershipSource, SnapshotItem, Store, repo::sharing::ExclusionKind,
};
use secrecy::{SecretBox, SecretString};

struct World {
    store: Store,
    alice: ProfileId,
    bob: ProfileId,
    carol: ProfileId,
    shared_game: gameshelf_core::GameId,
    epic_only_game: gameshelf_core::GameId,
    secret_game: gameshelf_core::GameId,
    bob_steam: gameshelf_core::AccountId,
}

async fn profile(store: &Store, username: &str, name: &str) -> ProfileId {
    let pw = SecretString::from("a long enough password".to_string());
    store
        .create_profile(NewProfile {
            username,
            display_name: name,
            email: None,
            password: &pw,
            role: Role::Admin,
        })
        .await
        .expect("profile")
        .id
}

async fn own(
    store: &Store,
    account: gameshelf_core::AccountId,
    platform: &str,
    pgid: &str,
    title: &str,
) -> gameshelf_core::GameId {
    let p = store
        .upsert_product(&NewProduct {
            platform,
            platform_game_id: pgid,
            title,
            kind: ProductKind::Game,
            parent_platform_game_id: None,
            edition: None,
            artwork: None,
            raw: None,
        })
        .await
        .expect("product");
    let game = match p.game_id {
        Some(g) => g,
        None => {
            let existing = store
                .find_games_by_search_key(&gameshelf_core::normalize::search_key(title))
                .await
                .expect("q");
            let g = match existing.first() {
                Some((g, _)) => *g,
                None => store
                    .create_game(title, ProductKind::Game, None, None, "platform")
                    .await
                    .expect("game"),
            };
            store.set_product_game(p.id, Some(g)).await.expect("attach");
            g
        }
    };
    let existing: Vec<SnapshotItem> = Vec::new();
    let _ = existing;
    store
        .apply_snapshot(
            account,
            &[SnapshotItem {
                product_id: p.id,
                acquisition: Acquisition::Purchase,
                playtime_minutes: Some(10),
                last_played_at: None,
                achievements: None,
            }],
            false,
            OwnershipSource::Connector,
        )
        .await
        .expect("own");
    game
}

async fn account(store: &Store, profile: ProfileId, platform: &str, ext: &str) -> gameshelf_core::AccountId {
    store
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
        .id
}

async fn world() -> World {
    let store = Store::open_in_memory(&SecretBox::new(Box::new([5u8; 32])))
        .await
        .expect("store");
    let alice = profile(&store, "alice", "Alice").await;
    let bob = profile(&store, "bob", "Bob").await;
    let carol = profile(&store, "carol", "Carol").await;
    let bob_steam = account(&store, bob, "steam", "bob-steam").await;
    let bob_epic = account(&store, bob, "epic", "bob-epic").await;
    let shared_game = own(&store, bob_steam, "steam", "1", "Lantern Run").await;
    let epic_only_game = own(&store, bob_epic, "epic", "ns:2", "Paper Comets").await;
    let secret_game = own(&store, bob_steam, "steam", "3", "Night Market Detective").await;
    World {
        store,
        alice,
        bob,
        carol,
        shared_game,
        epic_only_game,
        secret_game,
        bob_steam,
    }
}

async fn visible_titles(store: &Store, viewer: ProfileId) -> Vec<String> {
    let mut t: Vec<String> = store
        .library(viewer, &LibraryQuery::default())
        .await
        .expect("library")
        .games
        .into_iter()
        .map(|g| g.title)
        .collect();
    t.sort();
    t
}

#[tokio::test]
async fn nothing_is_shared_by_default() {
    let w = world().await;
    assert!(visible_titles(&w.store, w.alice).await.is_empty());
    assert!(
        w.store
            .game_ownerships(w.alice, w.shared_game)
            .await
            .expect("detail")
            .is_empty()
    );
    assert!(w.store.visible_game_keys(w.alice).await.expect("keys").is_empty());
    assert_eq!(visible_titles(&w.store, w.bob).await.len(), 3);
}

#[tokio::test]
async fn sharing_reveals_everything_to_the_audience_only() {
    let w = world().await;
    w.store.set_sharing(w.bob, w.alice, true).await.expect("share");
    assert_eq!(visible_titles(&w.store, w.alice).await.len(), 3);
    assert!(
        visible_titles(&w.store, w.carol).await.is_empty(),
        "Carol is not an audience"
    );
    let page = w
        .store
        .library(w.alice, &LibraryQuery::default())
        .await
        .expect("library");
    assert!(
        page.games
            .iter()
            .all(|g| g.other_owners == vec![w.bob] && g.own_account_count == 0)
    );
}

#[tokio::test]
async fn excluded_games_platforms_and_accounts_never_leak() {
    let w = world().await;
    w.store.set_sharing(w.bob, w.alice, true).await.expect("share");
    w.store
        .add_exclusion(w.bob, None, ExclusionKind::Game, &w.secret_game.to_string())
        .await
        .expect("game");
    w.store
        .add_exclusion(w.bob, Some(w.alice), ExclusionKind::Platform, "epic")
        .await
        .expect("platform");

    assert_eq!(
        visible_titles(&w.store, w.alice).await,
        vec!["Lantern Run".to_string()]
    );
    // Detail, search keys, filters and counts follow the same rule.
    assert!(
        w.store
            .game_ownerships(w.alice, w.secret_game)
            .await
            .expect("detail")
            .is_empty()
    );
    assert!(
        w.store
            .game_ownerships(w.alice, w.epic_only_game)
            .await
            .expect("detail")
            .is_empty()
    );
    let keys = w.store.visible_game_keys(w.alice).await.expect("keys");
    assert_eq!(keys.len(), 1);
    let epic = w
        .store
        .library(
            w.alice,
            &LibraryQuery {
                platforms: vec!["epic".into()],
                ..Default::default()
            },
        )
        .await
        .expect("filtered");
    assert_eq!(epic.total, 0);
    let restricted = w
        .store
        .library(
            w.alice,
            &LibraryQuery {
                games: Some(vec![w.secret_game, w.epic_only_game]),
                ..Default::default()
            },
        )
        .await
        .expect("by id");
    assert_eq!(
        restricted.total, 0,
        "asking for excluded games by id returns nothing"
    );
    // The owner still sees everything.
    assert_eq!(visible_titles(&w.store, w.bob).await.len(), 3);

    // Excluding the account hides the rest.
    w.store
        .add_exclusion(w.bob, None, ExclusionKind::Account, &w.bob_steam.to_string())
        .await
        .expect("account");
    assert!(visible_titles(&w.store, w.alice).await.is_empty());

    // Lifting exclusions brings games back.
    w.store
        .remove_exclusion(w.bob, ExclusionKind::Account, &w.bob_steam.to_string(), None)
        .await
        .expect("remove");
    w.store
        .remove_exclusion(w.bob, ExclusionKind::Platform, "epic", Some(w.alice))
        .await
        .expect("remove");
    assert_eq!(visible_titles(&w.store, w.alice).await.len(), 2);
}

#[tokio::test]
async fn a_game_owned_on_an_excluded_and_a_visible_platform_shows_only_the_visible_ownership() {
    let w = world().await;
    let bob_gog = account(&w.store, w.bob, "gog", "bob-gog").await;
    own(&w.store, bob_gog, "gog", "99", "Lantern Run").await;
    w.store.set_sharing(w.bob, w.alice, true).await.expect("share");
    w.store
        .add_exclusion(w.bob, None, ExclusionKind::Platform, "steam")
        .await
        .expect("exclude steam");
    let owns = w
        .store
        .game_ownerships(w.alice, w.shared_game)
        .await
        .expect("detail");
    assert_eq!(owns.len(), 1);
    assert_eq!(owns[0].platform, "gog");
    let page = w
        .store
        .library(w.alice, &LibraryQuery::default())
        .await
        .expect("library");
    let lantern = page
        .games
        .iter()
        .find(|g| g.title == "Lantern Run")
        .expect("visible");
    assert_eq!(lantern.platforms, vec!["gog".to_string()]);
    assert_eq!(lantern.account_count, 1);
}

#[tokio::test]
async fn deleting_the_owner_removes_access_immediately() {
    let w = world().await;
    w.store.set_sharing(w.bob, w.alice, true).await.expect("share");
    assert!(!visible_titles(&w.store, w.alice).await.is_empty());
    w.store.delete_profile(w.bob).await.expect("delete");
    assert!(visible_titles(&w.store, w.alice).await.is_empty());
}
