//! Agent API contract tests (task T091, contracts/agent-api.md). In-process, synthetic data only.
#![cfg(feature = "ssr")]

use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode, header},
};
use gameshelf_app::server::{AppState, agent_api};
use gameshelf_connectors::Registry;
use gameshelf_core::{ProfileId, Role};
use gameshelf_store::{AgentStatus, NewProfile, Store};
use gameshelf_sync::{Importer, Syncer};
use leptos::prelude::LeptosOptions;
use secrecy::{SecretBox, SecretString};
use serde_json::{Value, json};
use std::io::Write;
use tower::ServiceExt;

const VERSION: &str = "0.1.0";

struct Api {
    app: Router,
    store: Store,
    profile: ProfileId,
    state: AppState,
}

async fn setup() -> Api {
    let store = Store::open_in_memory(&SecretBox::new(Box::new([6u8; 32])))
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
    let syncer = Syncer::new(Importer::new(store.clone(), None, None), Registry::default());
    let state = AppState::new(
        LeptosOptions::builder().output_name("gameshelf").build(),
        store.clone(),
        syncer,
        std::env::temp_dir(),
        false,
    );
    let app = Router::new()
        .nest("/api/agent/v1", agent_api::router())
        .with_state(state.clone());
    Api {
        app,
        store,
        profile,
        state,
    }
}

struct Call<'a> {
    method: &'a str,
    path: &'a str,
    token: Option<&'a str>,
    version: Option<&'a str>,
    body: Option<Vec<u8>>,
    gzip: bool,
    client: &'a str,
}

impl<'a> Call<'a> {
    fn new(method: &'a str, path: &'a str) -> Self {
        Self {
            method,
            path,
            token: None,
            version: Some(VERSION),
            body: None,
            gzip: false,
            client: "198.51.100.7",
        }
    }

    fn token(mut self, t: &'a str) -> Self {
        self.token = Some(t);
        self
    }

    fn json(mut self, v: &Value) -> Self {
        self.body = Some(serde_json::to_vec(v).expect("json"));
        self
    }
}

async fn send(api: &Api, c: Call<'_>) -> (StatusCode, Value) {
    let mut req = Request::builder()
        .method(c.method)
        .uri(format!("/api/agent/v1{}", c.path))
        .header("x-forwarded-for", c.client);
    if let Some(v) = c.version {
        req = req.header(agent_api::VERSION_HEADER, v);
    }
    if let Some(t) = c.token {
        req = req.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    let body = match c.body {
        Some(bytes) => {
            req = req.header(header::CONTENT_TYPE, "application/json");
            if c.gzip {
                req = req.header(header::CONTENT_ENCODING, "gzip");
                let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
                enc.write_all(&bytes).expect("gzip");
                Body::from(enc.finish().expect("gzip"))
            } else {
                Body::from(bytes)
            }
        }
        None => Body::empty(),
    };
    let resp = api
        .app
        .clone()
        .oneshot(req.body(body).expect("request"))
        .await
        .expect("response");
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20)
        .await
        .expect("body");
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

async fn pair(api: &Api) -> (String, String) {
    let (code, _) = api.store.create_pairing_code(api.profile).await.expect("code");
    let (status, body) = send(
        api,
        Call::new("POST", "/pair")
            .json(&json!({"code": code, "name": "Test PC", "os": "linux", "version": VERSION})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    (
        body["token"].as_str().expect("token").to_string(),
        body["agent_id"].as_str().expect("id").to_string(),
    )
}

fn report(hash: &str) -> Value {
    json!({
        "source": "steam-local",
        "platform": "steam",
        "generated_at": "2026-10-15T06:00:02Z",
        "content_hash": hash,
        "accounts": [],
        "machine_items": [
            {"platform_game_id": "20", "title": "Paper Comets", "kind": "game", "installed": true,
             "size_bytes": 1000, "playtime_minutes": null, "last_played_at": null, "install_path": null}
        ]
    })
}

#[tokio::test]
async fn pairing_codes_work_once_and_return_a_config() {
    let api = setup().await;
    let (code, _) = api.store.create_pairing_code(api.profile).await.expect("code");
    let req = json!({"code": code, "name": "Test PC", "os": "windows", "version": VERSION});
    let (status, body) = send(&api, Call::new("POST", "/pair").json(&req)).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["config"]["status"], "active");
    assert_eq!(body["config"]["sources"]["steam-local"]["enabled"], true);
    let (status, body) = send(&api, Call::new("POST", "/pair").json(&req)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_code");

    let old = json!({"code": "AAAA-AAAA", "name": "Old", "os": "windows", "version": "0.0.1"});
    let (status, body) = send(&api, Call::new("POST", "/pair").json(&old)).await;
    assert_eq!(status, StatusCode::UPGRADE_REQUIRED);
    assert_eq!(body["min_version"], VERSION);
}

#[tokio::test]
async fn pairing_attempts_are_limited_per_address() {
    let api = setup().await;
    let req = json!({"code": "WRNG-CODE", "name": "PC", "os": "linux", "version": VERSION});
    for _ in 0..5 {
        let (status, _) = send(&api, Call::new("POST", "/pair").json(&req)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
    let (status, body) = send(&api, Call::new("POST", "/pair").json(&req)).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(body["error"]["code"], "too_many_attempts");
    let mut other = Call::new("POST", "/pair").json(&req);
    other.client = "198.51.100.8";
    assert_eq!(
        send(&api, other).await.0,
        StatusCode::BAD_REQUEST,
        "other addresses are not blocked"
    );
}

#[tokio::test]
async fn calls_need_a_token_and_a_recent_version() {
    let api = setup().await;
    let (token, _) = pair(&api).await;
    let (status, body) = send(&api, Call::new("GET", "/config")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"]["code"], "invalid_token");
    let (status, _) = send(&api, Call::new("GET", "/config").token("not-a-token")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let mut old = Call::new("GET", "/config").token(&token);
    old.version = Some("0.0.9");
    assert_eq!(send(&api, old).await.0, StatusCode::UPGRADE_REQUIRED);
    let (status, body) = send(&api, Call::new("GET", "/config").token(&token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["scan_hours"], 6);
    assert_eq!(body["send_install_paths"], false);
}

#[tokio::test]
async fn heartbeats_are_recorded() {
    let api = setup().await;
    let (token, _) = pair(&api).await;
    let hb = json!({"version": "0.1.2", "autostart": true, "paused": false,
                    "sources": {"steam-local": {"enabled": true, "detected": true, "last_scan_at": null}}});
    let (status, _) = send(&api, Call::new("POST", "/heartbeat").token(&token).json(&hb)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let agent = api.store.list_agents(api.profile).await.expect("db").remove(0);
    assert!(agent.autostart);
    assert!(agent.source_states["steam-local"].detected);
}

#[tokio::test]
async fn reports_are_applied_then_recognized_as_unchanged() {
    let api = setup().await;
    let (token, agent_id) = pair(&api).await;
    let mut call = Call::new("POST", "/reports")
        .token(&token)
        .json(&report("sha256:aa"));
    call.gzip = true;
    let (status, body) = send(&api, call).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    assert_eq!(body["result"], "applied");

    // The report is applied in the background.
    let machine = format!("agent:{agent_id}");
    let mut applied = false;
    for _ in 0..100 {
        if api
            .store
            .find_account_by_external("steam", &machine)
            .await
            .expect("db")
            .is_some()
            && api
                .store
                .last_report_hash(
                    api.store.list_agents(api.profile).await.expect("db")[0].id,
                    "steam-local",
                    "steam",
                )
                .await
                .expect("db")
                .is_some()
        {
            applied = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(applied, "the report was applied");
    let (status, body) = send(
        &api,
        Call::new("POST", "/reports")
            .token(&token)
            .json(&report("sha256:aa")),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(body["result"], "unchanged");

    let mut bad = report("sha256:bb");
    bad["source"] = json!("epic-local");
    let (status, body) = send(&api, Call::new("POST", "/reports").token(&token).json(&bad)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "unknown_source");
    let (status, _) = send(
        &api,
        Call::new("POST", "/reports")
            .token(&token)
            .json(&json!({"nope": 1})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn paused_and_revoked_agents_are_stopped() {
    let api = setup().await;
    let (token, _) = pair(&api).await;
    let agent = api.store.list_agents(api.profile).await.expect("db").remove(0);
    api.store
        .set_agent_status(agent.id, Some(api.profile), AgentStatus::Paused)
        .await
        .expect("pause");
    let (status, body) = send(&api, Call::new("GET", "/config").token(&token)).await;
    assert_eq!(
        (status, body["status"].as_str()),
        (StatusCode::OK, Some("paused"))
    );
    let (status, body) = send(
        &api,
        Call::new("POST", "/reports")
            .token(&token)
            .json(&report("sha256:p")),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "agent_paused");

    let (status, _) = send(&api, Call::new("DELETE", "/self").token(&token)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, body) = send(&api, Call::new("GET", "/config").token(&token)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"]["code"], "agent_revoked");
}

#[tokio::test]
async fn silent_agents_are_reported_once_per_silence() {
    let api = setup().await;
    let (token, _) = pair(&api).await;
    gameshelf_app::server::notifications::check_silent_agents(&api.state).await;
    let silent = |events: Vec<gameshelf_store::ActivityEvent>| {
        events
            .into_iter()
            .filter(|e| e.kind == gameshelf_store::ActivityKind::AgentSilent)
            .count()
    };
    assert_eq!(
        silent(api.store.list_activity(api.profile, 50).await.expect("db")),
        0,
        "just paired"
    );

    let long_ago = time::OffsetDateTime::now_utc() - time::Duration::days(3);
    sqlx::query("UPDATE agents SET last_seen_at = ?")
        .bind(long_ago)
        .execute(api.store.pool())
        .await
        .expect("age");
    gameshelf_app::server::notifications::check_silent_agents(&api.state).await;
    gameshelf_app::server::notifications::check_silent_agents(&api.state).await;
    assert_eq!(
        silent(api.store.list_activity(api.profile, 50).await.expect("db")),
        1
    );

    // A contact ends the silence; the next one is reported again.
    let (status, _) = send(&api, Call::new("GET", "/config").token(&token)).await;
    assert_eq!(status, StatusCode::OK);
    sqlx::query("UPDATE agents SET last_seen_at = ?")
        .bind(long_ago)
        .execute(api.store.pool())
        .await
        .expect("age");
    gameshelf_app::server::notifications::check_silent_agents(&api.state).await;
    assert_eq!(
        silent(api.store.list_activity(api.profile, 50).await.expect("db")),
        2
    );
}
