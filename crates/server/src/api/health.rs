//! `GET /healthz` and `GET /readyz`.

use axum::http::StatusCode;

/// The process is running.
pub async fn healthz() -> (StatusCode, &'static str) {
    (StatusCode::OK, "ok")
}

/// The database is reachable and migrations ran. Wired to the store in Phase 2.
pub async fn readyz() -> (StatusCode, &'static str) {
    (StatusCode::OK, "ready")
}
