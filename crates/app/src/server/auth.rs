//! Session helpers for server functions (research D4).

use super::AppState;
use gameshelf_core::ProfileId;
use gameshelf_store::{Profile, session_store::PROFILE_KEY};
use leptos::prelude::*;
use tower_sessions::Session;

pub fn state() -> Result<AppState, ServerFnError> {
    use_context::<AppState>().ok_or_else(|| ServerFnError::new("server state missing"))
}

pub async fn session() -> Result<Session, ServerFnError> {
    leptos_axum::extract::<Session>()
        .await
        .map_err(|_| ServerFnError::new("session unavailable"))
}

/// The signed-in profile, if any.
pub async fn current_profile() -> Result<Option<Profile>, ServerFnError> {
    let session = session().await?;
    let id: Option<String> = session
        .get(PROFILE_KEY)
        .await
        .map_err(|_| ServerFnError::new("session unavailable"))?;
    let Some(id) = id.and_then(|s| s.parse::<ProfileId>().ok()) else {
        return Ok(None);
    };
    let profile = state()?
        .store
        .get_profile(id)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    Ok(profile.filter(|p| !p.disabled))
}

/// The signed-in profile, or an error that the client treats as "sign in again".
pub async fn require_profile() -> Result<Profile, ServerFnError> {
    match current_profile().await? {
        Some(p) => Ok(p),
        None => {
            if let Some(resp) = use_context::<leptos_axum::ResponseOptions>() {
                resp.set_status(axum::http::StatusCode::UNAUTHORIZED);
            }
            Err(ServerFnError::new("unauthorized"))
        }
    }
}

/// Signs a profile in: a fresh session id prevents fixation.
pub async fn sign_in(profile: &Profile) -> Result<(), ServerFnError> {
    let session = session().await?;
    session
        .cycle_id()
        .await
        .map_err(|_| ServerFnError::new("session unavailable"))?;
    session
        .insert(PROFILE_KEY, profile.id.to_string())
        .await
        .map_err(|_| ServerFnError::new("session unavailable"))?;
    Ok(())
}

pub async fn sign_out() -> Result<(), ServerFnError> {
    session()
        .await?
        .flush()
        .await
        .map_err(|_| ServerFnError::new("session unavailable"))
}

/// Client address for rate limiting.
pub async fn client_key() -> String {
    let headers = leptos_axum::extract::<axum::http::HeaderMap>()
        .await
        .unwrap_or_default();
    headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .map(|v| v.trim().to_string())
        .unwrap_or_else(|| "direct".to_string())
}
