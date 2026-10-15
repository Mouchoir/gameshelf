//! GOG connector contract tests against anonymized fixtures (contracts/connector.md section 7).

use gameshelf_connectors::{
    AccountRef, Connector, ConnectorError, Context, CredentialPayload, gog::GogConnector,
};
use gameshelf_core::ProductKind;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path, query_param},
};

fn fixture(name: &str) -> serde_json::Value {
    let p = format!("{}/tests/fixtures/gog/{name}", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_str(&std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{p}: {e}"))).expect("json")
}

fn cred() -> CredentialPayload {
    CredentialPayload(serde_json::json!({ "access_token": "gog-access-1", "refresh_token": "gog-refresh-1" }))
}

async fn setup() -> (MockServer, GogConnector, Context) {
    let server = MockServer::start().await;
    let gog = GogConnector::with_base(server.uri());
    (server, gog, Context::for_tests())
}

#[tokio::test]
async fn code_exchange_and_refresh() {
    let (server, gog, cx) = setup().await;
    Mock::given(method("GET"))
        .and(path("/token"))
        .and(query_param("grant_type", "authorization_code"))
        .and(query_param("code", "GOGCODE12345"))
        .and(query_param("client_id", "46899977096215655"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("token.json")))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/token"))
        .and(query_param("grant_type", "refresh_token"))
        .and(query_param("refresh_token", "gog-refresh-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("token.json")))
        .mount(&server)
        .await;
    let first = gog.exchange_code(&cx, "GOGCODE12345").await.expect("exchange");
    assert_eq!(first.payload.str("user_id"), Some("48600000000000001"));
    let refreshed = gog
        .refresh(&cx, &cred())
        .await
        .expect("refresh")
        .expect("rotated");
    assert_eq!(refreshed.payload.str("access_token"), Some("gog-access-1"));
}

#[tokio::test]
async fn validate_requires_a_signed_in_user() {
    let (server, gog, cx) = setup().await;
    Mock::given(method("GET"))
        .and(path("/userData.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("user_data.json")))
        .mount(&server)
        .await;
    let id = gog.validate(&cx, &cred()).await.expect("valid");
    assert_eq!(id.external_account_id, "48600000000000001");
    assert_eq!(id.display_name.as_deref(), Some("example-user"));

    let (server, gog, cx) = setup().await;
    Mock::given(method("GET"))
        .and(path("/userData.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("user_data_logged_out.json")))
        .mount(&server)
        .await;
    assert!(matches!(
        gog.validate(&cx, &cred()).await,
        Err(ConnectorError::AuthInvalid(_))
    ));
}

#[tokio::test]
async fn library_titles_kinds_and_delisted_products() {
    let (server, gog, cx) = setup().await;
    Mock::given(method("GET"))
        .and(path("/user/data/games"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("owned.json")))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/products"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("products.json")))
        .mount(&server)
        .await;
    let lib = gog
        .library(
            &cx,
            &AccountRef {
                external_account_id: "48600000000000001".into(),
                display_name: None,
            },
            &cred(),
        )
        .await
        .expect("library");
    assert_eq!(lib.items.len(), 3);
    // Product 1000000099 has no public page: kept as incomplete so it is not marked removed.
    assert!(!lib.complete);
    assert_eq!(lib.items[0].title, "Cobalt Orchard");
    assert_eq!(
        lib.items[0].artwork.header.as_deref(),
        Some("https://images.example.com/logo1.jpg")
    );
    assert_eq!(lib.items[1].kind, ProductKind::Dlc);
    assert_eq!(lib.items[0].playtime_minutes, None);
}

#[tokio::test]
async fn expired_token_is_an_auth_error() {
    let (server, gog, cx) = setup().await;
    Mock::given(method("GET"))
        .and(path("/user/data/games"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;
    let err = gog
        .library(
            &cx,
            &AccountRef {
                external_account_id: "1".into(),
                display_name: None,
            },
            &cred(),
        )
        .await
        .expect_err("must fail");
    assert!(err.is_auth());
}
