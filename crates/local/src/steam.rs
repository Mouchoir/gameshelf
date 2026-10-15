//! `steam-local`: installed games from the library manifests, and per-account playtime from each account's
//! `localconfig.vdf`. Names and app types come from Steam's app cache (`appcache/appinfo.vdf`) when it is
//! readable, so the server rarely has to look them up. Login names in `loginusers.vdf` are never read; only the
//! public persona name is.

use crate::{LocalError, LocalSource, ScanOptions, appinfo, existing, finish, home, unix, vdf};
use gameshelf_core::{
    ProductKind,
    agent::{Report, ReportAccount, ReportItem},
};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    path::{Path, PathBuf},
};

/// SteamID64 of account id 0 (individual accounts, public universe).
const STEAMID64_BASE: u64 = 76_561_197_960_265_728;

/// Runtimes and compatibility tools Steam installs alongside games.
const RUNTIME_APPS: &[u64] = &[
    228_980, 1_070_560, 1_391_110, 1_628_350, 1_493_710, 2_180_100, 1_826_330, 1_161_040,
];
const RUNTIME_PREFIXES: &[&str] = &["Proton ", "Steam Linux Runtime", "Steamworks Common"];
/// Software that is not a game but is worth listing.
const TOOL_APPS: &[u64] = &[250_820];
/// App types in Steam's app cache that are not worth listing: demos, playtests (`beta`), tools and client
/// configuration.
const SKIPPED_TYPES: &[&str] = &["demo", "beta", "tool", "config"];

#[derive(Clone, Debug, Default)]
pub struct Steam {
    roots: Option<Vec<PathBuf>>,
}

impl Steam {
    /// Reads the given Steam folders instead of the default locations.
    pub fn with_roots(roots: Vec<PathBuf>) -> Self {
        Self { roots: Some(roots) }
    }

    fn roots(&self, opts: &ScanOptions) -> Vec<PathBuf> {
        let base = self.roots.clone().unwrap_or_else(default_roots);
        existing(base.into_iter().chain(opts.extra_paths.iter().cloned()))
            .into_iter()
            .filter(|r| r.join("steamapps").is_dir() || r.join("userdata").is_dir())
            .collect()
    }
}

fn default_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    #[cfg(windows)]
    {
        if let Some(p) = crate::registry::current_user_string(r"Software\Valve\Steam", "SteamPath") {
            roots.push(PathBuf::from(p));
        }
        if let Some(pf) = crate::env_path("ProgramFiles(x86)") {
            roots.push(pf.join("Steam"));
        }
    }
    if let Some(h) = home() {
        roots.push(h.join(".steam").join("steam"));
        roots.push(h.join(".local").join("share").join("Steam"));
        roots.push(h.join(".var/app/com.valvesoftware.Steam/.local/share/Steam"));
    }
    roots
}

#[async_trait::async_trait]
impl LocalSource for Steam {
    fn id(&self) -> &'static str {
        "steam-local"
    }

    fn detect(&self, opts: &ScanOptions) -> bool {
        !self.roots(opts).is_empty()
    }

    async fn scan(&self, opts: &ScanOptions) -> Result<Vec<Report>, LocalError> {
        let roots = self.roots(opts);
        if roots.is_empty() {
            return Err(LocalError::NotFound);
        }
        let opts = opts.clone();
        tokio::task::spawn_blocking(move || read(&roots, &opts))
            .await
            .map_err(|e| LocalError::Format(e.to_string()))?
    }
}

#[derive(Default)]
struct Account {
    name: Option<String>,
    items: BTreeMap<String, ReportItem>,
}

fn read(roots: &[PathBuf], opts: &ScanOptions) -> Result<Vec<Report>, LocalError> {
    let mut accounts: BTreeMap<String, Account> = BTreeMap::new();
    let mut machine: BTreeMap<String, ReportItem> = BTreeMap::new();
    for root in roots {
        read_users(root, &mut accounts);
        read_playtime(root, &mut accounts);
        for manifest in libraries(root).iter().flat_map(|lib| manifests(lib)) {
            let Some(item) = manifest.item(opts) else {
                continue;
            };
            let target = match manifest.owner {
                Some(owner) => &mut accounts.entry(owner).or_default().items,
                None => &mut machine,
            };
            merge(target, item);
        }
    }
    let ids = accounts
        .values()
        .flat_map(|a| a.items.keys())
        .chain(machine.keys())
        .filter_map(|id| id.parse().ok())
        .collect();
    let cache = app_cache(roots, ids);
    for items in accounts
        .values_mut()
        .map(|a| &mut a.items)
        .chain(std::iter::once(&mut machine))
    {
        items.retain(|_, item| {
            let app = item.platform_game_id.parse().ok().and_then(|id| cache.get(&id));
            app.is_none_or(|app| describe(item, app))
        });
    }
    let accounts = accounts
        .into_iter()
        .filter(|(_, a)| !a.items.is_empty())
        .map(|(id, a)| ReportAccount {
            external_account_id: id,
            display_name: a.name,
            items: a.items.into_values().collect(),
        })
        .collect();
    Ok(vec![finish(
        "steam-local",
        "steam",
        accounts,
        machine.into_values().collect(),
    )])
}

/// Adds or completes an item: manifests bring the title and install state, `localconfig.vdf` the playtime.
fn merge(items: &mut BTreeMap<String, ReportItem>, new: ReportItem) {
    match items.get_mut(&new.platform_game_id) {
        None => {
            items.insert(new.platform_game_id.clone(), new);
        }
        Some(old) => {
            old.title = old.title.take().or(new.title);
            old.installed |= new.installed;
            old.size_bytes = old.size_bytes.or(new.size_bytes);
            old.install_path = old.install_path.take().or(new.install_path);
            old.playtime_minutes = old.playtime_minutes.max(new.playtime_minutes);
            old.last_played_at = old.last_played_at.max(new.last_played_at);
            if new.kind != ProductKind::Game {
                old.kind = new.kind;
            }
        }
    }
}

/// Names and app types of the given apps from each Steam folder's app cache. A missing, unknown or damaged cache
/// leaves the apps it does not describe untitled, for the server to name.
fn app_cache(roots: &[PathBuf], mut ids: BTreeSet<u64>) -> HashMap<u64, appinfo::App> {
    let mut found = HashMap::new();
    for root in roots {
        if ids.is_empty() {
            break;
        }
        let path = root.join("appcache").join("appinfo.vdf");
        if !path.is_file() {
            continue;
        }
        match appinfo::read(&path, &ids) {
            Ok(apps) => {
                ids.retain(|id| !apps.contains_key(id));
                found.extend(apps);
            }
            Err(e) => {
                tracing::warn!(file = %path.display(), error = %e, "skipping the Steam app cache");
            }
        }
    }
    found
}

/// Completes an item from the app cache: the cached name when the item has none, and the kind from the app
/// type. `false` when the app is not worth listing (a demo, a tool, a runtime).
fn describe(item: &mut ReportItem, app: &appinfo::App) -> bool {
    if app.name.as_deref().is_some_and(is_runtime) {
        return false;
    }
    let tool = item
        .platform_game_id
        .parse()
        .is_ok_and(|id: u64| TOOL_APPS.contains(&id));
    if !tool {
        match app.app_type.as_deref() {
            None => {}
            Some("game") => item.kind = ProductKind::Game,
            Some("dlc" | "music") => item.kind = ProductKind::Dlc,
            Some("application") => item.kind = ProductKind::Tool,
            Some(t) if SKIPPED_TYPES.contains(&t) => return false,
            Some(_) => item.kind = ProductKind::Unknown,
        }
    }
    if item.title.is_none() {
        item.title = app.name.clone();
    }
    true
}

fn is_runtime(name: &str) -> bool {
    RUNTIME_PREFIXES.iter().any(|p| name.starts_with(p))
}

fn read_vdf(path: &Path) -> Option<vdf::Obj> {
    let text = std::fs::read_to_string(path).ok()?;
    match vdf::parse(&text) {
        Ok(o) => Some(o),
        Err(e) => {
            tracing::warn!(file = %path.display(), error = %e, "skipping an unreadable Steam file");
            None
        }
    }
}

fn steam_id(text: &str) -> Option<String> {
    let id: u64 = text.trim().parse().ok()?;
    (id > STEAMID64_BASE).then(|| id.to_string())
}

fn read_users(root: &Path, accounts: &mut BTreeMap<String, Account>) {
    let Some(file) = read_vdf(&root.join("config").join("loginusers.vdf")) else {
        return;
    };
    for (id, user) in file.obj("users").into_iter().flat_map(vdf::Obj::objects) {
        if let Some(id) = steam_id(id) {
            let name = user.str("PersonaName").map(str::trim).filter(|n| !n.is_empty());
            accounts.entry(id).or_default().name = name.map(str::to_string);
        }
    }
}

fn read_playtime(root: &Path, accounts: &mut BTreeMap<String, Account>) {
    let Ok(dir) = std::fs::read_dir(root.join("userdata")) else {
        return;
    };
    for entry in dir.flatten() {
        let Some(account_id) = entry.file_name().to_str().and_then(|n| n.parse::<u64>().ok()) else {
            continue;
        };
        if account_id == 0 {
            continue;
        }
        let Some(config) = read_vdf(&entry.path().join("config").join("localconfig.vdf")) else {
            continue;
        };
        let Some(apps) = config
            .path(&["UserLocalConfigStore", "Software", "Valve", "Steam"])
            .and_then(|s| s.obj("apps"))
        else {
            continue;
        };
        let account = accounts
            .entry((account_id + STEAMID64_BASE).to_string())
            .or_default();
        for (app_id, app) in apps.objects() {
            let Ok(id) = app_id.parse::<u64>() else {
                continue;
            };
            let playtime = app.num("Playtime").filter(|m| *m > 0);
            let last = app.num("LastPlayed").and_then(unix);
            if RUNTIME_APPS.contains(&id) || (playtime.is_none() && last.is_none()) {
                continue;
            }
            merge(
                &mut account.items,
                ReportItem {
                    platform_game_id: id.to_string(),
                    title: None,
                    kind: kind_of(id),
                    installed: false,
                    size_bytes: None,
                    playtime_minutes: playtime.and_then(|m| u32::try_from(m).ok()),
                    last_played_at: last,
                    install_path: None,
                },
            );
        }
    }
}

fn kind_of(app_id: u64) -> ProductKind {
    if TOOL_APPS.contains(&app_id) {
        ProductKind::Tool
    } else {
        ProductKind::Game
    }
}

/// Library folders: the Steam folder itself plus those listed in `libraryfolders.vdf`.
fn libraries(root: &Path) -> Vec<PathBuf> {
    let mut libs = vec![root.to_path_buf()];
    for file in [
        root.join("steamapps").join("libraryfolders.vdf"),
        root.join("config").join("libraryfolders.vdf"),
    ] {
        let Some(folders) = read_vdf(&file) else {
            continue;
        };
        let Some(list) = folders.obj("libraryfolders") else {
            continue;
        };
        for (key, value) in &list.0 {
            if key.parse::<u32>().is_err() {
                continue;
            }
            let path = match value {
                vdf::Value::Obj(o) => o.str("path").map(str::to_string),
                vdf::Value::Str(s) => Some(s.clone()),
            };
            libs.extend(path.map(PathBuf::from));
        }
    }
    existing(libs)
}

struct Manifest {
    app_id: u64,
    name: Option<String>,
    installed: bool,
    size: Option<u64>,
    owner: Option<String>,
    last_played: Option<time::OffsetDateTime>,
    path: Option<PathBuf>,
}

impl Manifest {
    fn item(&self, opts: &ScanOptions) -> Option<ReportItem> {
        if RUNTIME_APPS.contains(&self.app_id) || self.name.as_deref().is_some_and(is_runtime) {
            return None;
        }
        Some(ReportItem {
            platform_game_id: self.app_id.to_string(),
            title: self.name.clone(),
            kind: kind_of(self.app_id),
            installed: self.installed,
            size_bytes: self.size.filter(|s| *s > 0),
            playtime_minutes: None,
            last_played_at: self.last_played,
            install_path: self
                .path
                .as_ref()
                .filter(|_| opts.send_install_paths)
                .map(|p| p.to_string_lossy().into_owned()),
        })
    }
}

fn manifests(library: &Path) -> Vec<Manifest> {
    let apps = library.join("steamapps");
    let Ok(dir) = std::fs::read_dir(&apps) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in dir.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if !(name.starts_with("appmanifest_") && name.ends_with(".acf")) {
            continue;
        }
        let Some(file) = read_vdf(&entry.path()) else {
            continue;
        };
        let Some(state) = file.obj("AppState") else {
            continue;
        };
        let Some(app_id) = state.num("appid") else {
            continue;
        };
        out.push(Manifest {
            app_id,
            name: state
                .str("name")
                .map(str::trim)
                .filter(|n| !n.is_empty())
                .map(str::to_string),
            // StateFlags bit 4: fully installed.
            installed: state.num("StateFlags").is_some_and(|f| f & 4 != 0),
            size: state.num("SizeOnDisk"),
            owner: state.str("LastOwner").and_then(steam_id),
            last_played: state.num("LastPlayed").and_then(unix),
            path: state
                .str("installdir")
                .filter(|d| !d.is_empty())
                .map(|d| apps.join("common").join(d)),
        });
    }
    out
}
