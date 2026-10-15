//! `launchers-local`: games of the many launchers that keep no readable library, recognized from the Windows
//! uninstall entries. Steam, GOG, Ubisoft and Battle.net entries are left to their own sources. Each platform
//! found becomes its own report; games are reported for the machine.

use crate::{
    LocalError, LocalSource, ScanOptions, finish, installed_item,
    registry::{UninstallEntry, uninstall_entries},
    slug,
};
use gameshelf_core::agent::{Report, ReportItem};
use std::collections::BTreeMap;

enum Match {
    /// Publisher contains this text (case-insensitive).
    Publisher(&'static str),
    /// Display name equals this text (case-insensitive).
    Name(&'static str),
    /// Registry key starts with this text; the rest of the key is the game id.
    KeyPrefix(&'static str),
}

/// Games recognized by publisher, name or key. The first matching rule wins.
const RULES: &[(&str, Match)] = &[
    ("riot", Match::KeyPrefix("Riot Game ")),
    ("rockstar", Match::Publisher("Rockstar Games")),
    ("levelinfinite", Match::Publisher("Level Infinite")),
    ("levelinfinite", Match::Publisher("Garena")),
    ("levelinfinite", Match::Name("Delta Force")),
    ("hoyoplay", Match::Publisher("miHoYo")),
    ("hoyoplay", Match::Publisher("COGNOSPHERE")),
    ("hoyoplay", Match::Publisher("HoYoverse")),
    ("wargaming", Match::Publisher("Wargaming")),
    ("jagex", Match::Publisher("Jagex")),
    ("arenanet", Match::Publisher("ArenaNet")),
    ("arenanet", Match::Name("Guild Wars 2")),
    ("ggg", Match::Publisher("Grinding Gear Games")),
    ("ccp", Match::Publisher("CCP ehf")),
    ("ccp", Match::Name("EVE Online")),
    ("battlestate", Match::Publisher("Battlestate Games")),
    ("gaijin", Match::Publisher("Gaijin")),
    ("warframe", Match::Publisher("Digital Extremes")),
    ("albion", Match::Name("Albion Online")),
    ("pearlabyss", Match::Publisher("Pearl Abyss")),
    ("frontier", Match::Publisher("Frontier Developments")),
    ("squareenix", Match::Publisher("Square Enix")),
    ("nexon", Match::Publisher("Nexon")),
    ("ncsoft", Match::Publisher("NCSOFT")),
    ("stove", Match::Publisher("Smilegate")),
    ("plarium", Match::Publisher("Plarium")),
    ("ankama", Match::Publisher("Ankama")),
    ("gameforge", Match::Publisher("Gameforge")),
    ("kuro", Match::Publisher("Kuro Games")),
    ("gryphline", Match::Publisher("Gryphline")),
    ("ea", Match::Publisher("Electronic Arts")),
    ("legacygames", Match::Publisher("Legacy Games")),
];

/// Launchers that exist for a single game: finding the launcher means the game is installed.
const SINGLE_GAME_LAUNCHERS: &[(&str, &str, &str)] = &[
    ("Tempo Launcher", "tempo", "The Bazaar"),
    ("RSI Launcher", "rsi", "Star Citizen"),
    ("Roblox Player", "roblox", "Roblox"),
];

/// Words that mark a launcher or a support program rather than a game.
const NOT_A_GAME: &[&str] = &[
    "launcher",
    "game center",
    "gamecenter",
    "social club",
    "anti-cheat",
    "anticheat",
    "easyanticheat",
    "battleye",
    "redistributable",
    "runtime",
    "updater",
    "uninstall",
    "client",
    "hoyoplay",
    "ea app",
    "origin",
    "toolkit",
    "sdk",
];

/// Entries owned by other sources or by stores whose games are read elsewhere.
fn handled_elsewhere(e: &UninstallEntry) -> bool {
    let location = e.install_location.as_deref().unwrap_or_default().to_lowercase();
    e.key.starts_with("Steam App ")
        || e.key.starts_with("Uplay Install ")
        || e.publisher_contains("GOG.com")
        || e.publisher_contains("Blizzard Entertainment")
        || e.uninstall_string
            .as_deref()
            .is_some_and(|u| u.contains("Battle.net"))
        || location.contains("steamapps")
        || location.contains("epic games")
}

/// Recognizes games in uninstall entries, grouped by platform.
pub fn classify(entries: &[UninstallEntry], opts: &ScanOptions) -> BTreeMap<&'static str, Vec<ReportItem>> {
    let mut found: BTreeMap<&'static str, BTreeMap<String, ReportItem>> = BTreeMap::new();
    for e in entries.iter().filter(|e| !handled_elsewhere(e)) {
        let name = e.display_name.trim();
        if let Some((_, platform, title)) = SINGLE_GAME_LAUNCHERS
            .iter()
            .find(|(launcher, _, _)| name.to_lowercase().starts_with(&launcher.to_lowercase()))
        {
            found
                .entry(platform)
                .or_default()
                .entry(slug(title))
                .or_insert_with(|| {
                    installed_item(
                        slug(title),
                        Some((*title).to_string()),
                        e.size_bytes(),
                        e.location(),
                        opts,
                    )
                });
            continue;
        }
        let lower = name.to_lowercase();
        let Some((platform, rule)) = RULES.iter().find(|(_, rule)| match rule {
            Match::Publisher(p) => e.publisher_contains(p),
            Match::Name(n) => name.eq_ignore_ascii_case(n),
            Match::KeyPrefix(k) => e.key.starts_with(k),
        }) else {
            continue;
        };
        let id = match rule {
            Match::KeyPrefix(k) => e.key[k.len()..].trim().trim_end_matches(".live").to_string(),
            _ => {
                if NOT_A_GAME.iter().any(|w| lower.contains(w)) {
                    continue;
                }
                slug(name)
            }
        };
        if id.is_empty() {
            continue;
        }
        found
            .entry(platform)
            .or_default()
            .entry(id.clone())
            .or_insert_with(|| {
                installed_item(id, Some(name.to_string()), e.size_bytes(), e.location(), opts)
            });
    }
    found
        .into_iter()
        .map(|(p, items)| (p, items.into_values().collect()))
        .collect()
}

#[derive(Clone, Debug, Default)]
pub struct Launchers {
    entries: Option<Vec<UninstallEntry>>,
}

impl Launchers {
    pub fn with_entries(entries: Vec<UninstallEntry>) -> Self {
        Self {
            entries: Some(entries),
        }
    }

    fn entries(&self) -> Vec<UninstallEntry> {
        self.entries.clone().unwrap_or_else(uninstall_entries)
    }
}

#[async_trait::async_trait]
impl LocalSource for Launchers {
    fn id(&self) -> &'static str {
        "launchers-local"
    }

    fn detect(&self, opts: &ScanOptions) -> bool {
        !classify(&self.entries(), opts).is_empty()
    }

    async fn scan(&self, opts: &ScanOptions) -> Result<Vec<Report>, LocalError> {
        let found = classify(&self.entries(), opts);
        if found.is_empty() {
            return Err(LocalError::NotFound);
        }
        Ok(found
            .into_iter()
            .map(|(platform, items)| finish("launchers-local", platform, Vec::new(), items))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(key: &str, name: &str, publisher: Option<&str>) -> UninstallEntry {
        UninstallEntry {
            key: key.into(),
            display_name: name.into(),
            publisher: publisher.map(str::to_string),
            install_location: Some(r"C:\Games\Example".into()),
            size_kb: Some(2048),
            ..Default::default()
        }
    }

    #[test]
    fn rule_platforms_are_known() {
        for (p, _) in RULES {
            assert!(gameshelf_core::platform::is_known(p), "{p}");
        }
        for (_, p, _) in SINGLE_GAME_LAUNCHERS {
            assert!(gameshelf_core::platform::is_known(p), "{p}");
        }
    }

    #[test]
    fn games_are_recognized_and_launchers_skipped() {
        let entries = vec![
            entry("Riot Game valorant.live", "VALORANT", Some("Riot Games, Inc")),
            entry("Riot Client", "Riot Client", Some("Riot Games, Inc")),
            entry("{GUID-1}", "Rockstar Games Launcher", Some("Rockstar Games")),
            entry("{GUID-2}", "Grand Theft Auto V", Some("Rockstar Games")),
            entry("Tempo Launcher - Beta", "Tempo Launcher - Beta", Some("Tempo")),
            entry("{GUID-3}", "Delta Force", Some("Example Studio")),
            entry("Steam App 271590", "Grand Theft Auto V", Some("Rockstar North")),
            entry("Uplay Install 635", "Example Ubisoft Game", Some("Ubisoft")),
            entry("{GUID-4}", "Some Office Suite", Some("Example Corp")),
        ];
        let found = classify(&entries, &ScanOptions::default());
        let ids = |p: &str| -> Vec<String> {
            found
                .get(p)
                .map(|v| v.iter().map(|i| i.platform_game_id.clone()).collect())
                .unwrap_or_default()
        };
        assert_eq!(ids("riot"), vec!["valorant"]);
        assert_eq!(ids("rockstar"), vec!["grand-theft-auto-v"]);
        assert_eq!(ids("tempo"), vec!["the-bazaar"]);
        assert_eq!(ids("levelinfinite"), vec!["delta-force"]);
        assert_eq!(found.len(), 4, "{found:?}");
        let gta = &found["rockstar"][0];
        assert_eq!(gta.size_bytes, Some(2048 * 1024));
        assert_eq!(gta.install_path, None, "paths need consent");
    }
}
