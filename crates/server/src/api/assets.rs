//! `GET /assets/{sha256}`: cached artwork, for signed-in profiles only.

use axum::{
    extract::{Path, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use gameshelf_app::server::AppState;
use gameshelf_store::session_store::PROFILE_KEY;
use tower_sessions::Session;

pub async fn asset(State(state): State<AppState>, session: Session, Path(sha256): Path<String>) -> Response {
    let signed_in = session.get::<String>(PROFILE_KEY).await.ok().flatten().is_some();
    if !signed_in {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if sha256.len() != 64 || !sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let mime = match state.store.asset_mime(&sha256).await {
        Ok(Some(m)) => m,
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    match tokio::fs::read(state.assets_dir.join(&sha256)).await {
        Ok(bytes) => (
            [
                (header::CONTENT_TYPE, mime),
                (
                    header::CACHE_CONTROL,
                    "private, max-age=31536000, immutable".to_string(),
                ),
            ],
            bytes,
        )
            .into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}
