//! `GET /healthz` and `GET /readyz`.

use axum::{extract::State, http::StatusCode};
use gameshelf_app::server::AppState;

/// The process is running.
pub async fn healthz() -> (StatusCode, &'static str) {
    (StatusCode::OK, "ok")
}

/// The database is reachable and migrations ran.
pub async fn readyz(State(state): State<AppState>) -> (StatusCode, &'static str) {
    if state.store.ready().await {
        (StatusCode::OK, "ready")
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, "database unavailable")
    }
}
