//! Wire format of the agent API (contracts/agent-api.md), shared by the server and the agent. Dates are
//! RFC 3339 strings.

use crate::{AgentId, ProductKind};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use time::OffsetDateTime;

/// Oldest agent version the server accepts.
pub const MIN_AGENT_VERSION: &str = "0.1.0";
/// Default scan interval, in hours.
pub const DEFAULT_SCAN_HOURS: u32 = 6;
/// Maximum accepted report body, in bytes.
pub const MAX_REPORT_BYTES: usize = 5 * 1024 * 1024;

/// Local sources an agent can read, with the platform each one reports for.
pub const SOURCES: &[(&str, &str)] = &[
    ("steam-local", "steam"),
    ("epic-local", "epic"),
    ("gog-galaxy-local", "gog"),
    ("battlenet-local", "battlenet"),
    ("ubisoft-local", "ubisoft"),
    ("launchers-local", "*"),
];

/// Platform of a single-platform source.
pub fn source_platform(source: &str) -> Option<&'static str> {
    SOURCES
        .iter()
        .find(|(s, _)| *s == source)
        .map(|(_, p)| *p)
        .filter(|p| *p != "*")
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairRequest {
    pub code: String,
    pub name: String,
    pub os: String,
    pub version: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairResponse {
    pub agent_id: AgentId,
    pub token: String,
    pub config: AgentConfig,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceConfig {
    pub enabled: bool,
    /// Extra folders to look in, set by the player when detection misses a launcher.
    #[serde(default)]
    pub paths: Vec<String>,
}

/// What the server asks the agent to do (pulled by the agent).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentConfig {
    /// `active`, `paused` or `revoked`.
    pub status: String,
    pub scan_hours: u32,
    pub send_install_paths: bool,
    pub sources: BTreeMap<String, SourceConfig>,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            status: "active".into(),
            scan_hours: DEFAULT_SCAN_HOURS,
            send_install_paths: false,
            sources: SOURCES
                .iter()
                .map(|(s, _)| {
                    (
                        s.to_string(),
                        SourceConfig {
                            enabled: true,
                            paths: Vec::new(),
                        },
                    )
                })
                .collect(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceState {
    pub enabled: bool,
    pub detected: bool,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub last_scan_at: Option<OffsetDateTime>,
}

/// What the agent reports about itself.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Heartbeat {
    pub version: String,
    pub autostart: bool,
    pub paused: bool,
    pub sources: BTreeMap<String, SourceState>,
}

/// One game seen locally.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportItem {
    pub platform_game_id: String,
    /// Missing when the launcher's files do not hold the name; the server resolves it.
    pub title: Option<String>,
    #[serde(default = "default_kind")]
    pub kind: ProductKind,
    pub installed: bool,
    pub size_bytes: Option<u64>,
    pub playtime_minutes: Option<u32>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub last_played_at: Option<OffsetDateTime>,
    /// Only sent when the player allows it.
    pub install_path: Option<String>,
}

fn default_kind() -> ProductKind {
    ProductKind::Game
}

/// Games of one account as stored on this machine.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportAccount {
    pub external_account_id: String,
    pub display_name: Option<String>,
    pub items: Vec<ReportItem>,
}

/// A full snapshot of one source for one platform. Agent reports never remove games: launchers often only
/// know installed games, and uninstalling is not losing ownership.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
    pub source: String,
    pub platform: String,
    #[serde(with = "time::serde::rfc3339")]
    pub generated_at: OffsetDateTime,
    pub content_hash: String,
    pub accounts: Vec<ReportAccount>,
    /// Games the launcher does not attach to an account.
    pub machine_items: Vec<ReportItem>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportResult {
    /// `applied` or `unchanged`.
    pub result: String,
}

/// Compares dotted versions numerically (`0.10.0` is newer than `0.9.1`).
pub fn version_at_least(version: &str, min: &str) -> bool {
    let parse = |v: &str| -> Vec<u64> { v.split('.').map(|p| p.trim().parse().unwrap_or(0)).collect() };
    let (a, b) = (parse(version), parse(min));
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (a.get(i).copied().unwrap_or(0), b.get(i).copied().unwrap_or(0));
        if x != y {
            return x > y;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_numerically() {
        assert!(version_at_least("0.1.0", "0.1.0"));
        assert!(version_at_least("0.10.0", "0.9.1"));
        assert!(!version_at_least("0.0.9", "0.1.0"));
        assert!(version_at_least("1", "0.9"));
    }

    #[test]
    fn sources_map_to_platforms() {
        assert_eq!(source_platform("steam-local"), Some("steam"));
        assert_eq!(source_platform("launchers-local"), None);
        assert_eq!(source_platform("nope"), None);
        for (_, p) in SOURCES.iter().filter(|(_, p)| *p != "*") {
            assert!(crate::platform::is_known(p), "{p}");
        }
    }

    #[test]
    fn dates_travel_as_rfc3339() {
        let item: ReportItem = serde_json::from_str(
            r#"{"platform_game_id":"1","title":null,"installed":true,"size_bytes":null,
                "playtime_minutes":null,"last_played_at":"2026-10-14T21:12:00Z"}"#,
        )
        .expect("parse");
        assert_eq!(item.kind, ProductKind::Game);
        let json = serde_json::to_string(&item).expect("json");
        assert!(json.contains("\"2026-10-14T21:12:00Z\""), "{json}");
        let missing: SourceState =
            serde_json::from_str(r#"{"enabled":true,"detected":false}"#).expect("parse");
        assert_eq!(missing.last_scan_at, None);
    }

    #[test]
    fn default_config_enables_every_source() {
        let c = AgentConfig::default();
        assert_eq!(c.sources.len(), SOURCES.len());
        assert!(c.sources.values().all(|s| s.enabled));
        assert!(!c.send_install_paths);
    }
}
