//! Local sources read synthetic launcher files built in a temporary folder. No real launcher data is used.

use gameshelf_core::{ProductKind, agent::Report};
use gameshelf_local::{
    LocalSource, ScanOptions, appinfo, battlenet::BattleNet, epic::Epic, gog::GogGalaxy,
    registry::UninstallEntry, steam::Steam, ubisoft::Ubisoft,
};
use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};
use std::{collections::BTreeSet, path::Path};

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    std::fs::write(path, text).expect("write");
}

/// A path as written inside a VDF string.
fn vdf_path(p: &Path) -> String {
    p.display().to_string().replace('\\', "\\\\")
}

fn item<'a>(r: &'a Report, account: Option<&str>, id: &str) -> &'a gameshelf_core::agent::ReportItem {
    let items = match account {
        Some(a) => {
            &r.accounts
                .iter()
                .find(|x| x.external_account_id == a)
                .expect("account")
                .items
        }
        None => &r.machine_items,
    };
    items.iter().find(|i| i.platform_game_id == id).expect("item")
}

const PLAYER_ONE: &str = "76561197960266729"; // account id 1001
const PLAYER_TWO: &str = "76561197960266730"; // account id 1002

fn steam_tree(base: &Path) -> std::path::PathBuf {
    let root = base.join("Steam");
    let second = base.join("SecondLibrary");
    write(
        &root.join("steamapps/libraryfolders.vdf"),
        &format!(
            "\"libraryfolders\"\n{{\n\t\"0\"\n\t{{\n\t\t\"path\"\t\t\"{}\"\n\t}}\n\t\"1\"\n\t{{\n\t\t\"path\"\t\t\"{}\"\n\t}}\n}}\n",
            vdf_path(&root),
            vdf_path(&second)
        ),
    );
    write(
        &root.join("steamapps/appmanifest_10.acf"),
        &format!(
            "\"AppState\"\n{{\n\t\"appid\"\t\t\"10\"\n\t\"name\"\t\t\"Lantern Run\"\n\t\"StateFlags\"\t\t\"4\"\n\t\"installdir\"\t\t\"Lantern Run\"\n\t\"SizeOnDisk\"\t\t\"1000\"\n\t\"LastOwner\"\t\t\"{PLAYER_ONE}\"\n\t\"LastPlayed\"\t\t\"1700000000\"\n}}\n"
        ),
    );
    write(
        &root.join("steamapps/appmanifest_228980.acf"),
        "\"AppState\" { \"appid\" \"228980\" \"name\" \"Steamworks Common Redistributables\" \"StateFlags\" \"4\" }",
    );
    write(
        &second.join("steamapps/appmanifest_20.acf"),
        "\"AppState\" { \"appid\" \"20\" \"name\" \"Paper Comets\" \"StateFlags\" \"6\" \"LastOwner\" \"0\" \"installdir\" \"Paper Comets\" }",
    );
    write(
        &root.join("config/loginusers.vdf"),
        &format!(
            "\"users\" {{ \"{PLAYER_ONE}\" {{ \"AccountName\" \"secret-login-name\" \"PersonaName\" \"Player One\" \"MostRecent\" \"1\" }} }}"
        ),
    );
    write(
        &root.join("userdata/1001/config/localconfig.vdf"),
        "\"UserLocalConfigStore\" { \"Software\" { \"valve\" { \"Steam\" { \"apps\" {
            \"10\" { \"Playtime\" \"125\" \"LastPlayed\" \"1700000500\" }
            \"30\" { \"Playtime\" \"60\" }
            \"40\" { \"Playtime\" \"0\" }
            \"228980\" { \"Playtime\" \"3\" } } } } } }",
    );
    write(
        &root.join("userdata/1002/config/localconfig.vdf"),
        "\"UserLocalConfigStore\" { \"Software\" { \"Valve\" { \"Steam\" { \"Apps\" { \"10\" { \"Playtime\" \"5\" } } } } } }",
    );
    root
}

#[tokio::test]
async fn steam_reads_libraries_manifests_and_per_account_playtime() {
    let dir = tempfile::tempdir().expect("tmp");
    let root = steam_tree(dir.path());
    let source = Steam::with_roots(vec![root]);
    assert!(source.detect(&ScanOptions::default()));
    let reports = source.scan(&ScanOptions::default()).await.expect("scan");
    assert_eq!(reports.len(), 1);
    let r = &reports[0];
    assert_eq!((r.source.as_str(), r.platform.as_str()), ("steam-local", "steam"));
    assert!(r.content_hash.starts_with("sha256:"));

    let one = r
        .accounts
        .iter()
        .find(|a| a.external_account_id == PLAYER_ONE)
        .expect("one");
    assert_eq!(one.display_name.as_deref(), Some("Player One"));
    let ids: Vec<&str> = one.items.iter().map(|i| i.platform_game_id.as_str()).collect();
    assert_eq!(ids, vec!["10", "30"], "no zero-playtime apps, no runtimes");

    let lantern = item(r, Some(PLAYER_ONE), "10");
    assert_eq!(lantern.title.as_deref(), Some("Lantern Run"));
    assert!(lantern.installed);
    assert_eq!(lantern.size_bytes, Some(1000));
    assert_eq!(lantern.playtime_minutes, Some(125));
    assert_eq!(
        lantern.last_played_at.map(|t| t.unix_timestamp()),
        Some(1_700_000_500)
    );
    assert_eq!(lantern.install_path, None);
    assert_eq!(
        item(r, Some(PLAYER_ONE), "30").title,
        None,
        "the server resolves missing titles"
    );

    let two = item(r, Some(PLAYER_TWO), "10");
    assert_eq!(two.playtime_minutes, Some(5));
    assert!(!two.installed, "installed by the other account");

    let comets = item(r, None, "20");
    assert!(comets.installed);
    assert_eq!(comets.kind, ProductKind::Game);

    let json = serde_json::to_string(&reports).expect("json");
    assert!(!json.contains("secret-login-name"), "login names are never read");
    assert!(!json.contains("228980"));
}

#[tokio::test]
async fn steam_install_paths_follow_consent_and_hash_is_stable() {
    let dir = tempfile::tempdir().expect("tmp");
    let root = steam_tree(dir.path());
    let source = Steam::with_roots(vec![root]);
    let a = source.scan(&ScanOptions::default()).await.expect("a");
    let b = source.scan(&ScanOptions::default()).await.expect("b");
    assert_eq!(a[0].content_hash, b[0].content_hash);
    let with_paths = source
        .scan(&ScanOptions {
            send_install_paths: true,
            ..Default::default()
        })
        .await
        .expect("paths");
    let lantern = item(&with_paths[0], Some(PLAYER_ONE), "10");
    assert!(
        lantern
            .install_path
            .as_deref()
            .is_some_and(|p| p.ends_with("Lantern Run"))
    );
    assert_ne!(a[0].content_hash, with_paths[0].content_hash);
}

const APPINFO_V27: u32 = 0x0756_4427;
const APPINFO_V28: u32 = 0x0756_4428;
const APPINFO_V29: u32 = 0x0756_4429;

/// A value in Steam's binary KeyValues.
enum Kv {
    Str(&'static str),
    Int(i32),
    U64(u64),
    Map(Vec<(&'static str, Kv)>),
}

/// One app as Steam caches it, with fields around `common` that must be skipped.
fn cached_app(id: u32, name: &'static str, app_type: &'static str) -> (u32, Vec<(&'static str, Kv)>) {
    let fields = vec![
        ("appid", Kv::Int(i32::try_from(id).expect("id"))),
        (
            "extended",
            Kv::Map(vec![
                ("name", Kv::Str("Not The Name")),
                ("developer", Kv::Str("Example Studio")),
            ]),
        ),
        (
            "common",
            Kv::Map(vec![
                ("name", Kv::Str(name)),
                ("oslist", Kv::Str("windows")),
                ("gameid", Kv::U64(u64::from(id))),
                ("type", Kv::Str(app_type)),
            ]),
        ),
        (
            "config",
            Kv::Map(vec![(
                "launch",
                Kv::Map(vec![(
                    "0",
                    Kv::Map(vec![("common", Kv::Map(vec![("name", Kv::Str("Nested Name"))]))]),
                )]),
            )]),
        ),
    ];
    (id, fields)
}

/// A synthetic `appcache/appinfo.vdf`. Version 28 writes key names inline, version 29 as indexes into a string
/// table at the end of the file.
fn appinfo_vdf(magic: u32, apps: Vec<(u32, Vec<(&'static str, Kv)>)>) -> Vec<u8> {
    fn encode(out: &mut Vec<u8>, table: &mut Option<Vec<&'static str>>, pairs: Vec<(&'static str, Kv)>) {
        for (key, value) in pairs {
            out.push(match value {
                Kv::Map(_) => 0x00,
                Kv::Str(_) => 0x01,
                Kv::Int(_) => 0x02,
                Kv::U64(_) => 0x07,
            });
            match table {
                Some(t) => {
                    let index = t.iter().position(|k| *k == key).unwrap_or_else(|| {
                        t.push(key);
                        t.len() - 1
                    });
                    out.extend(u32::try_from(index).expect("index").to_le_bytes());
                }
                None => {
                    out.extend(key.as_bytes());
                    out.push(0);
                }
            }
            match value {
                Kv::Map(inner) => {
                    encode(out, table, inner);
                    out.push(0x08);
                }
                Kv::Str(s) => {
                    out.extend(s.as_bytes());
                    out.push(0);
                }
                Kv::Int(i) => out.extend(i.to_le_bytes()),
                Kv::U64(u) => out.extend(u.to_le_bytes()),
            }
        }
    }
    let mut table = (magic == APPINFO_V29).then(Vec::new);
    let mut out = Vec::new();
    out.extend(magic.to_le_bytes());
    out.extend(1_u32.to_le_bytes()); // universe
    if table.is_some() {
        out.extend([0; 8]); // string table offset, set below
    }
    for (id, fields) in apps {
        // State, update time, access token, text hash, change number, binary hash.
        let mut entry = vec![0_u8; 60];
        encode(&mut entry, &mut table, vec![("appinfo", Kv::Map(fields))]);
        entry.push(0x08);
        out.extend(id.to_le_bytes());
        out.extend(u32::try_from(entry.len()).expect("size").to_le_bytes());
        out.extend(entry);
    }
    out.extend(0_u32.to_le_bytes());
    if let Some(keys) = table {
        let offset = i64::try_from(out.len()).expect("offset").to_le_bytes();
        out[8..16].copy_from_slice(&offset);
        out.extend(u32::try_from(keys.len()).expect("count").to_le_bytes());
        for k in keys {
            out.extend(k.as_bytes());
            out.push(0);
        }
    }
    out
}

fn write_bytes(path: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    std::fs::write(path, bytes).expect("write");
}

#[tokio::test]
async fn steam_names_and_sorts_apps_from_the_app_cache() {
    let dir = tempfile::tempdir().expect("tmp");
    let root = steam_tree(dir.path());
    write(
        &root.join("userdata/1002/config/localconfig.vdf"),
        "\"UserLocalConfigStore\" { \"Software\" { \"Valve\" { \"Steam\" { \"apps\" {
            \"10\" { \"Playtime\" \"5\" }
            \"50\" { \"Playtime\" \"5\" }
            \"60\" { \"Playtime\" \"5\" }
            \"70\" { \"Playtime\" \"5\" }
            \"80\" { \"Playtime\" \"5\" }
            \"90\" { \"Playtime\" \"5\" }
            \"95\" { \"Playtime\" \"5\" }
            \"96\" { \"Playtime\" \"5\" }
            \"250820\" { \"Playtime\" \"5\" } } } } } }",
    );
    write_bytes(
        &root.join("appcache/appinfo.vdf"),
        &appinfo_vdf(
            APPINFO_V29,
            vec![
                cached_app(10, "Lantern Run (cached)", "Game"),
                cached_app(30, "Glass Orchard", "game"),
                cached_app(50, "Paper Comets Demo", "Demo"),
                cached_app(60, "Example Dedicated Server", "Tool"),
                cached_app(70, "Glass Orchard - Extra Levels", "DLC"),
                cached_app(80, "Example Level Editor", "Application"),
                cached_app(95, "Proton 9.0", "Game"),
                cached_app(96, "Glass Orchard Playtest", "Beta"),
                cached_app(99, "Unrelated App", "Game"),
                cached_app(250_820, "Example VR Runtime", "Tool"),
            ],
        ),
    );
    let reports = Steam::with_roots(vec![root])
        .scan(&ScanOptions::default())
        .await
        .expect("scan");
    let r = &reports[0];

    let lantern = item(r, Some(PLAYER_ONE), "10");
    assert_eq!(
        lantern.title.as_deref(),
        Some("Lantern Run"),
        "manifest names win"
    );
    let orchard = item(r, Some(PLAYER_ONE), "30");
    assert_eq!(orchard.title.as_deref(), Some("Glass Orchard"));
    assert_eq!(orchard.kind, ProductKind::Game);

    let two = r
        .accounts
        .iter()
        .find(|a| a.external_account_id == PLAYER_TWO)
        .expect("two");
    let ids: Vec<&str> = two.items.iter().map(|i| i.platform_game_id.as_str()).collect();
    assert_eq!(
        ids,
        vec!["10", "250820", "70", "80", "90"],
        "no demos, playtests, tools or runtimes"
    );
    let extra = item(r, Some(PLAYER_TWO), "70");
    assert_eq!(extra.title.as_deref(), Some("Glass Orchard - Extra Levels"));
    assert_eq!(extra.kind, ProductKind::Dlc);
    assert_eq!(item(r, Some(PLAYER_TWO), "80").kind, ProductKind::Tool);
    assert_eq!(item(r, Some(PLAYER_TWO), "250820").kind, ProductKind::Tool);
    let uncached = item(r, Some(PLAYER_TWO), "90");
    assert_eq!(
        (uncached.title.as_deref(), uncached.kind),
        (None, ProductKind::Game)
    );

    let json = serde_json::to_string(&reports).expect("json");
    for unread in [
        "Unrelated App",
        "Not The Name",
        "Nested Name",
        "Example Studio",
        "windows",
    ] {
        assert!(!json.contains(unread), "{unread}");
    }
}

#[tokio::test]
async fn steam_app_cache_versions_and_damage_never_fail_the_scan() {
    let apps = || {
        vec![
            cached_app(30, "Glass Orchard", "Game"),
            cached_app(31, "Glass Orchard II", "Game"),
        ]
    };
    let v28 = appinfo_vdf(APPINFO_V28, apps());
    let v29 = appinfo_vdf(APPINFO_V29, apps());
    let mut v27 = v28.clone();
    v27[..4].copy_from_slice(&APPINFO_V27.to_le_bytes());
    let both = [Some("Glass Orchard"), Some("Glass Orchard II")];
    let cases = [
        ("v28", v28.clone(), both),
        ("v29", v29.clone(), both),
        (
            "v28 cut short",
            v28[..v28.len() - 20].to_vec(),
            [Some("Glass Orchard"), None],
        ),
        ("v29 cut short", v29[..v29.len() - 20].to_vec(), [None, None]),
        ("unknown version", v27, [None, None]),
        ("empty", Vec::new(), [None, None]),
    ];
    for (case, bytes, titles) in cases {
        let dir = tempfile::tempdir().expect("tmp");
        let root = steam_tree(dir.path());
        write(
            &root.join("userdata/1002/config/localconfig.vdf"),
            "\"UserLocalConfigStore\" { \"Software\" { \"Valve\" { \"Steam\" { \"apps\" {
                \"30\" { \"Playtime\" \"5\" } \"31\" { \"Playtime\" \"5\" } } } } } }",
        );
        write_bytes(&root.join("appcache/appinfo.vdf"), &bytes);
        let reports = Steam::with_roots(vec![root])
            .scan(&ScanOptions::default())
            .await
            .expect(case);
        let r = &reports[0];
        let read = ["30", "31"].map(|id| item(r, Some(PLAYER_TWO), id).title.as_deref());
        assert_eq!(read, titles, "{case}");
    }
}

#[test]
fn app_cache_reads_only_the_common_name_and_type_of_wanted_apps() {
    let dir = tempfile::tempdir().expect("tmp");
    let path = dir.path().join("appinfo.vdf");
    for magic in [APPINFO_V28, APPINFO_V29] {
        let bytes = appinfo_vdf(
            magic,
            vec![
                cached_app(10, "Lantern Run", "Game"),
                cached_app(20, "Paper Comets", "DLC"),
            ],
        );
        write_bytes(&path, &bytes);
        let apps = appinfo::read(&path, &BTreeSet::from([20, 30])).expect("read");
        assert_eq!(apps.len(), 1);
        assert_eq!(
            apps[&20],
            appinfo::App {
                name: Some("Paper Comets".into()),
                app_type: Some("dlc".into()),
            }
        );
        // Truncations are tolerated: an error or fewer apps, never a panic.
        for len in (0..bytes.len()).step_by(7) {
            write_bytes(&path, &bytes[..len]);
            if let Ok(apps) = appinfo::read(&path, &BTreeSet::from([10, 20])) {
                assert!(apps.values().all(|a| a.app_type.is_some()), "{len}");
            }
        }
    }
}

#[tokio::test]
async fn missing_launchers_are_not_detected() {
    let dir = tempfile::tempdir().expect("tmp");
    let opts = ScanOptions::default();
    let steam = Steam::with_roots(vec![dir.path().join("nope")]);
    assert!(!steam.detect(&opts));
    assert!(steam.scan(&opts).await.is_err());
    let gog = GogGalaxy::with_dirs(vec![dir.path().to_path_buf()]);
    assert!(!gog.detect(&opts));
    assert!(!Ubisoft::with_entries(Vec::new()).detect(&opts));
}

#[tokio::test]
async fn epic_reads_launcher_manifests_and_legendary() {
    let dir = tempfile::tempdir().expect("tmp");
    let manifests = dir.path().join("Manifests");
    write(
        &manifests.join("A1.item"),
        r#"{"DisplayName":"Night Market Detective","CatalogNamespace":"ns1","CatalogItemId":"item1","AppName":"Night",
            "InstallLocation":"C:\\Games\\Night","InstallSize":4096,"bIsIncompleteInstall":false,
            "MainGameCatalogItemId":"item1","AppCategories":["public","games","applications"]}"#,
    );
    write(
        &manifests.join("A2.item"),
        r#"{"DisplayName":"Night Market Detective - Soundtrack","CatalogNamespace":"ns1","CatalogItemId":"item2",
            "AppName":"NightOst","MainGameCatalogItemId":"item1","AppCategories":["addons"]}"#,
    );
    write(
        &manifests.join("UE.item"),
        r#"{"DisplayName":"Unreal Engine","CatalogNamespace":"ue","CatalogItemId":"ue1","AppName":"UE_5.4","AppCategories":["engines"]}"#,
    );
    write(&manifests.join("broken.item"), "{ not json");
    let legendary = dir.path().join("legendary");
    write(
        &legendary.join("installed.json"),
        r#"{"Comets":{"app_name":"Comets","title":"Paper Comets","install_path":"/games/comets","install_size":100,"is_dlc":false},
            "Orphan":{"app_name":"Orphan","title":"No Metadata"}}"#,
    );
    write(
        &legendary.join("metadata/Comets.json"),
        r#"{"app_name":"Comets","metadata":{"id":"item9","namespace":"ns9","title":"Paper Comets"}}"#,
    );

    let reports = Epic::with_dirs(vec![manifests, legendary])
        .scan(&ScanOptions::default())
        .await
        .expect("scan");
    let r = &reports[0];
    assert!(r.accounts.is_empty());
    let ids: Vec<&str> = r
        .machine_items
        .iter()
        .map(|i| i.platform_game_id.as_str())
        .collect();
    assert_eq!(ids, vec!["ns1:item1", "ns1:item2", "ns9:item9"]);
    assert_eq!(item(r, None, "ns1:item2").kind, ProductKind::Dlc);
    assert_eq!(item(r, None, "ns1:item1").size_bytes, Some(4096));
    assert_eq!(item(r, None, "ns9:item9").title.as_deref(), Some("Paper Comets"));
}

#[tokio::test]
async fn gog_galaxy_reads_a_copy_of_the_database() {
    let dir = tempfile::tempdir().expect("tmp");
    let storage = dir.path().join("storage");
    std::fs::create_dir_all(&storage).expect("mkdir");
    let db = storage.join("galaxy-2.0.db");
    let mut conn =
        SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(&db).create_if_missing(true))
            .await
            .expect("db");
    for sql in [
        "CREATE TABLE LibraryReleases (userId INTEGER, releaseKey TEXT)",
        "CREATE TABLE GamePieceTypes (id INTEGER PRIMARY KEY, type TEXT)",
        "CREATE TABLE GamePieces (releaseKey TEXT, gamePieceTypeId INTEGER, value TEXT)",
        "CREATE TABLE ReleaseProperties (releaseKey TEXT, isDlc INTEGER, isVisibleInLibrary INTEGER)",
        "CREATE TABLE GameTimes (userId INTEGER, releaseKey TEXT, minutesInGame INTEGER)",
        "CREATE TABLE LastPlayedDates (gameReleaseKey TEXT, lastPlayedDate TEXT, userId INTEGER)",
        "CREATE TABLE InstalledBaseProducts (productId INTEGER, installationDate TEXT)",
        "INSERT INTO LibraryReleases VALUES (4242, 'gog_1001'), (4242, 'gog_1002'), (4242, 'steam_10'), (5353, 'gog_1001')",
        "INSERT INTO GamePieceTypes VALUES (1, 'title'), (2, 'originalTitle'), (3, 'meta')",
        "INSERT INTO GamePieces VALUES ('gog_1001', 2, '{\"title\":\"Old Name\"}'), ('gog_1001', 1, '{\"title\":\"Lantern Run\"}'),
            ('gog_1002', 1, '{\"title\":\"Lantern Run - Artbook\"}'), ('steam_10', 1, '{\"title\":\"Elsewhere\"}')",
        "INSERT INTO ReleaseProperties VALUES ('gog_1002', 1, 1)",
        "INSERT INTO GameTimes VALUES (4242, 'gog_1001', 90), (5353, 'gog_1001', 0)",
        "INSERT INTO LastPlayedDates VALUES ('gog_1001', '2024-03-01 20:15:00', 4242)",
        "INSERT INTO InstalledBaseProducts VALUES (1001, '2024-01-01 00:00:00')",
    ] {
        sqlx::query(sql).execute(&mut conn).await.expect(sql);
    }
    conn.close().await.expect("close");

    let source = GogGalaxy::with_dirs(vec![storage]);
    assert!(source.detect(&ScanOptions::default()));
    let reports = source.scan(&ScanOptions::default()).await.expect("scan");
    let r = &reports[0];
    assert_eq!(r.platform, "gog");
    assert_eq!(r.accounts.len(), 2);
    let lantern = item(r, Some("4242"), "1001");
    assert_eq!(lantern.title.as_deref(), Some("Lantern Run"));
    assert!(lantern.installed);
    assert_eq!(lantern.playtime_minutes, Some(90));
    assert!(lantern.last_played_at.is_some());
    assert_eq!(item(r, Some("4242"), "1002").kind, ProductKind::Dlc);
    assert_eq!(item(r, Some("5353"), "1001").playtime_minutes, None);
    let json = serde_json::to_string(&reports).expect("json");
    assert!(
        !json.contains("Elsewhere"),
        "other platforms' releases are not reported"
    );
}

fn entry(key: &str, name: &str, publisher: &str, uninstall: Option<&str>) -> UninstallEntry {
    UninstallEntry {
        key: key.into(),
        display_name: name.into(),
        publisher: Some(publisher.into()),
        install_location: Some(r"C:\Games\Example".into()),
        uninstall_string: uninstall.map(str::to_string),
        size_kb: Some(10),
    }
}

#[tokio::test]
async fn battlenet_combines_uninstall_entries_and_last_played() {
    let dir = tempfile::tempdir().expect("tmp");
    let config = dir.path().join("Battle.net.config");
    write(
        &config,
        r#"{"Client":{"SavedAccountNames":"someone@example.invalid"},
            "Games":{"battle_net":{"LastPlayed":"1700000000"},"wow_enus":{"LastPlayed":"1700000100"},
                     "s2":{"LastPlayed":"1700000200"},"unknown_code":{"LastPlayed":"1700000300"}}}"#,
    );
    let entries = vec![
        entry(
            "World of Warcraft",
            "World of Warcraft",
            "Blizzard Entertainment",
            Some(r#""C:\Program Files (x86)\Battle.net\Battle.net.exe" --uid=wow_enus --uninstall"#),
        ),
        entry("Battle.net", "Battle.net", "Blizzard Entertainment", None),
    ];
    let reports = BattleNet::with_data(entries, Some(config))
        .scan(&ScanOptions::default())
        .await
        .expect("scan");
    let r = &reports[0];
    let ids: Vec<&str> = r
        .machine_items
        .iter()
        .map(|i| i.platform_game_id.as_str())
        .collect();
    assert_eq!(ids, vec!["s2", "wow"]);
    let wow = item(r, None, "wow");
    assert!(wow.installed);
    assert_eq!(
        wow.last_played_at.map(|t| t.unix_timestamp()),
        Some(1_700_000_100)
    );
    assert!(!item(r, None, "s2").installed);
    let json = serde_json::to_string(&reports).expect("json");
    assert!(!json.contains("example.invalid"), "account names are never read");
}

#[tokio::test]
async fn ubisoft_reads_uplay_install_entries() {
    let entries = vec![
        entry("Uplay Install 635", "Example Ubisoft Game", "Ubisoft", None),
        entry("Uplay Install abc", "Not an id", "Ubisoft", None),
        entry("{GUID}", "Ubisoft Connect", "Ubisoft", None),
    ];
    let reports = Ubisoft::with_entries(entries)
        .scan(&ScanOptions::default())
        .await
        .expect("scan");
    let ids: Vec<&str> = reports[0]
        .machine_items
        .iter()
        .map(|i| i.platform_game_id.as_str())
        .collect();
    assert_eq!(ids, vec!["635"]);
    assert_eq!(reports[0].platform, "ubisoft");
}
