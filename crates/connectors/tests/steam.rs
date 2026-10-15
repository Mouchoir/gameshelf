//! Steam connector contract tests (contracts/connector.md section 7). No network: a mock server replays the
//! anonymized fixtures in `tests/fixtures/steam/`.

use gameshelf_connectors::{
    AccountRef, Connector, ConnectorError, Context, CredentialPayload, steam::SteamConnector,
};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path, query_param},
};

const STEAM_ID: &str = "76561202255233001";

fn fixture(name: &str) -> serde_json::Value {
    let path = format!("{}/tests/fixtures/steam/{name}", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn cred(steam_id: &str) -> CredentialPayload {
    CredentialPayload(serde_json::json!({ "steam_id": steam_id, "api_key": "TEST-KEY" }))
}

fn account() -> AccountRef {
    AccountRef {
        external_account_id: STEAM_ID.into(),
        display_name: Some("example-user".into()),
    }
}

async fn setup() -> (MockServer, SteamConnector, Context) {
    let server = MockServer::start().await;
    let connector = SteamConnector::with_bases(server.uri(), "https://cdn.example.com/apps");
    (server, connector, Context::for_tests())
}

async fn mount(server: &MockServer, route: &str, status: u16, body: serde_json::Value) {
    Mock::given(method("GET"))
        .and(path(route))
        .respond_with(ResponseTemplate::new(status).set_body_json(body))
        .mount(server)
        .await;
}

#[tokio::test]
async fn validate_accepts_an_id_and_returns_the_persona() {
    let (server, steam, cx) = setup().await;
    mount(
        &server,
        "/ISteamUser/GetPlayerSummaries/v2/",
        200,
        fixture("player_summaries.json"),
    )
    .await;
    let id = steam.validate(&cx, &cred(STEAM_ID)).await.expect("valid");
    assert_eq!(id.external_account_id, STEAM_ID);
    assert_eq!(id.display_name.as_deref(), Some("example-user"));
}

#[tokio::test]
async fn validate_resolves_profile_urls_with_custom_names() {
    let (server, steam, cx) = setup().await;
    Mock::given(method("GET"))
        .and(path("/ISteamUser/ResolveVanityURL/v1/"))
        .and(query_param("vanityurl", "example-user"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("resolve_vanity.json")))
        .mount(&server)
        .await;
    mount(
        &server,
        "/ISteamUser/GetPlayerSummaries/v2/",
        200,
        fixture("player_summaries.json"),
    )
    .await;
    let id = steam
        .validate(&cx, &cred("https://steamcommunity.com/id/example-user/"))
        .await
        .expect("valid");
    assert_eq!(id.external_account_id, STEAM_ID);
}

#[tokio::test]
async fn validate_reports_unknown_custom_names() {
    let (server, steam, cx) = setup().await;
    mount(
        &server,
        "/ISteamUser/ResolveVanityURL/v1/",
        200,
        fixture("resolve_vanity_missing.json"),
    )
    .await;
    let err = steam
        .validate(&cx, &cred("nobody-here"))
        .await
        .expect_err("must fail");
    assert!(matches!(err, ConnectorError::AuthInvalid(_)), "{err:?}");
}

#[tokio::test]
async fn validate_reports_a_refused_key() {
    let (server, steam, cx) = setup().await;
    mount(
        &server,
        "/ISteamUser/GetPlayerSummaries/v2/",
        403,
        serde_json::json!({}),
    )
    .await;
    let err = steam.validate(&cx, &cred(STEAM_ID)).await.expect_err("must fail");
    assert!(matches!(err, ConnectorError::AuthInvalid(_)), "{err:?}");
}

#[tokio::test]
async fn library_lists_games_with_playtime_and_artwork() {
    let (server, steam, cx) = setup().await;
    Mock::given(method("GET"))
        .and(path("/IPlayerService/GetOwnedGames/v1/"))
        .and(query_param("steamid", STEAM_ID))
        .and(query_param("include_played_free_games", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("owned_games.json")))
        .mount(&server)
        .await;
    let lib = steam
        .library(&cx, &account(), &cred(STEAM_ID))
        .await
        .expect("library");
    // The nameless item is skipped, so the listing is not complete.
    assert!(!lib.complete);
    assert_eq!(lib.items.len(), 3);
    let portal = lib
        .items
        .iter()
        .find(|i| i.platform_game_id == "620")
        .expect("portal");
    assert_eq!(portal.title, "Portal 2");
    assert_eq!(portal.playtime_minutes, Some(2148));
    assert!(portal.last_played_at.is_some());
    assert_eq!(
        portal.artwork.cover.as_deref(),
        Some("https://cdn.example.com/apps/620/library_600x900_2x.jpg")
    );
    let cs = lib
        .items
        .iter()
        .find(|i| i.platform_game_id == "730")
        .expect("cs");
    assert_eq!(cs.playtime_minutes, Some(0));
    assert!(cs.last_played_at.is_none());
}

#[tokio::test]
async fn library_of_a_private_profile_explains_what_to_do() {
    let (server, steam, cx) = setup().await;
    mount(
        &server,
        "/IPlayerService/GetOwnedGames/v1/",
        200,
        fixture("owned_games_private.json"),
    )
    .await;
    let err = steam
        .library(&cx, &account(), &cred(STEAM_ID))
        .await
        .expect_err("must fail");
    match err {
        ConnectorError::Permanent(msg) => assert!(msg.contains("own")),
        other => panic!("expected Permanent, got {other:?}"),
    }
}

#[tokio::test]
async fn empty_library_is_complete() {
    let (server, steam, cx) = setup().await;
    mount(
        &server,
        "/IPlayerService/GetOwnedGames/v1/",
        200,
        fixture("owned_games_empty.json"),
    )
    .await;
    let lib = steam
        .library(&cx, &account(), &cred(STEAM_ID))
        .await
        .expect("library");
    assert!(lib.complete);
    assert!(lib.items.is_empty());
}

#[tokio::test]
async fn library_survives_a_rate_limit() {
    let (server, steam, cx) = setup().await;
    Mock::given(method("GET"))
        .and(path("/IPlayerService/GetOwnedGames/v1/"))
        .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "0"))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    mount(
        &server,
        "/IPlayerService/GetOwnedGames/v1/",
        200,
        fixture("owned_games_empty.json"),
    )
    .await;
    steam
        .library(&cx, &account(), &cred(STEAM_ID))
        .await
        .expect("retried");
}

#[tokio::test]
async fn expired_key_is_an_auth_error() {
    let (server, steam, cx) = setup().await;
    mount(
        &server,
        "/IPlayerService/GetOwnedGames/v1/",
        401,
        serde_json::json!({}),
    )
    .await;
    let err = steam
        .library(&cx, &account(), &cred(STEAM_ID))
        .await
        .expect_err("must fail");
    assert!(err.is_auth());
}

#[tokio::test]
async fn achievements_and_level() {
    let (server, steam, cx) = setup().await;
    Mock::given(method("GET"))
        .and(path("/ISteamUserStats/GetPlayerAchievements/v1/"))
        .and(query_param("appid", "620"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("player_achievements.json")))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/ISteamUserStats/GetPlayerAchievements/v1/"))
        .and(query_param("appid", "730"))
        .respond_with(ResponseTemplate::new(400).set_body_json(
            serde_json::json!({"playerstats": {"error": "Requested app has no stats", "success": false}}),
        ))
        .mount(&server)
        .await;
    mount(
        &server,
        "/IPlayerService/GetSteamLevel/v1/",
        200,
        fixture("steam_level.json"),
    )
    .await;

    let ach = steam
        .achievements(
            &cx,
            &account(),
            &cred(STEAM_ID),
            &["620".to_string(), "730".to_string()],
        )
        .await
        .expect("achievements");
    assert_eq!(ach.len(), 1);
    assert_eq!((ach[0].unlocked, ach[0].total), (2, 3));

    let level = steam
        .enrich(&cx, &account(), &cred(STEAM_ID))
        .await
        .expect("level");
    assert_eq!(level[0].key, "platform_level");
    assert_eq!(level[0].value, serde_json::json!(27));
}
