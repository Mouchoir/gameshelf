//! `battlenet-local`: games installed through the Battle.net app, from the Windows uninstall entries, with the
//! last-played dates kept in `Battle.net.config`. The saved account name in that file is an e-mail address and
//! is never read; games are reported for the machine.

use crate::{
    LocalError, LocalSource, ScanOptions, env_path, finish, installed_item,
    registry::{UninstallEntry, uninstall_entries},
    slug, unix,
};
use gameshelf_core::agent::{Report, ReportItem};
use std::{collections::BTreeMap, path::PathBuf};

/// Locale suffixes the app appends to product uids (`wow_enus`).
const LOCALES: &[&str] = &[
    "enus", "engb", "frfr", "dede", "eses", "esmx", "itit", "kokr", "ptbr", "ptpt", "ruru", "zhcn", "zhtw",
    "plpl", "jajp", "thth", "trtr",
];

/// Product uids of games whose title is not in an uninstall entry.
const TITLES: &[(&str, &str)] = &[
    ("wow", "World of Warcraft"),
    ("wow_classic", "World of Warcraft Classic"),
    ("wow_classic_era", "World of Warcraft Classic Era"),
    ("diablo3", "Diablo III"),
    ("fenris", "Diablo IV"),
    ("osi", "Diablo II: Resurrected"),
    ("hs_beta", "Hearthstone"),
    ("heroes", "Heroes of the Storm"),
    ("prometheus", "Overwatch 2"),
    ("s2", "StarCraft II"),
    ("s1", "StarCraft: Remastered"),
    ("w3", "Warcraft III: Reforged"),
];

/// Entries of the config that are not games.
const NOT_GAMES: &[&str] = &["battle_net", "agent", "bna"];

#[derive(Clone, Debug, Default)]
pub struct BattleNet {
    entries: Option<Vec<UninstallEntry>>,
    config: Option<PathBuf>,
}

impl BattleNet {
    /// Uses the given uninstall entries and config file instead of the machine's.
    pub fn with_data(entries: Vec<UninstallEntry>, config: Option<PathBuf>) -> Self {
        Self {
            entries: Some(entries),
            config,
        }
    }

    fn config_path(&self) -> Option<PathBuf> {
        if self.entries.is_some() {
            return self.config.clone();
        }
        env_path("APPDATA").map(|a| a.join("Battle.net").join("Battle.net.config"))
    }

    fn entries(&self) -> Vec<UninstallEntry> {
        self.entries.clone().unwrap_or_else(uninstall_entries)
    }
}

/// Whether an uninstall entry is a game installed by the Battle.net app.
fn is_battlenet_game(e: &UninstallEntry) -> bool {
    let by_app = e
        .uninstall_string
        .as_deref()
        .is_some_and(|u| u.contains("Battle.net") && u.contains("--uid="));
    (by_app || e.publisher_contains("Blizzard Entertainment"))
        && !e.display_name.eq_ignore_ascii_case("Battle.net")
}

/// The product uid without its locale suffix.
pub fn product_uid(raw: &str) -> String {
    let lower = raw.trim().to_lowercase();
    match lower.rsplit_once('_') {
        Some((base, suffix)) if LOCALES.contains(&suffix) => base.to_string(),
        _ => lower,
    }
}

fn uid_of(e: &UninstallEntry) -> Option<String> {
    let s = e.uninstall_string.as_deref()?;
    let rest = &s[s.find("--uid=")? + 6..];
    let uid: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    (!uid.is_empty()).then(|| product_uid(&uid))
}

#[async_trait::async_trait]
impl LocalSource for BattleNet {
    fn id(&self) -> &'static str {
        "battlenet-local"
    }

    fn detect(&self, _opts: &ScanOptions) -> bool {
        self.config_path().is_some_and(|c| c.is_file()) || self.entries().iter().any(is_battlenet_game)
    }

    async fn scan(&self, opts: &ScanOptions) -> Result<Vec<Report>, LocalError> {
        let mut items: BTreeMap<String, ReportItem> = BTreeMap::new();
        for e in self.entries().iter().filter(|e| is_battlenet_game(e)) {
            let id = uid_of(e).unwrap_or_else(|| slug(&e.display_name));
            items.entry(id.clone()).or_insert_with(|| {
                installed_item(
                    id,
                    Some(e.display_name.clone()),
                    e.size_bytes(),
                    e.location(),
                    opts,
                )
            });
        }
        if let Some(path) = self.config_path().filter(|p| p.is_file()) {
            let text = tokio::fs::read_to_string(&path).await?;
            let config: serde_json::Value =
                serde_json::from_str(&text).map_err(|e| LocalError::Format(e.to_string()))?;
            for (raw, game) in config
                .get("Games")
                .and_then(|g| g.as_object())
                .into_iter()
                .flatten()
            {
                let uid = product_uid(raw);
                if NOT_GAMES.contains(&uid.as_str()) {
                    continue;
                }
                let last = game
                    .get("LastPlayed")
                    .and_then(|v| v.as_str().and_then(|s| s.parse().ok()).or_else(|| v.as_u64()))
                    .and_then(unix);
                if let Some(item) = items.get_mut(&uid) {
                    item.last_played_at = item.last_played_at.max(last);
                } else if let Some((_, title)) = TITLES.iter().find(|(u, _)| *u == uid) {
                    items.insert(
                        uid.clone(),
                        ReportItem {
                            platform_game_id: uid,
                            title: Some((*title).to_string()),
                            kind: gameshelf_core::ProductKind::Game,
                            installed: false,
                            size_bytes: None,
                            playtime_minutes: None,
                            last_played_at: last,
                            install_path: None,
                        },
                    );
                }
            }
        }
        if items.is_empty() && !self.detect(opts) {
            return Err(LocalError::NotFound);
        }
        Ok(vec![finish(
            "battlenet-local",
            "battlenet",
            Vec::new(),
            items.into_values().collect(),
        )])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locale_suffixes_are_removed() {
        assert_eq!(product_uid("wow_enus"), "wow");
        assert_eq!(product_uid("wow_classic_era"), "wow_classic_era");
        assert_eq!(product_uid("Prometheus"), "prometheus");
    }
}
