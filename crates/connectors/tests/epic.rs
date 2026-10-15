//! Epic connector contract tests against anonymized fixtures (contracts/connector.md section 7).

use gameshelf_connectors::{
    AccountRef, Connector, ConnectorError, Context, CredentialPayload, epic::EpicConnector,
};
use gameshelf_core::ProductKind;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{body_string_contains, header_regex, method, path, query_param},
};

const ACCOUNT: &str = "00000000000000000000000000000abc";
/// HTTP Basic header of the launcher's public client: the prefix is base64 of `<client id>:`
/// (33 bytes, so it encodes on its own); the secret part is matched loosely to keep it out of the test.
const LAUNCHER_BASIC_AUTH: &str = "^Basic MzRhMDJjZjhmNDQxNGUyOWIxNTkyMTg3NmRhMzZmOWE6[A-Za-z0-9+/]+=*$";

fn fixture(name: &str) -> serde_json::Value {
    let p = format!("{}/tests/fixtures/epic/{name}", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_str(&std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{p}: {e}"))).expect("json")
}

fn cred() -> CredentialPayload {
    CredentialPayload(
        serde_json::json!({ "access_token": "test-access-token-1", "refresh_token": "test-refresh-token-1" }),
    )
}

fn account() -> AccountRef {
    AccountRef {
        external_account_id: ACCOUNT.into(),
        display_name: None,
    }
}

async fn setup() -> (MockServer, EpicConnector, Context) {
    let server = MockServer::start().await;
    let epic = EpicConnector::with_base(server.uri());
    (server, epic, Context::for_tests())
}

#[tokio::test]
async fn code_exchange_returns_rolling_tokens_and_expiry() {
    let (server, epic, cx) = setup().await;
    Mock::given(method("POST"))
        .and(path("/account/api/oauth/token"))
        .and(body_string_contains("grant_type=authorization_code"))
        .and(body_string_contains("code=AUTHCODE123"))
        .and(header_regex("authorization", LAUNCHER_BASIC_AUTH))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("token.json")))
        .mount(&server)
        .await;
    let refreshed = epic.exchange_code(&cx, "AUTHCODE123").await.expect("exchange");
    assert_eq!(
        refreshed.payload.str("refresh_token"),
        Some("test-refresh-token-1")
    );
    assert_eq!(refreshed.payload.str("account_id"), Some(ACCOUNT));
    assert!(refreshed.expires_at.is_some());
}

#[tokio::test]
async fn refused_code_is_reported_as_invalid() {
    let (server, epic, cx) = setup().await;
    Mock::given(method("POST"))
        .and(path("/account/api/oauth/token"))
        .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({"errorCode": "errors.com.epicgames.account.oauth.authorization_code_not_found"})))
        .mount(&server)
        .await;
    let err = epic.exchange_code(&cx, "WRONG").await.expect_err("must fail");
    assert!(matches!(err, ConnectorError::AuthInvalid(_)), "{err:?}");
}

#[tokio::test]
async fn refresh_failure_needs_reconnect() {
    let (server, epic, cx) = setup().await;
    Mock::given(method("POST"))
        .and(path("/account/api/oauth/token"))
        .and(body_string_contains("grant_type=refresh_token"))
        .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({})))
        .mount(&server)
        .await;
    let err = epic.refresh(&cx, &cred()).await.expect_err("must fail");
    assert_eq!(err, ConnectorError::AuthExpired);
}

#[tokio::test]
async fn validate_reads_the_account_id() {
    let (server, epic, cx) = setup().await;
    Mock::given(method("GET"))
        .and(path("/account/api/oauth/verify"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("verify.json")))
        .mount(&server)
        .await;
    let id = epic.validate(&cx, &cred()).await.expect("valid");
    assert_eq!(id.external_account_id, ACCOUNT);
    assert_eq!(id.display_name.as_deref(), Some("example-user"));
}

async fn mount_library(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/library/api/public/items"))
        .and(query_param("cursor", "cursor-2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("library_page2.json")))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/library/api/public/items"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("library_page1.json")))
        .mount(server)
        .await;
    for (ns, file) in [
        ("ns-one", "catalog_ns_one.json"),
        ("ns-two", "catalog_ns_two.json"),
        ("ns-three", "catalog_ns_three.json"),
    ] {
        Mock::given(method("GET"))
            .and(path(format!("/catalog/api/shared/namespace/{ns}/bulk/items")))
            .respond_with(ResponseTemplate::new(200).set_body_json(fixture(file)))
            .mount(server)
            .await;
    }
    Mock::given(method("GET"))
        .and(path(format!(
            "/library/api/public/playtime/account/{ACCOUNT}/all"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("playtime.json")))
        .mount(server)
        .await;
}

#[tokio::test]
async fn library_pages_filters_engine_and_assets_and_links_dlc() {
    let (server, epic, cx) = setup().await;
    mount_library(&server).await;
    let lib = epic.library(&cx, &account(), &cred()).await.expect("library");
    assert!(lib.complete);
    let titles: Vec<&str> = lib.items.iter().map(|i| i.title.as_str()).collect();
    assert_eq!(
        titles,
        vec!["Starlight Harbor", "Starlight Harbor - Tide Pack", "Paper Comets"]
    );

    let game = &lib.items[0];
    assert_eq!(game.platform_game_id, "ns-one:cat-game-1");
    assert_eq!(game.playtime_minutes, Some(121));
    assert_eq!(
        game.artwork.cover.as_deref(),
        Some("https://cdn.example.com/epic/tall.jpg")
    );

    let dlc = &lib.items[1];
    assert_eq!(dlc.kind, ProductKind::Dlc);
    assert_eq!(dlc.parent_platform_game_id.as_deref(), Some("ns-one:cat-game-1"));

    assert_eq!(lib.items[2].playtime_minutes, None);
}

#[tokio::test]
async fn expired_token_is_an_auth_error() {
    let (server, epic, cx) = setup().await;
    Mock::given(method("GET"))
        .and(path("/library/api/public/items"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;
    let err = epic
        .library(&cx, &account(), &cred())
        .await
        .expect_err("must fail");
    assert!(err.is_auth());
}

#[tokio::test]
async fn a_failing_catalog_makes_the_listing_incomplete() {
    let (server, epic, cx) = setup().await;
    Mock::given(method("GET"))
        .and(path("/library/api/public/items"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("library_page2.json")))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/catalog/api/shared/namespace/ns-two/bulk/items"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/catalog/api/shared/namespace/ns-three/bulk/items"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("catalog_ns_three.json")))
        .mount(&server)
        .await;
    let lib = epic.library(&cx, &account(), &cred()).await.expect("library");
    assert!(
        !lib.complete,
        "a missing catalog must never mark games as removed"
    );
}
