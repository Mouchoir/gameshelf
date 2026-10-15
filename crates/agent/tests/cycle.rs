//! Agent cycles against a mock server (tasks T106, T108). Synthetic data only, no real launcher is read.

use gameshelf_agent::service::{Agent, CycleError};
use gameshelf_core::agent::{Report, ReportItem};
use gameshelf_local::{LocalError, LocalSource, ScanOptions, finish};
use serde_json::json;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{header, method, path},
};

/// A source that always finds one game.
struct FakeSteam;

#[async_trait::async_trait]
impl LocalSource for FakeSteam {
    fn id(&self) -> &'static str {
        "steam-local"
    }

    fn detect(&self, _opts: &ScanOptions) -> bool {
        true
    }

    async fn scan(&self, _opts: &ScanOptions) -> Result<Vec<Report>, LocalError> {
        Ok(vec![finish(
            "steam-local",
            "steam",
            Vec::new(),
            vec![ReportItem {
                platform_game_id: "10".into(),
                title: Some("Lantern Run".into()),
                kind: gameshelf_core::ProductKind::Game,
                installed: true,
                size_bytes: None,
                playtime_minutes: None,
                last_played_at: None,
                install_path: None,
            }],
        )])
    }
}

fn config(status: &str) -> serde_json::Value {
    json!({
        "status": status,
        "scan_hours": 6,
        "send_install_paths": false,
        "sources": {
            "steam-local": {"enabled": true, "paths": []},
            "epic-local": {"enabled": true, "paths": []},
            "gog-galaxy-local": {"enabled": true, "paths": []},
            "battlenet-local": {"enabled": true, "paths": []},
            "ubisoft-local": {"enabled": true, "paths": []},
            "launchers-local": {"enabled": true, "paths": []}
        }
    })
}

async fn server(status: &str) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/agent/v1/pair"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "agent_id": "0193f5c2-0000-7000-8000-000000000001",
            "token": "test-token",
            "config": config("active"),
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/agent/v1/config"))
        .and(header("authorization", "Bearer test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(config(status)))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/agent/v1/heartbeat"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/agent/v1/reports"))
        .and(header("content-encoding", "gzip"))
        .respond_with(ResponseTemplate::new(202).set_body_json(json!({"result": "applied"})))
        .mount(&server)
        .await;
    server
}

async fn paired(server: &MockServer) -> (tempfile::TempDir, Agent) {
    let dir = tempfile::tempdir().expect("tmp");
    let agent = Agent::new(dir.path().to_path_buf(), vec![Box::new(FakeSteam)]);
    let cfg = agent
        .pair(&server.uri(), "AAAA-BBBB", "Test PC")
        .await
        .expect("pair");
    assert_eq!(cfg.token.as_deref(), Some("test-token"));
    (dir, agent)
}

async fn reports_sent(server: &MockServer) -> usize {
    server
        .received_requests()
        .await
        .unwrap_or_default()
        .iter()
        .filter(|r| r.url.path() == "/api/agent/v1/reports")
        .count()
}

#[tokio::test]
async fn a_cycle_scans_sends_and_waits_for_the_next_interval() {
    let server = server("active").await;
    let (_dir, agent) = paired(&server).await;
    let r = agent.cycle(false).await.expect("cycle");
    assert!(r.scanned);
    assert_eq!(
        r.sent,
        vec![("steam-local".into(), "steam".into(), "applied".into())]
    );
    let again = agent.cycle(false).await.expect("cycle");
    assert!(!again.scanned, "the next scan waits for the interval");
    assert_eq!(reports_sent(&server).await, 1);
    let heartbeats = server
        .received_requests()
        .await
        .unwrap_or_default()
        .iter()
        .filter(|r| r.url.path() == "/api/agent/v1/heartbeat")
        .count();
    assert_eq!(heartbeats, 1, "heartbeat after the scan, not on every pull");
}

#[tokio::test]
async fn paused_agents_and_disabled_sources_send_nothing() {
    let server = server("paused").await;
    let (_dir, agent) = paired(&server).await;
    let r = agent.cycle(true).await.expect("cycle");
    assert!(r.paused);
    assert_eq!(reports_sent(&server).await, 0);

    let server = self::server("active").await;
    let (_dir, agent) = paired(&server).await;
    let mut cfg = agent.load().expect("load");
    cfg.disabled_sources.insert("steam-local".into());
    agent.save(&cfg).expect("save");
    let r = agent.cycle(true).await.expect("cycle");
    assert!(r.scanned);
    assert!(r.sent.is_empty());
    assert_eq!(
        reports_sent(&server).await,
        0,
        "a source turned off locally is never read"
    );

    let mut cfg = agent.load().expect("load");
    cfg.disabled_sources.clear();
    cfg.paused = true;
    agent.save(&cfg).expect("save");
    agent.cycle(true).await.expect("cycle");
    assert_eq!(reports_sent(&server).await, 0, "paused on this computer");
}

#[tokio::test]
async fn a_removed_agent_forgets_its_pairing() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/agent/v1/pair"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "agent_id": "0193f5c2-0000-7000-8000-000000000001",
            "token": "test-token",
            "config": config("active"),
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/agent/v1/config"))
        .respond_with(
            ResponseTemplate::new(401)
                .set_body_json(json!({"error": {"code": "agent_revoked", "message": "removed"}})),
        )
        .mount(&server)
        .await;
    let (_dir, agent) = paired(&server).await;
    assert!(matches!(agent.cycle(false).await, Err(CycleError::Revoked)));
    assert!(agent.load().expect("load").token.is_none());
    assert!(matches!(agent.cycle(false).await, Err(CycleError::NotPaired)));
}

#[tokio::test]
async fn old_agents_are_told_to_update() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/agent/v1/pair"))
        .respond_with(ResponseTemplate::new(426).set_body_json(json!({
            "error": {"code": "upgrade_required", "message": "update"}, "min_version": "9.0.0"
        })))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().expect("tmp");
    let agent = Agent::new(dir.path().to_path_buf(), vec![Box::new(FakeSteam)]);
    let err = agent
        .pair(&server.uri(), "AAAA-BBBB", "PC")
        .await
        .expect_err("too old");
    assert!(err.contains("9.0.0"), "{err}");
}

#[tokio::test]
async fn unreachable_servers_keep_the_last_settings() {
    let server = server("active").await;
    let (_dir, agent) = paired(&server).await;
    // Nothing listens on port 1.
    let mut cfg = agent.load().expect("load");
    cfg.server_url = Some("http://127.0.0.1:1".into());
    agent.save(&cfg).expect("save");
    let r = agent.cycle(true).await.expect("cycle");
    assert!(r.offline);
    assert!(r.sent.is_empty());
    assert!(
        agent.load().expect("load").paired(),
        "a network error never unpairs"
    );
}
