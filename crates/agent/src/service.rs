//! What the agent does: pair, run one cycle (pull settings, scan when due, send reports, heartbeat), loop,
//! unpair.

use crate::{
    autostart,
    client::{ApiError, Client, VERSION, parse_server},
    config::LocalConfig,
    scan,
};
use gameshelf_core::agent::{Heartbeat, PairRequest};
use gameshelf_local::LocalSource;
use std::{path::PathBuf, time::Duration};
use time::OffsetDateTime;

/// How often the agent pulls its settings, so pauses and changes made in the web app apply quickly.
pub const PULL_EVERY: Duration = Duration::from_secs(15 * 60);
/// Longest time between heartbeats.
const HEARTBEAT_EVERY: time::Duration = time::Duration::hours(6);

#[derive(Debug, thiserror::Error)]
pub enum CycleError {
    #[error("this agent is not paired")]
    NotPaired,
    #[error("this agent was removed from the server")]
    Revoked,
    #[error("the server needs agent version {0} or newer")]
    Upgrade(String),
    #[error("{0}")]
    Other(String),
}

impl From<String> for CycleError {
    fn from(s: String) -> Self {
        CycleError::Other(s)
    }
}

/// What one cycle did.
#[derive(Debug, Default)]
pub struct CycleReport {
    /// The server could not be reached; cached settings were used.
    pub offline: bool,
    pub paused: bool,
    pub scanned: bool,
    /// `(source, platform, result)` for each report sent.
    pub sent: Vec<(String, String, String)>,
    pub failed: Vec<(String, String, String)>,
    pub source_errors: Vec<(String, String)>,
}

pub struct Agent {
    dir: PathBuf,
    sources: Vec<Box<dyn LocalSource>>,
}

impl std::fmt::Debug for Agent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Agent")
            .field("dir", &self.dir)
            .finish_non_exhaustive()
    }
}

/// The operating system name the server expects.
pub fn os() -> Option<&'static str> {
    match std::env::consts::OS {
        "windows" => Some("windows"),
        "linux" => Some("linux"),
        _ => None,
    }
}

/// This computer's name, offered as the agent name.
pub fn hostname() -> String {
    std::env::var("COMPUTERNAME")
        .ok()
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok())
        .or_else(|| std::fs::read_to_string("/proc/sys/kernel/hostname").ok())
        .map(|h| h.trim().to_string())
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "computer".to_string())
}

impl Agent {
    pub fn new(dir: PathBuf, sources: Vec<Box<dyn LocalSource>>) -> Self {
        Self { dir, sources }
    }

    pub fn sources(&self) -> &[Box<dyn LocalSource>] {
        &self.sources
    }

    pub fn load(&self) -> Result<LocalConfig, String> {
        LocalConfig::load(&self.dir)
    }

    pub fn save(&self, cfg: &LocalConfig) -> Result<(), String> {
        cfg.save(&self.dir)
    }

    /// Pairs with a server using a code from its Agents page.
    pub async fn pair(&self, server: &str, code: &str, name: &str) -> Result<LocalConfig, String> {
        let os = os().ok_or("this operating system is not supported")?;
        let (server, _) = parse_server(server)?;
        let client = Client::new(&server)?;
        let resp = client
            .pair(&PairRequest {
                code: code.trim().to_string(),
                name: name.trim().to_string(),
                os: os.to_string(),
                version: VERSION.to_string(),
            })
            .await
            .map_err(|e| e.to_string())?;
        let mut cfg = self.load()?;
        cfg.server_url = Some(server);
        cfg.agent_id = Some(resp.agent_id.to_string());
        cfg.name = Some(name.trim().to_string());
        cfg.token = Some(resp.token);
        cfg.server_config = Some(resp.config);
        cfg.last_scan_at = None;
        cfg.last_heartbeat_at = None;
        self.save(&cfg)?;
        Ok(cfg)
    }

    fn revoked(&self, cfg: &mut LocalConfig) -> CycleError {
        cfg.forget_pairing();
        if let Err(e) = self.save(cfg) {
            tracing::warn!(error = %e, "cannot forget the pairing");
        }
        CycleError::Revoked
    }

    /// One cycle: pull settings, scan when due (or when `force_scan`), send reports, heartbeat.
    pub async fn cycle(&self, force_scan: bool) -> Result<CycleReport, CycleError> {
        let mut cfg = self.load()?;
        let (Some(server), Some(token)) = (cfg.server_url.clone(), cfg.token.clone()) else {
            return Err(CycleError::NotPaired);
        };
        let client = Client::new(&server)?;
        let mut out = CycleReport::default();
        match client.config(&token).await {
            Ok(c) => cfg.server_config = Some(c),
            Err(ApiError::Revoked) => return Err(self.revoked(&mut cfg)),
            Err(ApiError::Upgrade(v)) => return Err(CycleError::Upgrade(v)),
            Err(e) => {
                tracing::warn!(error = %e, "cannot pull settings, using the last ones");
                out.offline = true;
            }
        }
        let now = OffsetDateTime::now_utc();
        out.paused = cfg.is_paused();
        let sources = cfg.effective_sources();
        let mut states = if !out.paused && !out.offline && (force_scan || cfg.scan_due(now)) {
            let scan = scan::run(&self.sources, &sources, cfg.server().send_install_paths).await;
            for r in &scan.reports {
                match client.report(&token, r).await {
                    Ok(result) => out.sent.push((r.source.clone(), r.platform.clone(), result)),
                    Err(ApiError::Revoked) => return Err(self.revoked(&mut cfg)),
                    Err(ApiError::Upgrade(v)) => return Err(CycleError::Upgrade(v)),
                    Err(e) => out
                        .failed
                        .push((r.source.clone(), r.platform.clone(), e.to_string())),
                }
            }
            out.scanned = true;
            out.source_errors = scan.errors;
            if out.failed.is_empty() {
                cfg.last_scan_at = Some(now);
            }
            scan.states
        } else {
            scan::detect(&self.sources, &sources)
        };
        for s in states.values_mut() {
            if s.last_scan_at.is_none() && s.enabled && s.detected {
                s.last_scan_at = cfg.last_scan_at;
            }
        }
        let heartbeat_due = cfg.last_heartbeat_at.is_none_or(|t| now - t >= HEARTBEAT_EVERY);
        if !out.offline && (out.scanned || heartbeat_due) {
            let hb = Heartbeat {
                version: VERSION.to_string(),
                autostart: autostart::is_enabled(),
                paused: cfg.paused,
                sources: states,
            };
            match client.heartbeat(&token, &hb).await {
                Ok(()) => cfg.last_heartbeat_at = Some(now),
                Err(ApiError::Revoked) => return Err(self.revoked(&mut cfg)),
                Err(e) => tracing::warn!(error = %e, "heartbeat failed"),
            }
        }
        self.save(&cfg)?;
        Ok(out)
    }

    /// Runs cycles until stopped. Stops for good when the agent is removed, too old or not paired.
    pub async fn run_forever(&self) -> Result<(), CycleError> {
        loop {
            match self.cycle(false).await {
                Ok(r) => tracing::info!(
                    scanned = r.scanned,
                    sent = r.sent.len(),
                    failed = r.failed.len(),
                    paused = r.paused,
                    offline = r.offline,
                    "cycle done"
                ),
                Err(e @ (CycleError::Revoked | CycleError::Upgrade(_) | CycleError::NotPaired)) => {
                    return Err(e);
                }
                Err(e) => tracing::warn!(error = %e, "cycle failed"),
            }
            tokio::select! {
                _ = tokio::time::sleep(PULL_EVERY) => {}
                _ = tokio::signal::ctrl_c() => return Ok(()),
            }
        }
    }

    /// Unpairs on the server when it can be reached, and forgets the pairing locally in every case.
    pub async fn unpair(&self) -> Result<bool, String> {
        let mut cfg = self.load()?;
        let reached = match (cfg.server_url.clone(), cfg.token.clone()) {
            (Some(server), Some(token)) => match Client::new(&server)?.unpair(&token).await {
                Ok(()) | Err(ApiError::Revoked) => true,
                Err(e) => {
                    tracing::warn!(error = %e, "cannot reach the server to unpair");
                    false
                }
            },
            _ => false,
        };
        cfg.forget_pairing();
        cfg.server_url = None;
        self.save(&cfg)?;
        Ok(reached)
    }
}
