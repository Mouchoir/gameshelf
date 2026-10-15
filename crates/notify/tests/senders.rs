//! Sender tests against mock servers (tasks T118). No network.

use gameshelf_notify::{Message, NotifyError, Priority, TargetConfig, send};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{body_partial_json, body_string, header, method, path},
};

fn msg() -> Message {
    Message {
        title: "2 new games".into(),
        body: "Hades (Steam, account Main)\nBalatro (Steam, account Second)".into(),
        url: Some("https://gameshelf.example.com/activity".into()),
        priority: Priority::Normal,
    }
}

#[tokio::test]
async fn discord_receives_an_embed() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/webhooks/1/token"))
        .and(body_partial_json(serde_json::json!({
            "username": "gameshelf",
            "embeds": [{ "title": "2 new games", "url": "https://gameshelf.example.com/activity" }]
        })))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    let target = TargetConfig::Discord {
        webhook_url: format!("{}/api/webhooks/1/token", server.uri()),
    };
    send(&reqwest::Client::new(), &target, &msg())
        .await
        .expect("sent");
}

#[tokio::test]
async fn discord_rate_limit_is_retried() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(429))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;
    let target = TargetConfig::Discord {
        webhook_url: format!("{}/api/webhooks/1/token", server.uri()),
    };
    send(&reqwest::Client::new(), &target, &msg())
        .await
        .expect("sent after retry");
}

#[tokio::test]
async fn ntfy_gets_headers_body_and_bearer_token() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/games"))
        .and(header("title", "2 new games"))
        .and(header("priority", "default"))
        .and(header("click", "https://gameshelf.example.com/activity"))
        .and(header("authorization", "Bearer tk_test"))
        .and(body_string(
            "Hades (Steam, account Main)\nBalatro (Steam, account Second)",
        ))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;
    let target = TargetConfig::Ntfy {
        server: format!("{}/", server.uri()),
        topic: "games".into(),
        token: Some("tk_test".into()),
    };
    send(&reqwest::Client::new(), &target, &msg())
        .await
        .expect("sent");
}

#[tokio::test]
async fn gotify_uses_its_key_header() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/message"))
        .and(header("x-gotify-key", "app-token"))
        .and(body_partial_json(
            serde_json::json!({ "title": "2 new games", "priority": 5 }),
        ))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;
    let target = TargetConfig::Gotify {
        server: server.uri(),
        token: "app-token".into(),
    };
    send(&reqwest::Client::new(), &target, &msg())
        .await
        .expect("sent");
}

#[tokio::test]
async fn refused_messages_are_not_retried() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(401))
        .expect(1)
        .mount(&server)
        .await;
    let target = TargetConfig::Gotify {
        server: server.uri(),
        token: "wrong".into(),
    };
    let err = send(&reqwest::Client::new(), &target, &msg())
        .await
        .expect_err("must fail");
    assert!(matches!(err, NotifyError::Status(401)));
}
