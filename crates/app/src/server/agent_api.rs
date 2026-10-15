//! The agent API, mounted at `/api/agent/v1` (contracts/agent-api.md).
//!
//! Every call but pairing carries a bearer token. Errors use `{"error": {"code", "message"}}`. Reports are
//! checked synchronously and applied in the background, so a slow lookup never holds the agent's request.

use super::AppState;
use axum::{
    Json, Router,
    extract::{FromRequestParts, Path, State, rejection::JsonRejection},
    http::{HeaderMap, StatusCode, header, request::Parts},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use gameshelf_core::agent::{
    Heartbeat, MAX_REPORT_BYTES, MIN_AGENT_VERSION, PairRequest, PairResponse, Report, ReportResult,
    version_at_least,
};
use gameshelf_store::{Agent, AgentStatus, StoreError};
use gameshelf_sync::{ReportCheck, ReportRejection};

/// Header carrying the agent's version on every call.
pub const VERSION_HEADER: &str = "x-gameshelf-agent-version";

/// Routes relative to `/api/agent/v1`.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/pair", post(pair))
        .route("/config", get(config))
        .route("/heartbeat", post(heartbeat))
        .route("/reports", post(reports))
        .route("/logins/{id}/result", post(login_unknown))
        .route("/logins/{id}/decline", post(login_unknown))
        .route("/self", delete(unpair))
        .layer(axum::extract::DefaultBodyLimit::max(MAX_REPORT_BYTES))
        .layer(tower_http::decompression::RequestDecompressionLayer::new())
}

fn error(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        Json(serde_json::json!({ "error": { "code": code, "message": message } })),
    )
        .into_response()
}

fn internal(e: impl std::fmt::Display) -> Response {
    tracing::error!(error = %e, "agent API failure");
    error(
        StatusCode::INTERNAL_SERVER_ERROR,
        "internal",
        "the server could not complete the request",
    )
}

fn upgrade() -> Response {
    (
        StatusCode::UPGRADE_REQUIRED,
        Json(serde_json::json!({
            "error": { "code": "upgrade_required", "message": "this agent is too old for the server; update it" },
            "min_version": MIN_AGENT_VERSION,
        })),
    )
        .into_response()
}

fn bad_json(e: JsonRejection) -> Response {
    error(StatusCode::BAD_REQUEST, "invalid_request", &e.body_text())
}

/// An authenticated, non-revoked agent.
pub struct AgentAuth(pub Agent);

impl FromRequestParts<AppState> for AgentAuth {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let version = parts
            .headers
            .get(VERSION_HEADER)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .trim()
            .to_string();
        if version.is_empty() || !version_at_least(&version, MIN_AGENT_VERSION) {
            return Err(upgrade());
        }
        let token = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(str::trim)
            .filter(|t| !t.is_empty());
        let unknown = || {
            error(
                StatusCode::UNAUTHORIZED,
                "invalid_token",
                "missing or unknown agent token",
            )
        };
        let Some(token) = token else {
            return Err(unknown());
        };
        let agent = state
            .store
            .agent_by_token(token)
            .await
            .map_err(internal)?
            .ok_or_else(unknown)?;
        if agent.status == AgentStatus::Revoked {
            return Err(error(
                StatusCode::UNAUTHORIZED,
                "agent_revoked",
                "this agent was removed from the server; pair it again to continue",
            ));
        }
        state
            .store
            .touch_agent(agent.id, &version)
            .await
            .map_err(internal)?;
        Ok(AgentAuth(agent))
    }
}

async fn pair(
    State(st): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<PairRequest>, JsonRejection>,
) -> Response {
    if !st.allow_pair_attempt(&super::auth::client_key_from(&headers)) {
        return error(
            StatusCode::TOO_MANY_REQUESTS,
            "too_many_attempts",
            "too many pairing attempts; wait a few minutes",
        );
    }
    let Json(req) = match body {
        Ok(b) => b,
        Err(e) => return bad_json(e),
    };
    if !version_at_least(&req.version, MIN_AGENT_VERSION) {
        return upgrade();
    }
    match st
        .store
        .pair_agent(&req.code, &req.name, &req.os, &req.version)
        .await
    {
        Ok(Some(paired)) => {
            tracing::info!(agent = %paired.agent.id, "agent paired");
            (
                StatusCode::CREATED,
                Json(PairResponse {
                    agent_id: paired.agent.id,
                    config: paired.agent.config(),
                    token: paired.token,
                }),
            )
                .into_response()
        }
        Ok(None) => error(
            StatusCode::BAD_REQUEST,
            "invalid_code",
            "this pairing code is not valid",
        ),
        Err(StoreError::Invalid(msg)) => error(StatusCode::BAD_REQUEST, "invalid_request", &msg),
        Err(e) => internal(e),
    }
}

async fn config(AgentAuth(agent): AgentAuth) -> Response {
    Json(agent.config()).into_response()
}

async fn heartbeat(
    State(st): State<AppState>,
    AgentAuth(agent): AgentAuth,
    body: Result<Json<Heartbeat>, JsonRejection>,
) -> Response {
    let Json(hb) = match body {
        Ok(b) => b,
        Err(e) => return bad_json(e),
    };
    match st.store.record_heartbeat(agent.id, &hb).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => internal(e),
    }
}

async fn reports(
    State(st): State<AppState>,
    AgentAuth(agent): AgentAuth,
    body: Result<Json<Report>, JsonRejection>,
) -> Response {
    if agent.status == AgentStatus::Paused {
        return error(
            StatusCode::CONFLICT,
            "agent_paused",
            "this agent is paused in the web app",
        );
    }
    let Json(report) = match body {
        Ok(b) => b,
        Err(e) => return bad_json(e),
    };
    let accepted = |result: &str| {
        (
            StatusCode::ACCEPTED,
            Json(ReportResult {
                result: result.to_string(),
            }),
        )
            .into_response()
    };
    match st.agents.check(&agent, &report).await {
        Err(e) => internal(e),
        Ok(Err(ReportRejection::Unknown)) => error(
            StatusCode::BAD_REQUEST,
            "unknown_source",
            "unknown source or platform",
        ),
        Ok(Err(ReportRejection::SourceDisabled)) => error(
            StatusCode::CONFLICT,
            "source_disabled",
            "this source is turned off for this agent",
        ),
        Ok(Err(ReportRejection::TooLarge)) => error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "too_large",
            "the report has too many items",
        ),
        Ok(Ok(ReportCheck::Unchanged)) => accepted("unchanged"),
        Ok(Ok(ReportCheck::Accepted)) => {
            tokio::spawn(async move {
                match st.agents.apply(&agent, &report).await {
                    Ok(applied) => {
                        tracing::info!(
                            agent = %agent.id,
                            source = %report.source,
                            items = applied.items,
                            unresolved = applied.unresolved,
                            "agent report applied"
                        );
                        for (account, outcome) in &applied.runs {
                            super::notifications::after_run(&st, *account, outcome).await;
                        }
                        st.rebuild_search().await;
                    }
                    Err(e) => tracing::error!(agent = %agent.id, error = %e, "agent report failed"),
                }
            });
            accepted("applied")
        }
    }
}

/// Logins through the agent are not offered yet, so no request can exist.
async fn login_unknown(AgentAuth(_agent): AgentAuth, Path(_id): Path<String>) -> Response {
    error(StatusCode::NOT_FOUND, "unknown_request", "no such login request")
}

async fn unpair(State(st): State<AppState>, AgentAuth(agent): AgentAuth) -> Response {
    match st
        .store
        .set_agent_status(agent.id, None, AgentStatus::Revoked)
        .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => internal(e),
    }
}
