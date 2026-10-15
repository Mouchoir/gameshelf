//! gameshelf server entry point.

mod api;
mod config;

use axum::{Router, routing::get};
use gameshelf_app::{App, shell};
use leptos::prelude::*;
use leptos_axum::{LeptosRoutes, generate_route_list};
use tracing_subscriber::{EnvFilter, fmt};

#[tokio::main]
async fn main() {
    let cfg = match config::Config::load() {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("gameshelf: {e}");
            std::process::exit(2);
        }
    };
    init_tracing(&cfg);

    let conf = match get_configuration(None) {
        Ok(conf) => conf,
        Err(e) => {
            eprintln!("gameshelf: cannot read the web configuration: {e}");
            std::process::exit(2);
        }
    };
    let leptos_options = conf.leptos_options;
    let addr = cfg.bind.unwrap_or(leptos_options.site_addr);
    let routes = generate_route_list(App);

    let app = Router::new()
        .route("/healthz", get(api::health::healthz))
        .route("/readyz", get(api::health::readyz))
        .leptos_routes(&leptos_options, routes, {
            let leptos_options = leptos_options.clone();
            move || shell(leptos_options.clone())
        })
        .fallback(leptos_axum::file_and_error_handler(shell))
        .with_state(leptos_options);

    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("gameshelf: cannot listen on {addr}: {e}");
            std::process::exit(2);
        }
    };
    tracing::info!(%addr, "gameshelf listening");
    if let Err(e) = axum::serve(listener, app.into_make_service())
        .with_graceful_shutdown(shutdown())
        .await
    {
        tracing::error!(error = %e, "server stopped with an error");
    }
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
