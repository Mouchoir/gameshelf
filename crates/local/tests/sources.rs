//! Local sources read synthetic launcher files built in a temporary folder. No real launcher data is used.

use gameshelf_core::{ProductKind, agent::Report};
use gameshelf_local::{
    LocalSource, ScanOptions, battlenet::BattleNet, epic::Epic, gog::GogGalaxy, registry::UninstallEntry,
    steam::Steam, ubisoft::Ubisoft,
};
use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};
use std::path::Path;

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
