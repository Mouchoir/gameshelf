//! Readers for the data launchers keep on the player's machine. The agent runs them and sends the resulting
//! reports to the server (contracts/agent-api.md). Readers only read: they never write to a launcher's files,
//! and they never collect credentials, login names or e-mail addresses.

pub mod appinfo;
pub mod battlenet;
pub mod epic;
pub mod gog;
pub mod launchers;
pub mod registry;
pub mod steam;
pub mod ubisoft;
pub mod vdf;

use gameshelf_core::agent::{Report, ReportAccount, ReportItem};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use time::OffsetDateTime;

#[derive(Debug, thiserror::Error)]
pub enum LocalError {
    #[error("launcher data not found")]
    NotFound,
    #[error("unreadable launcher file: {0}")]
    Format(String),
    #[error("file error: {0}")]
    Io(#[from] std::io::Error),
    #[error("database error: {0}")]
    Database(String),
}

/// What the player allowed for this scan.
#[derive(Clone, Debug, Default)]
pub struct ScanOptions {
    /// Include install folders in reports.
    pub send_install_paths: bool,
    /// Extra folders the player pointed the source to.
    pub extra_paths: Vec<PathBuf>,
}

/// One local source (`steam-local`, `epic-local`, ...).
#[async_trait::async_trait]
pub trait LocalSource: Send + Sync {
    /// The source id, one of `gameshelf_core::agent::SOURCES`.
    fn id(&self) -> &'static str;
    /// Whether the launcher's data exists on this machine.
    fn detect(&self, opts: &ScanOptions) -> bool;
    /// Reads everything the source can see. One report per platform.
    async fn scan(&self, opts: &ScanOptions) -> Result<Vec<Report>, LocalError>;
}

/// Every source, reading the default locations of the current operating system.
pub fn builtin() -> Vec<Box<dyn LocalSource>> {
    vec![
        Box::new(steam::Steam::default()),
        Box::new(epic::Epic::default()),
        Box::new(gog::GogGalaxy::default()),
        Box::new(battlenet::BattleNet::default()),
        Box::new(ubisoft::Ubisoft::default()),
        Box::new(launchers::Launchers::default()),
    ]
}

/// Builds a report with a hash of its content, so the server can skip unchanged ones. Items are sorted first so
/// the hash does not depend on directory listing order.
pub fn finish(
    source: &str,
    platform: &str,
    mut accounts: Vec<ReportAccount>,
    mut machine_items: Vec<ReportItem>,
) -> Report {
    accounts.sort_by(|a, b| a.external_account_id.cmp(&b.external_account_id));
    for a in &mut accounts {
        a.items
            .sort_by(|x, y| x.platform_game_id.cmp(&y.platform_game_id));
    }
    machine_items.sort_by(|x, y| x.platform_game_id.cmp(&y.platform_game_id));
    let mut hasher = Sha256::new();
    hasher.update(source.as_bytes());
    hasher.update(platform.as_bytes());
    hasher.update(serde_json::to_vec(&accounts).unwrap_or_default());
    hasher.update(serde_json::to_vec(&machine_items).unwrap_or_default());
    let hex: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
    Report {
        source: source.to_string(),
        platform: platform.to_string(),
        generated_at: OffsetDateTime::now_utc(),
        content_hash: format!("sha256:{hex}"),
        accounts,
        machine_items,
    }
}

/// A Unix timestamp in seconds; zero and out-of-range values mean "never".
pub(crate) fn unix(secs: u64) -> Option<OffsetDateTime> {
    if secs == 0 {
        return None;
    }
    OffsetDateTime::from_unix_timestamp(i64::try_from(secs).ok()?).ok()
}

/// A stable id built from a title, for launchers that do not expose one.
pub fn slug(title: &str) -> String {
    let mut out = String::new();
    for c in title.chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_string()
}

/// A plain item for a game found installed on this machine.
pub(crate) fn installed_item(
    id: String,
    title: Option<String>,
    size_bytes: Option<u64>,
    path: Option<&Path>,
    opts: &ScanOptions,
) -> ReportItem {
    ReportItem {
        platform_game_id: id,
        title,
        kind: gameshelf_core::ProductKind::Game,
        installed: true,
        size_bytes: size_bytes.filter(|s| *s > 0),
        playtime_minutes: None,
        last_played_at: None,
        install_path: path
            .filter(|_| opts.send_install_paths)
            .map(|p| p.to_string_lossy().into_owned()),
    }
}

/// An environment variable as a path, when set and not empty.
pub(crate) fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// The user's home folder.
pub(crate) fn home() -> Option<PathBuf> {
    env_path("HOME").or_else(|| env_path("USERPROFILE"))
}

/// Keeps existing folders, without duplicates.
pub(crate) fn existing(paths: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
    let mut seen = std::collections::HashSet::new();
    paths
        .into_iter()
        .filter(|p| p.exists())
        .filter(|p| seen.insert(std::fs::canonicalize(p).unwrap_or_else(|_| p.clone())))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_are_stable_and_readable() {
        assert_eq!(slug("The Bazaar"), "the-bazaar");
        assert_eq!(slug("  Final Fantasy XIV: Online  "), "final-fantasy-xiv-online");
        assert_eq!(slug("Élan & Co."), "élan-co");
    }

    #[test]
    fn hash_ignores_order_and_time() {
        let item = |id: &str| installed_item(id.into(), Some(id.into()), None, None, &ScanOptions::default());
        let a = finish("steam-local", "steam", vec![], vec![item("1"), item("2")]);
        let b = finish("steam-local", "steam", vec![], vec![item("2"), item("1")]);
        assert_eq!(a.content_hash, b.content_hash);
        let c = finish("steam-local", "steam", vec![], vec![item("1")]);
        assert_ne!(a.content_hash, c.content_hash);
    }

    #[test]
    fn install_paths_need_consent() {
        let path = Path::new("/games/example");
        let off = installed_item("1".into(), None, Some(0), Some(path), &ScanOptions::default());
        assert_eq!(off.install_path, None);
        assert_eq!(off.size_bytes, None);
        let on = installed_item(
            "1".into(),
            None,
            None,
            Some(path),
            &ScanOptions {
                send_install_paths: true,
                ..Default::default()
            },
        );
        assert!(on.install_path.is_some());
    }

    #[test]
    fn every_builtin_source_is_known_to_the_protocol() {
        for s in builtin() {
            assert!(
                gameshelf_core::agent::SOURCES.iter().any(|(id, _)| *id == s.id()),
                "{}",
                s.id()
            );
        }
    }
}
