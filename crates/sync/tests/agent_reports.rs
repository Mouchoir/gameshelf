//! Agent reports through the import pipeline (tasks T091, T094, quickstart V8). Synthetic data only.

use gameshelf_connectors::{
    LibrarySnapshot, Registry,
    fake::{self, FakeConnector},
};
use gameshelf_core::{
    Acquisition, ProductKind, ProfileId, Role,
    account_status::AccountStatus,
    agent::{Report, ReportAccount, ReportItem, SourceConfig},
};
use gameshelf_store::{
    Agent, AgentSettings, NewAccount, NewProfile, OwnershipSource, Store, VisibleOwnership,
};
use gameshelf_sync::{AgentImporter, Importer, ReportCheck, ReportRejection};
use secrecy::{SecretBox, SecretString};
use std::sync::Arc;

const PLAYER: &str = "76561197960266729";

struct World {
    store: Store,
    agents: AgentImporter,
    profile: ProfileId,
    agent: Agent,
}

async fn profile(store: &Store, username: &str) -> ProfileId {
    let pw = SecretString::from("a long enough password".to_string());
    store
        .create_profile(NewProfile {
            username,
            display_name: username,
            email: None,
            password: &pw,
            role: Role::Admin,
        })
        .await
        .expect("profile")
        .id
}

async fn world() -> World {
    let store = Store::open_in_memory(&SecretBox::new(Box::new([9u8; 32])))
        .await
        .expect("store");
    let profile = profile(&store, "player-one").await;
    let (code, _) = store.create_pairing_code(profile).await.expect("code");
    let agent = store
        .pair_agent(&code, "Living-room PC", "windows", "0.1.0")
        .await
        .expect("pair")
        .expect("valid code")
        .agent;
    let mut registry = Registry::default();
    registry.register(Arc::new(FakeConnector::new("steam")));
    let agents = AgentImporter::new(Importer::new(store.clone(), None, None), registry);
    World {
        store,
        agents,
        profile,
        agent,
    }
}

fn item(id: &str, title: Option<&str>, installed: bool, playtime: Option<u32>) -> ReportItem {
    ReportItem {
        platform_game_id: id.into(),
        title: title.map(str::to_string),
        kind: ProductKind::Game,
        installed,
        size_bytes: Some(1000),
        playtime_minutes: playtime,
        last_played_at: None,
        install_path: Some("/games/example".into()),
    }
}

fn report(accounts: Vec<ReportAccount>, machine: Vec<ReportItem>, hash: &str) -> Report {
    Report {
        source: "steam-local".into(),
        platform: "steam".into(),
        generated_at: time::OffsetDateTime::now_utc(),
        content_hash: hash.into(),
        accounts,
        machine_items: machine,
    }
}

fn account(items: Vec<ReportItem>) -> ReportAccount {
    ReportAccount {
        external_account_id: PLAYER.into(),
        display_name: Some("Player One".into()),
        items,
    }
}

async fn owned(store: &Store, viewer: ProfileId) -> Vec<String> {
    let mut titles: Vec<String> = store
        .library(viewer, &Default::default())
        .await
        .expect("library")
        .games
        .into_iter()
        .map(|g| g.title)
        .collect();
    titles.sort();
    titles
}

async fn ownership(store: &Store, viewer: ProfileId, title: &str) -> VisibleOwnership {
    let page = store.library(viewer, &Default::default()).await.expect("library");
    let game = page
        .games
        .iter()
        .find(|g| g.title == title)
        .expect("game")
        .game_id;
    store
        .game_ownerships(viewer, game)
        .await
        .expect("detail")
        .into_iter()
        .next()
        .expect("ownership")
}

#[tokio::test]
async fn unknown_accounts_are_created_for_the_agent_owner_and_reports_dedupe() {
    let w = world().await;
    let r = report(
        vec![account(vec![
            item("10", Some("Lantern Run"), true, Some(125)),
            item("30", None, false, Some(60)),
        ])],
        vec![],
        "sha256:one",
    );
    assert_eq!(
        w.agents.check(&w.agent, &r).await.expect("db"),
        Ok(ReportCheck::Accepted)
    );
    let applied = w.agents.apply(&w.agent, &r).await.expect("apply");
    assert_eq!(applied.items, 1);
    assert_eq!(applied.unresolved, 1, "untitled and unknown to the platform");
    assert_eq!(applied.runs.len(), 1);

    let acc = w
        .store
        .find_account_by_external("steam", PLAYER)
        .await
        .expect("db")
        .expect("created");
    assert_eq!(acc.profile_id, w.profile);
    assert_eq!(acc.agent_id, Some(w.agent.id));
    assert!(!acc.sync_enabled, "agent accounts are not scheduled");
    assert_eq!(owned(&w.store, w.profile).await, vec!["Lantern Run".to_string()]);
    assert_eq!(w.store.count_installs(w.agent.id).await.expect("installs"), 1);

    // The same content is not applied twice.
    assert_eq!(
        w.agents.check(&w.agent, &r).await.expect("db"),
        Ok(ReportCheck::Unchanged)
    );
}

#[tokio::test]
async fn reports_enrich_connected_accounts_without_overriding_them() {
    let w = world().await;
    let acc = w
        .store
        .create_account(NewAccount {
            profile_id: w.profile,
            platform: "steam",
            external_account_id: PLAYER,
            display_name: None,
            label: Some("Main"),
            status: AccountStatus::Active,
        })
        .await
        .expect("account");
    // The connector knows the game as a free claim with more playtime than this machine saw.
    let mut listed = fake::item("steam", "10", "Lantern Run", Some(500));
    listed.acquisition = Acquisition::FreeClaim;
    Importer::new(w.store.clone(), None, None)
        .import(
            &acc,
            &LibrarySnapshot {
                complete: true,
                items: vec![listed],
            },
            OwnershipSource::Connector,
        )
        .await
        .expect("import");

    let r = report(
        vec![account(vec![item(
            "10",
            Some("Lantern Run (local name)"),
            true,
            Some(120),
        )])],
        vec![],
        "sha256:two",
    );
    w.agents.apply(&w.agent, &r).await.expect("apply");
    let o = ownership(&w.store, w.profile, "Lantern Run").await;
    assert_eq!(
        o.acquisition,
        Acquisition::FreeClaim,
        "agents never change how a game was acquired"
    );
    assert_eq!(o.playtime_minutes, Some(500), "agents never lower playtime");
    assert_eq!(
        w.store
            .find_account_by_external("steam", PLAYER)
            .await
            .expect("db")
            .expect("acc")
            .agent_id,
        None,
        "the connected account is used, not duplicated"
    );
}

#[tokio::test]
async fn machine_items_go_to_the_only_account_or_to_an_agent_account() {
    let w = world().await;
    let r = report(
        vec![],
        vec![item("20", Some("Paper Comets"), true, None)],
        "sha256:m1",
    );
    w.agents.apply(&w.agent, &r).await.expect("apply");
    let machine = w
        .store
        .find_account_by_external("steam", &format!("agent:{}", w.agent.id))
        .await
        .expect("db")
        .expect("agent account");
    assert_eq!(machine.label, "Living-room PC");

    // With exactly one connected account, machine games are attributed to it.
    let w = world().await;
    let only = w
        .store
        .create_account(NewAccount {
            profile_id: w.profile,
            platform: "steam",
            external_account_id: "76561197960266999",
            display_name: None,
            label: Some("Only"),
            status: AccountStatus::Active,
        })
        .await
        .expect("account");
    w.agents.apply(&w.agent, &r).await.expect("apply");
    let o = ownership(&w.store, w.profile, "Paper Comets").await;
    assert_eq!(o.account_id, only.id);
}

#[tokio::test]
async fn accounts_of_other_profiles_are_never_touched() {
    let w = world().await;
    let other = profile(&w.store, "player-two").await;
    w.store
        .create_account(NewAccount {
            profile_id: other,
            platform: "steam",
            external_account_id: PLAYER,
            display_name: None,
            label: Some("Theirs"),
            status: AccountStatus::Active,
        })
        .await
        .expect("account");
    let r = report(
        vec![account(vec![item("10", Some("Lantern Run"), true, None)])],
        vec![],
        "sha256:x",
    );
    let applied = w.agents.apply(&w.agent, &r).await.expect("apply");
    assert!(applied.runs.is_empty());
    assert!(
        owned(&w.store, other).await.is_empty(),
        "nothing imported into another profile"
    );
    assert!(owned(&w.store, w.profile).await.is_empty());
}

#[tokio::test]
async fn disabled_sources_and_mismatched_platforms_are_refused() {
    let w = world().await;
    let mut sources = std::collections::BTreeMap::new();
    sources.insert(
        "steam-local".to_string(),
        SourceConfig {
            enabled: false,
            paths: vec![],
        },
    );
    w.store
        .update_agent_settings(
            w.agent.id,
            w.profile,
            &AgentSettings {
                scan_hours: 12,
                send_install_paths: false,
                sources,
            },
        )
        .await
        .expect("settings");
    let agent = w.store.get_agent(w.agent.id).await.expect("db").expect("agent");
    assert_eq!(agent.config().scan_hours, 12);
    let r = report(vec![], vec![item("1", Some("A"), true, None)], "sha256:d");
    assert_eq!(
        w.agents.check(&agent, &r).await.expect("db"),
        Err(ReportRejection::SourceDisabled)
    );

    let mut wrong = r.clone();
    wrong.source = "epic-local".into();
    assert_eq!(
        w.agents.check(&agent, &wrong).await.expect("db"),
        Err(ReportRejection::Unknown)
    );
    let mut console = r.clone();
    console.source = "launchers-local".into();
    console.platform = "playstation".into();
    assert_eq!(
        w.agents.check(&agent, &console).await.expect("db"),
        Err(ReportRejection::Unknown)
    );
}

#[tokio::test]
async fn install_paths_are_kept_only_with_consent() {
    let w = world().await;
    let r = report(
        vec![],
        vec![item("20", Some("Paper Comets"), true, None)],
        "sha256:p",
    );
    w.agents.apply(&w.agent, &r).await.expect("apply");
    let paths: Vec<Option<String>> = sqlx::query_scalar("SELECT install_path FROM installs")
        .fetch_all(w.store.pool())
        .await
        .expect("q");
    assert_eq!(paths, vec![None]);
}
