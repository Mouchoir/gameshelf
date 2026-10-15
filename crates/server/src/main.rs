//! gameshelf server entry point.

// Release builds keep full Leptos view types (dev builds erase them), which nest deeper than the default limit.
#![recursion_limit = "512"]

mod api;
mod config;
mod cookies;
mod origin;
mod schedule;

use axum::{
    Router, body::Body, extract::State, http::Request, middleware, response::IntoResponse, routing::get,
};
use gameshelf_app::{App, server::AppState, shell};
use gameshelf_connectors::{Registry, fake::FakeConnector};
use gameshelf_metadata::{ArtworkCache, Igdb};
use gameshelf_store::{SqliteSessionStore, Store};
use gameshelf_sync::{Importer, Syncer};
use leptos::prelude::*;
use leptos_axum::{LeptosRoutes, generate_route_list, handle_server_fns_with_context};
use std::{sync::Arc, time::Duration};
use tower_sessions::{Expiry, SessionManagerLayer, cookie::SameSite};
use tracing_subscriber::{EnvFilter, fmt};

#[tokio::main]
async fn main() {
    let cfg = match config::Config::load() {
        Ok(cfg) => cfg,
        Err(e) => fatal(&e.to_string()),
    };
    init_tracing(&cfg);

    let conf = match get_configuration(None) {
        Ok(conf) => conf,
        Err(e) => fatal(&format!("cannot read the web configuration: {e}")),
    };
    let leptos_options = conf.leptos_options;
    let addr = cfg.bind.unwrap_or(leptos_options.site_addr);

    let store = match Store::open(&cfg.data_dir.join("gameshelf.db"), &cfg.secret_key).await {
        Ok(s) => s,
        Err(e) => fatal(&format!("cannot open the database: {e}")),
    };
    if let Ok(n) = store.fail_interrupted_runs().await
        && n > 0
    {
        tracing::warn!(runs = n, "closed sync runs interrupted by a restart");
    }

    let http = gameshelf_connectors::http::client();
    let igdb = match (cfg.igdb_client_id.clone(), cfg.igdb_client_secret.clone()) {
        (Some(id), Some(secret)) => Some(Igdb::new(http.clone(), id, secret)),
        _ => None,
    };
    let assets_dir = cfg.data_dir.join("assets");
    let artwork = ArtworkCache::new(assets_dir.clone(), http);
    let mut registry = Registry::builtin();
    if cfg.dev_fake_connector {
        tracing::warn!("development fake connector enabled for the steam platform");
        registry.register(Arc::new(FakeConnector::new("steam")));
    }
    let syncer = Syncer::new(Importer::new(store.clone(), igdb, Some(artwork)), registry);
    let state = AppState::new(
        leptos_options.clone(),
        store.clone(),
        syncer,
        assets_dir,
        cfg.https(),
    );
    let state = state.with_notifications(
        cfg.base_url.as_ref().map(|u| u.to_string()),
        gameshelf_connectors::http::client(),
    );
    state.rebuild_search().await;
    spawn_housekeeping(store.clone());
    spawn_agent_watch(state.clone());
    match schedule::Schedule::parse(&cfg.sync_schedule, cfg.time_zone.as_deref()) {
        Ok(s) => schedule::spawn(state.clone(), s),
        Err(e) => fatal(&e.to_string()),
    }

    let sessions = SessionManagerLayer::new(SqliteSessionStore::new(store.clone()))
        .with_name("gameshelf_session")
        // Set per request by `cookies::secure`: on for the public HTTPS address, off on the LAN.
        .with_secure(false)
        .with_http_only(true)
        .with_same_site(SameSite::Lax)
        .with_expiry(Expiry::OnInactivity(time::Duration::days(30)));
    let origin_policy = origin::Policy::new(cfg.base_url.clone());

    let routes = generate_route_list(App);
    let app = Router::new()
        .route("/healthz", get(api::health::healthz))
        .route("/readyz", get(api::health::readyz))
        .route("/assets/{sha256}", get(api::assets::asset))
        .nest("/api/agent/v1", gameshelf_app::server::agent_api::router())
        .route("/api/{*fn_name}", get(server_fn_handler).post(server_fn_handler))
        .leptos_routes_with_context(
            &state,
            routes,
            {
                let state = state.clone();
                move || provide_context(state.clone())
            },
            {
                let options = leptos_options.clone();
                move || shell(options.clone())
            },
        )
        .fallback(leptos_axum::file_and_error_handler::<AppState, _>(shell))
        .layer(sessions)
        .layer(middleware::from_fn_with_state(
            cfg.base_url.clone(),
            cookies::secure,
        ))
        .layer(middleware::from_fn_with_state(origin_policy, origin::check))
        .with_state(state);

    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => fatal(&format!("cannot listen on {addr}: {e}")),
    };
    tracing::info!(%addr, "gameshelf listening");
    if let Err(e) = axum::serve(listener, app.into_make_service())
        .with_graceful_shutdown(shutdown())
        .await
    {
        tracing::error!(error = %e, "server stopped with an error");
    }
}

async fn server_fn_handler(State(state): State<AppState>, req: Request<Body>) -> impl IntoResponse {
    handle_server_fns_with_context(move || provide_context(state.clone()), req).await
}

/// Hourly cleanup of expired sessions and old sync runs.
fn spawn_housekeeping(store: Store) {
    tokio::spawn(async move {
        let sessions = SqliteSessionStore::new(store.clone());
        let mut tick = tokio::time::interval(Duration::from_secs(3600));
        loop {
            tick.tick().await;
            if let Err(e) = sessions.purge_expired().await {
                tracing::warn!(error = %e, "session purge failed");
            }
            if let Err(e) = store.purge_old_runs().await {
                tracing::warn!(error = %e, "run purge failed");
            }
        }
    });
}

/// Hourly check for agents that stopped reporting.
fn spawn_agent_watch(state: AppState) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(3600));
        loop {
            tick.tick().await;
            gameshelf_app::server::notifications::check_silent_agents(&state).await;
        }
    });
}

fn fatal(msg: &str) -> ! {
    eprintln!("gameshelf: {msg}");
    std::process::exit(2);
}

fn init_tracing(cfg: &config::Config) {
    let filter = EnvFilter::try_new(&cfg.log).unwrap_or_else(|_| EnvFilter::new("info"));
    let builder = fmt().with_env_filter(filter);
    match cfg.log_format {
        config::LogFormat::Json => builder.json().init(),
        config::LogFormat::Pretty => builder.init(),
    }
}

async fn shutdown() {
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("shutting down");
}
