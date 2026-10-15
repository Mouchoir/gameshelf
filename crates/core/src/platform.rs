//! The platform registry (FR-002, research sections 1 and 2).
//!
//! Brand names are proper nouns and are not translated; every other label shown next to them goes through
//! the localization catalog.

use serde::{Deserialize, Serialize};

/// How reliably gameshelf can read a platform (Constitution, Principle V).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    /// Public, documented API.
    Official,
    /// API used by the platform's own client or by maintained open-source clients.
    Unofficial,
    /// No usable remote source: the agent reads local launcher data.
    Agent,
    /// Ownership is visible on another platform.
    ViaOther,
    /// No automatic source.
    Manual,
}

impl Tier {
    pub fn as_str(self) -> &'static str {
        match self {
            Tier::Official => "official",
            Tier::Unofficial => "unofficial",
            Tier::Agent => "agent",
            Tier::ViaOther => "via_other",
            Tier::Manual => "manual",
        }
    }
}

/// What a platform can provide.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Capabilities {
    pub library: bool,
    pub playtime: bool,
    pub last_played: bool,
    pub achievements: bool,
    pub enrichment: bool,
    pub family_sharing: bool,
}

impl Capabilities {
    const fn new(library: bool, playtime: bool, last_played: bool, achievements: bool) -> Self {
        Self {
            library,
            playtime,
            last_played,
            achievements,
            enrichment: false,
            family_sharing: false,
        }
    }

    const fn with_enrichment(mut self) -> Self {
        self.enrichment = true;
        self
    }

    const fn with_family(mut self) -> Self {
        self.family_sharing = true;
        self
    }
}

/// One entry of the registry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct PlatformInfo {
    /// Stable key stored in the database.
    pub key: &'static str,
    /// Brand name.
    pub name: &'static str,
    pub tier: Tier,
    pub capabilities: Capabilities,
    /// Icon file name under `/icons/`.
    pub icon: &'static str,
    /// A server-side connector reads it remotely.
    pub remote: bool,
    /// The agent can read it locally.
    pub local: bool,
    /// Kept in the registry so manual entries can use it, but no connector exists yet.
    pub console: bool,
}

const LIB: Capabilities = Capabilities::new(true, false, false, false);
const LIB_TIME: Capabilities = Capabilities::new(true, true, true, false);
const FULL: Capabilities = Capabilities::new(true, true, true, true);

const fn remote(
    key: &'static str,
    name: &'static str,
    tier: Tier,
    caps: Capabilities,
    local: bool,
) -> PlatformInfo {
    PlatformInfo {
        key,
        name,
        tier,
        capabilities: caps,
        icon: key,
        remote: true,
        local,
        console: false,
    }
}

const fn agent(key: &'static str, name: &'static str, caps: Capabilities) -> PlatformInfo {
    PlatformInfo {
        key,
        name,
        tier: Tier::Agent,
        capabilities: caps,
        icon: key,
        remote: false,
        local: true,
        console: false,
    }
}

const fn manual(key: &'static str, name: &'static str, console: bool) -> PlatformInfo {
    PlatformInfo {
        key,
        name,
        tier: Tier::Manual,
        capabilities: LIB,
        icon: key,
        remote: false,
        local: false,
        console,
    }
}

/// Every known platform, in display order.
pub const PLATFORMS: &[PlatformInfo] = &[
    remote(
        "steam",
        "Steam",
        Tier::Official,
        FULL.with_enrichment().with_family(),
        true,
    ),
    remote("epic", "Epic Games Store", Tier::Unofficial, FULL, true),
    remote("gog", "GOG", Tier::Unofficial, FULL, true),
    remote(
        "xbox",
        "Xbox (PC) and Microsoft Store",
        Tier::Unofficial,
        FULL,
        true,
    ),
    PlatformInfo {
        remote: true,
        ..agent("battlenet", "Battle.net", LIB_TIME.with_enrichment())
    },
    PlatformInfo {
        remote: true,
        ..agent("ubisoft", "Ubisoft Connect", LIB_TIME)
    },
    remote("ea", "EA app", Tier::Unofficial, FULL, true),
    remote("amazon", "Amazon Games", Tier::Unofficial, LIB, true),
    remote("itch", "itch.io", Tier::Official, LIB, true),
    remote(
        "humble",
        "Humble Bundle",
        Tier::Unofficial,
        LIB.with_enrichment(),
        false,
    ),
    remote("indiegala", "IndieGala", Tier::Unofficial, LIB, false),
    remote("gamejolt", "Game Jolt", Tier::Unofficial, LIB, false),
    remote("zoom", "ZOOM Platform", Tier::Unofficial, LIB, false),
    agent("legacygames", "Legacy Games", LIB),
    agent("metapc", "Meta Quest Link", LIB),
    agent("viveport", "Viveport", LIB),
    agent("rockstar", "Rockstar Games Launcher", LIB),
    PlatformInfo {
        capabilities: LIB.with_enrichment(),
        ..agent("riot", "Riot Client", LIB)
    },
    remote("minecraft", "Minecraft Launcher", Tier::Unofficial, LIB, true),
    agent("roblox", "Roblox", LIB),
    PlatformInfo {
        capabilities: LIB.with_enrichment(),
        ..agent("hoyoplay", "HoYoPlay", LIB)
    },
    agent("kuro", "Kuro Games", LIB),
    agent("gryphline", "Gryphline", LIB),
    agent("levelinfinite", "Level Infinite and Garena", LIB),
    PlatformInfo {
        capabilities: LIB.with_enrichment(),
        ..agent("wargaming", "Wargaming Game Center", LIB)
    },
    agent("gaijin", "Gaijin.Net", LIB),
    PlatformInfo {
        capabilities: LIB.with_enrichment(),
        ..agent("jagex", "Jagex Launcher", LIB)
    },
    agent("squareenix", "Square Enix", LIB),
    agent("nexon", "Nexon", LIB),
    agent("ncsoft", "NCSoft PURPLE", LIB),
    agent("stove", "Smilegate STOVE", LIB),
    agent("plarium", "Plarium Play", LIB),
    agent("ankama", "Ankama Launcher", LIB),
    agent("gameforge", "Gameforge", LIB),
    agent("arc", "Arc", LIB),
    agent("tempo", "Tempo Launcher", LIB),
    agent("battlestate", "Battlestate Games", LIB),
    agent("rsi", "RSI Launcher", LIB),
    agent("hytale", "Hytale Launcher", LIB),
    PlatformInfo {
        capabilities: LIB.with_enrichment(),
        ..agent("arenanet", "ArenaNet", LIB)
    },
    agent("ggg", "Grinding Gear Games", LIB),
    PlatformInfo {
        capabilities: LIB.with_enrichment(),
        ..agent("ccp", "CCP", LIB)
    },
    agent("frontier", "Frontier", LIB),
    agent("pearlabyss", "Pearl Abyss", LIB),
    agent("albion", "Albion Online", LIB),
    agent("warframe", "Digital Extremes", LIB),
    manual("playstation", "PlayStation", true),
    manual("switch", "Nintendo Switch", true),
    manual("manual", "Other", false),
];

/// Looks up a platform by key.
pub fn platform(key: &str) -> Option<&'static PlatformInfo> {
    PLATFORMS.iter().find(|p| p.key == key)
}

/// Whether a key exists in the registry.
pub fn is_known(key: &str) -> bool {
    platform(key).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn keys_are_unique_and_lowercase() {
        let mut seen = HashSet::new();
        for p in PLATFORMS {
            assert!(seen.insert(p.key), "duplicate key {}", p.key);
            assert!(
                p.key
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()),
                "{}",
                p.key
            );
        }
    }

    #[test]
    fn spec_platform_coverage_is_present() {
        for key in [
            "steam",
            "epic",
            "gog",
            "xbox",
            "battlenet",
            "ubisoft",
            "ea",
            "amazon",
            "itch",
            "humble",
        ] {
            assert!(is_known(key), "missing {key}");
        }
    }

    #[test]
    fn agent_platforms_are_local() {
        for p in PLATFORMS.iter().filter(|p| p.tier == Tier::Agent) {
            assert!(p.local, "{} is agent tier but not local", p.key);
        }
    }
}
