//! `epic-local`: games installed by the Epic Games Launcher (Windows manifests) or by Legendary and Heroic
//! (Linux). Neither keeps a usable account id, so items are reported for the machine.

use crate::{LocalError, LocalSource, ScanOptions, env_path, existing, finish, home, installed_item};
use gameshelf_core::{
    ProductKind,
    agent::{Report, ReportItem},
};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Default)]
pub struct Epic {
    dirs: Option<Vec<PathBuf>>,
}

impl Epic {
    /// Reads the given folders (launcher `Manifests` folders or Legendary configuration folders).
    pub fn with_dirs(dirs: Vec<PathBuf>) -> Self {
        Self { dirs: Some(dirs) }
    }

    fn dirs(&self, opts: &ScanOptions) -> Vec<PathBuf> {
        let base = self.dirs.clone().unwrap_or_else(default_dirs);
        existing(base.into_iter().chain(opts.extra_paths.iter().cloned()))
    }
}

fn default_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(pd) = env_path("ProgramData") {
        dirs.push(
            pd.join("Epic")
                .join("EpicGamesLauncher")
                .join("Data")
                .join("Manifests"),
        );
    }
    let config = env_path("XDG_CONFIG_HOME").or_else(|| home().map(|h| h.join(".config")));
    if let Some(c) = config {
        dirs.push(c.join("legendary"));
        dirs.push(c.join("heroic").join("legendaryConfig").join("legendary"));
    }
    if let Some(h) = home() {
        dirs.push(h.join(".var/app/com.heroicgameslauncher.hgl/config/heroic/legendaryConfig/legendary"));
    }
    dirs
}

#[async_trait::async_trait]
impl LocalSource for Epic {
    fn id(&self) -> &'static str {
        "epic-local"
    }

    fn detect(&self, opts: &ScanOptions) -> bool {
        !self.dirs(opts).is_empty()
    }

    async fn scan(&self, opts: &ScanOptions) -> Result<Vec<Report>, LocalError> {
        let dirs = self.dirs(opts);
        if dirs.is_empty() {
            return Err(LocalError::NotFound);
        }
        let opts = opts.clone();
        tokio::task::spawn_blocking(move || {
            let mut items: BTreeMap<String, ReportItem> = BTreeMap::new();
            for dir in &dirs {
                let found = if dir.join("installed.json").is_file() {
                    legendary(dir, &opts)
                } else {
                    launcher_manifests(dir, &opts)
                };
                for item in found {
                    items.entry(item.platform_game_id.clone()).or_insert(item);
                }
            }
            Ok(vec![finish(
                "epic-local",
                "epic",
                Vec::new(),
                items.into_values().collect(),
            )])
        })
        .await
        .map_err(|e| LocalError::Format(e.to_string()))?
    }
}

#[derive(Deserialize)]
struct LauncherItem {
    #[serde(rename = "DisplayName")]
    display_name: Option<String>,
    #[serde(rename = "CatalogNamespace")]
    namespace: Option<String>,
    #[serde(rename = "CatalogItemId")]
    item_id: Option<String>,
    #[serde(rename = "AppName", default)]
    app_name: String,
    #[serde(rename = "InstallLocation")]
    install_location: Option<String>,
    #[serde(rename = "InstallSize")]
    install_size: Option<u64>,
    #[serde(rename = "bIsIncompleteInstall", default)]
    incomplete: bool,
    #[serde(rename = "MainGameCatalogItemId")]
    main_item_id: Option<String>,
    #[serde(rename = "AppCategories", default)]
    categories: Vec<String>,
}

fn launcher_manifests(dir: &Path, opts: &ScanOptions) -> Vec<ReportItem> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("item") {
            continue;
        }
        let parsed = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<LauncherItem>(&bytes).ok());
        let Some(m) = parsed else {
            tracing::warn!(file = %path.display(), "skipping an unreadable Epic manifest");
            continue;
        };
        let (Some(ns), Some(id)) = (m.namespace.as_deref(), m.item_id.as_deref()) else {
            continue;
        };
        if m.categories.iter().any(|c| c == "engines") || m.app_name.starts_with("UE_") {
            continue;
        }
        let dlc = m.categories.iter().any(|c| c == "addons")
            || m.main_item_id
                .as_deref()
                .is_some_and(|main| !main.is_empty() && main != id);
        let mut item = installed_item(
            format!("{ns}:{id}"),
            m.display_name.filter(|t| !t.trim().is_empty()),
            m.install_size,
            m.install_location.as_deref().map(Path::new),
            opts,
        );
        item.installed = !m.incomplete;
        if dlc {
            item.kind = ProductKind::Dlc;
        }
        out.push(item);
    }
    out
}

#[derive(Deserialize)]
struct LegendaryInstalled {
    title: Option<String>,
    install_path: Option<String>,
    install_size: Option<u64>,
    #[serde(default)]
    is_dlc: bool,
}

#[derive(Deserialize)]
struct LegendaryMetadata {
    metadata: LegendaryCatalog,
}

#[derive(Deserialize)]
struct LegendaryCatalog {
    id: String,
    namespace: String,
}

fn legendary(dir: &Path, opts: &ScanOptions) -> Vec<ReportItem> {
    let installed: HashMap<String, LegendaryInstalled> = std::fs::read(dir.join("installed.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    let mut out = Vec::new();
    for (app_name, game) in installed {
        // The catalog ids live in the per-game metadata cache; without them the game cannot be matched.
        let meta = std::fs::read(dir.join("metadata").join(format!("{app_name}.json")))
            .ok()
            .and_then(|b| serde_json::from_slice::<LegendaryMetadata>(&b).ok());
        let Some(meta) = meta else {
            tracing::debug!(app = %app_name, "no Legendary metadata for an installed game");
            continue;
        };
        let mut item = installed_item(
            format!("{}:{}", meta.metadata.namespace, meta.metadata.id),
            game.title.filter(|t| !t.trim().is_empty()),
            game.install_size,
            game.install_path.as_deref().map(Path::new),
            opts,
        );
        if game.is_dlc {
            item.kind = ProductKind::Dlc;
        }
        out.push(item);
    }
    out
}
