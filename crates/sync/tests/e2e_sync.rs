//! End-to-end sync with the fake connector (tasks T063, quickstart V4 and V5).

use gameshelf_connectors::{
    ConnectorError, CredentialPayload, LibrarySnapshot, Registry,
    fake::{self, FakeConnector, Script},
};
use gameshelf_core::{ProfileId, Role, account_status::AccountStatus};
use gameshelf_store::{ActivityKind, NewProfile, RunTrigger, Store};
use gameshelf_sync::{Importer, RunOutcome, Syncer};
use secrecy::{SecretBox, SecretString};
use std::sync::Arc;

async fn setup() -> (Syncer, FakeConnector, ProfileId) {
    let store = Store::open_in_memory(&SecretBox::new(Box::new([3u8; 32])))
        .await
        .expect("store");
    let pw = SecretString::from("a long enough password".to_string());
    let profile = store
        .create_profile(NewProfile {
            username: "player-one",
            display_name: "Player One",
            email: None,
            password: &pw,
            role: Role::Admin,
        })
        .await
        .expect("profile")
        .id;
    let fake = FakeConnector::new("steam");
    let mut registry = Registry::default();
    registry.register(Arc::new(fake.clone()));
    (
        Syncer::new(Importer::new(store, None, None), registry),
        fake,
        profile,
    )
}

fn payload(account: &str) -> CredentialPayload {
    CredentialPayload(serde_json::json!({ "account": account }))
}

#[tokio::test]
async fn connect_then_sync_imports_and_records_activity() {
    let (syncer, fake, profile) = setup().await;
    let account = syncer
        .connect(profile, "steam", payload("acct-a"), None)
        .await
        .expect("db")
        .expect("connected");
    fake.script(
        "acct-a",
        Script::Library(LibrarySnapshot {
            complete: true,
            items: vec![fake::item("steam", "1", "Paper Comets", Some(30))],
        }),
    );
    let outcome = syncer
        .run_account(account, RunTrigger::Manual)
        .await
        .expect("run");
    assert!(
        matches!(outcome, RunOutcome::Imported { ref report, .. } if report.added_games.len() == 1),
        "{outcome:?}"
    );

    // A game added on the platform shows up at the next run, with an activity entry.
    fake.script(
        "acct-a",
        Script::Library(LibrarySnapshot {
            complete: true,
            items: vec![
                fake::item("steam", "1", "Paper Comets", Some(45)),
                fake::item("steam", "2", "Signal Lost", None),
            ],
        }),
    );
    syncer
        .run_account(account, RunTrigger::Schedule)
        .await
        .expect("run");
    let store = syncer.importer().store();
    assert_eq!(
        store
            .library(profile, &Default::default())
            .await
            .expect("library")
            .total,
        2
    );
    let activity = store.list_activity(profile, 10).await.expect("activity");
    assert_eq!(
        activity
            .iter()
            .filter(|e| e.kind == ActivityKind::GameAdded)
            .count(),
        2
    );
    let runs = store.list_runs(account, 10).await.expect("runs");
    assert_eq!(runs.len(), 2);
}

#[tokio::test]
async fn one_failing_account_does_not_affect_another() {
    let (syncer, fake, profile) = setup().await;
    let good = syncer
        .connect(profile, "steam", payload("acct-good"), None)
        .await
        .expect("db")
        .expect("ok");
    let bad = syncer
        .connect(profile, "steam", payload("acct-bad"), None)
        .await
        .expect("db")
        .expect("ok");
    syncer
        .run_account(bad, RunTrigger::Manual)
        .await
        .expect("first run");
    fake.script(
        "acct-bad",
        Script::Fail(ConnectorError::Transient("server error 503".into())),
    );
    let outcome = syncer.run_account(bad, RunTrigger::Schedule).await.expect("run");
    assert!(matches!(outcome, RunOutcome::Failed { .. }));
    syncer.run_account(good, RunTrigger::Schedule).await.expect("run");

    let store = syncer.importer().store();
    let bad_acct = store.get_account(bad).await.expect("get").expect("bad");
    assert_eq!(bad_acct.consecutive_failures, 1);
    // The failed account keeps its previous library.
    assert!(store.count_available_ownerships(bad).await.expect("count") > 0);
    assert!(store.count_available_ownerships(good).await.expect("count") > 0);
}

#[tokio::test]
async fn auth_failure_needs_reconnect_and_incomplete_listing_removes_nothing() {
    let (syncer, fake, profile) = setup().await;
    let account = syncer
        .connect(profile, "steam", payload("acct-a"), None)
        .await
        .expect("db")
        .expect("ok");
    fake.script(
        "acct-a",
        Script::Library(LibrarySnapshot {
            complete: true,
            items: vec![
                fake::item("steam", "1", "Lantern Run", None),
                fake::item("steam", "2", "Ember Atlas", None),
            ],
        }),
    );
    syncer
        .run_account(account, RunTrigger::Manual)
        .await
        .expect("run");
    fake.script(
        "acct-a",
        Script::Library(LibrarySnapshot {
            complete: false,
            items: vec![fake::item("steam", "1", "Lantern Run", None)],
        }),
    );
    syncer
        .run_account(account, RunTrigger::Schedule)
        .await
        .expect("run");
    let store = syncer.importer().store();
    assert_eq!(store.count_available_ownerships(account).await.expect("count"), 2);

    fake.script("acct-a", Script::Fail(ConnectorError::AuthExpired));
    syncer
        .run_account(account, RunTrigger::Schedule)
        .await
        .expect("run");
    let acct = store.get_account(account).await.expect("get").expect("acct");
    assert_eq!(acct.status, AccountStatus::NeedsReconnect);
    let activity = store.list_activity(profile, 10).await.expect("activity");
    assert!(
        activity
            .iter()
            .any(|e| e.kind == ActivityKind::AccountNeedsReconnect)
    );
    // Further runs are skipped until the player reconnects.
    assert!(matches!(
        syncer
            .run_account(account, RunTrigger::Schedule)
            .await
            .expect("run"),
        RunOutcome::Skipped(_)
    ));
    // Reconnecting reactivates the account.
    syncer
        .connect(profile, "steam", payload("acct-a"), None)
        .await
        .expect("db")
        .expect("ok");
    let acct = store.get_account(account).await.expect("get").expect("acct");
    assert_eq!(acct.status, AccountStatus::Active);
}

#[tokio::test]
async fn an_account_held_by_another_profile_is_refused() {
    let (syncer, _fake, profile) = setup().await;
    let store = syncer.importer().store();
    let pw = SecretString::from("a long enough password".to_string());
    let other = store
        .create_profile(NewProfile {
            username: "player-two",
            display_name: "Player Two",
            email: None,
            password: &pw,
            role: Role::Member,
        })
        .await
        .expect("profile")
        .id;
    syncer
        .connect(profile, "steam", payload("acct-a"), None)
        .await
        .expect("db")
        .expect("ok");
    let err = syncer
        .connect(other, "steam", payload("acct-a"), None)
        .await
        .expect("db")
        .expect_err("must fail");
    assert_eq!(err, gameshelf_sync::ConnectError::Taken("Player One".into()));
}
