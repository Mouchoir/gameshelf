//! The agent's local state: server address, token, local choices and the last configuration pulled.
//!
//! It lives in the user's configuration folder (`%APPDATA%\gameshelf-agent` or `~/.config/gameshelf-agent`),
//! readable only by the user on Linux. `GAMESHELF_AGENT_DIR` overrides the folder.

use gameshelf_core::agent::{AgentConfig, SourceConfig};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};
use time::OffsetDateTime;

const FILE: &str = "agent.json";

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LocalConfig {
    pub server_url: Option<String>,
    pub agent_id: Option<String>,
    pub name: Option<String>,
    pub token: Option<String>,
    /// Paused on this computer: nothing is sent.
    #[serde(default)]
    pub paused: bool,
    /// Sources turned off on this computer, on top of the web app's choices.
    #[serde(default)]
    pub disabled_sources: BTreeSet<String>,
    /// The configuration last pulled from the server.
    pub server_config: Option<AgentConfig>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub last_scan_at: Option<OffsetDateTime>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub last_heartbeat_at: Option<OffsetDateTime>,
}

/// The configuration folder.
pub fn dir() -> PathBuf {
    if let Some(d) = std::env::var_os("GAMESHELF_AGENT_DIR").filter(|d| !d.is_empty()) {
        return PathBuf::from(d);
    }
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .filter(|d| !d.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
    };
    base.unwrap_or_else(|| PathBuf::from(".")).join("gameshelf-agent")
}

impl LocalConfig {
    pub fn load(dir: &Path) -> Result<Self, String> {
        match std::fs::read(dir.join(FILE)) {
            Ok(bytes) => {
                serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", dir.join(FILE).display()))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(format!("{}: {e}", dir.join(FILE).display())),
        }
    }

    /// Writes atomically, readable by the user only on Linux.
    pub fn save(&self, dir: &Path) -> Result<(), String> {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let tmp = dir.join(format!("{FILE}.tmp"));
        let bytes = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(&tmp, bytes).map_err(|e| format!("{}: {e}", tmp.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))
                .map_err(|e| format!("{}: {e}", tmp.display()))?;
        }
        std::fs::rename(&tmp, dir.join(FILE)).map_err(|e| format!("{}: {e}", dir.join(FILE).display()))
    }

    pub fn paired(&self) -> bool {
        self.server_url.is_some() && self.token.is_some()
    }

    /// Forgets the pairing but keeps local choices.
    pub fn forget_pairing(&mut self) {
        self.token = None;
        self.agent_id = None;
        self.server_config = None;
    }

    /// The server's configuration, or the defaults before the first pull.
    pub fn server(&self) -> AgentConfig {
        self.server_config.clone().unwrap_or_default()
    }

    /// Sources as the server set them, minus those turned off on this computer.
    pub fn effective_sources(&self) -> BTreeMap<String, SourceConfig> {
        let mut sources = self.server().sources;
        for (id, s) in &mut sources {
            if self.disabled_sources.contains(id) {
                s.enabled = false;
            }
        }
        sources
    }

    /// Whether this computer sends nothing right now.
    pub fn is_paused(&self) -> bool {
        self.paused || self.server().status == "paused"
    }

    /// Whether a scan is due.
    pub fn scan_due(&self, now: OffsetDateTime) -> bool {
        let every = time::Duration::hours(i64::from(self.server().scan_hours.max(1)));
        self.last_scan_at.is_none_or(|t| now - t >= every)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_effective_sources() {
        let dir = tempfile::tempdir().expect("tmp");
        let mut c = LocalConfig {
            server_url: Some("https://games.example.com".into()),
            token: Some("t".into()),
            ..Default::default()
        };
        c.disabled_sources.insert("steam-local".into());
        c.save(dir.path()).expect("save");
        let back = LocalConfig::load(dir.path()).expect("load");
        assert!(back.paired());
        assert!(!back.effective_sources()["steam-local"].enabled);
        assert!(back.effective_sources()["epic-local"].enabled);
        assert!(
            LocalConfig::load(&dir.path().join("missing"))
                .expect("empty")
                .server_url
                .is_none()
        );
    }

    #[test]
    fn scans_follow_the_server_interval() {
        let now = OffsetDateTime::now_utc();
        let mut c = LocalConfig::default();
        assert!(c.scan_due(now));
        c.last_scan_at = Some(now - time::Duration::hours(5));
        assert!(!c.scan_due(now), "default interval is 6 hours");
        c.last_scan_at = Some(now - time::Duration::hours(7));
        assert!(c.scan_due(now));
        c.server_config = Some(AgentConfig {
            status: "paused".into(),
            ..AgentConfig::default()
        });
        assert!(c.is_paused());
    }
}
