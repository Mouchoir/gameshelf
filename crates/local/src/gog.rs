//! `gog-galaxy-local`: the GOG Galaxy 2.0 database. It is copied to a temporary folder before reading so the
//! running client is never disturbed. Only `gog_` releases are reported; releases Galaxy imports from other
//! platforms are left to those platforms' own sources.

use crate::{LocalError, LocalSource, ScanOptions, env_path, existing, finish, unix};
use gameshelf_core::{
    ProductKind,
    agent::{Report, ReportAccount, ReportItem},
};
use sqlx::{Connection, Row, SqliteConnection, sqlite::SqliteConnectOptions};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::{Path, PathBuf},
};
use time::{OffsetDateTime, PrimitiveDateTime, macros::format_description};

const DB_FILE: &str = "galaxy-2.0.db";

#[derive(Clone, Debug, Default)]
pub struct GogGalaxy {
    dirs: Option<Vec<PathBuf>>,
}

impl GogGalaxy {
    /// Reads the database in the given folders instead of the default location.
    pub fn with_dirs(dirs: Vec<PathBuf>) -> Self {
        Self { dirs: Some(dirs) }
    }

    fn database(&self, opts: &ScanOptions) -> Option<PathBuf> {
        let base = self.dirs.clone().unwrap_or_else(|| {
            env_path("ProgramData")
                .map(|pd| vec![pd.join("GOG.com").join("Galaxy").join("storage")])
                .unwrap_or_default()
        });
        existing(base.into_iter().chain(opts.extra_paths.iter().cloned()))
            .into_iter()
            .map(|d| d.join(DB_FILE))
            .find(|f| f.is_file())
    }
}

#[async_trait::async_trait]
impl LocalSource for GogGalaxy {
    fn id(&self) -> &'static str {
        "gog-galaxy-local"
    }

    fn detect(&self, opts: &ScanOptions) -> bool {
        self.database(opts).is_some()
    }

    async fn scan(&self, opts: &ScanOptions) -> Result<Vec<Report>, LocalError> {
        let db = self.database(opts).ok_or(LocalError::NotFound)?;
        let copy = tempfile::tempdir()?;
        for suffix in ["", "-wal"] {
            let from = PathBuf::from(format!("{}{suffix}", db.display()));
            if from.is_file() {
                tokio::fs::copy(&from, copy.path().join(format!("{DB_FILE}{suffix}"))).await?;
            }
        }
        let report = read(&copy.path().join(DB_FILE)).await;
        drop(copy);
        Ok(vec![report?])
    }
}

fn db_err(e: sqlx::Error) -> LocalError {
    LocalError::Database(e.to_string())
}

async fn read(path: &Path) -> Result<Report, LocalError> {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(false);
    let mut conn = SqliteConnection::connect_with(&options).await.map_err(db_err)?;
    let tables: HashSet<String> = sqlx::query_scalar("SELECT name FROM sqlite_master WHERE type = 'table'")
        .fetch_all(&mut conn)
        .await
        .map_err(db_err)?
        .into_iter()
        .collect();
    if !tables.contains("LibraryReleases") {
        return Err(LocalError::Format("unsupported GOG Galaxy database".into()));
    }

    let owned =
        sqlx::query("SELECT DISTINCT CAST(userId AS TEXT) AS user_id, releaseKey FROM LibraryReleases")
            .fetch_all(&mut conn)
            .await
            .map_err(db_err)?;

    let mut titles: HashMap<String, String> = HashMap::new();
    if tables.contains("GamePieces") && tables.contains("GamePieceTypes") {
        let rows = sqlx::query(
            "SELECT p.releaseKey AS release_key, p.value AS value, t.type AS kind FROM GamePieces p
               JOIN GamePieceTypes t ON t.id = p.gamePieceTypeId
              WHERE t.type IN ('title', 'originalTitle') AND p.releaseKey LIKE 'gog\\_%' ESCAPE '\\'
              ORDER BY t.type = 'title'",
        )
        .fetch_all(&mut conn)
        .await
        .map_err(db_err)?;
        for r in rows {
            let key: String = r.try_get("release_key").map_err(db_err)?;
            let value: String = r.try_get("value").unwrap_or_default();
            let title = serde_json::from_str::<serde_json::Value>(&value)
                .ok()
                .and_then(|v| v.get("title").and_then(|t| t.as_str()).map(str::to_string));
            if let Some(t) = title.filter(|t| !t.trim().is_empty()) {
                titles.insert(key, t);
            }
        }
    }

    let dlcs: HashSet<String> =
        if has_columns(&mut conn, &tables, "ReleaseProperties", &["releaseKey", "isDlc"]).await {
            sqlx::query_scalar("SELECT releaseKey FROM ReleaseProperties WHERE isDlc = 1")
                .fetch_all(&mut conn)
                .await
                .map_err(db_err)?
                .into_iter()
                .collect()
        } else {
            HashSet::new()
        };

    let mut playtime: HashMap<(String, String), u32> = HashMap::new();
    if has_columns(
        &mut conn,
        &tables,
        "GameTimes",
        &["userId", "releaseKey", "minutesInGame"],
    )
    .await
    {
        for r in
            sqlx::query("SELECT CAST(userId AS TEXT) AS user_id, releaseKey, minutesInGame FROM GameTimes")
                .fetch_all(&mut conn)
                .await
                .map_err(db_err)?
        {
            let minutes: i64 = r.try_get("minutesInGame").unwrap_or(0);
            if let (Ok(user), Ok(key), Ok(m)) = (
                r.try_get::<String, _>("user_id"),
                r.try_get::<String, _>("releaseKey"),
                u32::try_from(minutes),
            ) && m > 0
            {
                playtime.insert((user, key), m);
            }
        }
    }

    let mut last_played: HashMap<(String, String), OffsetDateTime> = HashMap::new();
    if has_columns(
        &mut conn,
        &tables,
        "LastPlayedDates",
        &["userId", "gameReleaseKey", "lastPlayedDate"],
    )
    .await
    {
        for r in sqlx::query(
            "SELECT CAST(userId AS TEXT) AS user_id, gameReleaseKey, CAST(lastPlayedDate AS TEXT) AS last FROM LastPlayedDates",
        )
        .fetch_all(&mut conn)
        .await
        .map_err(db_err)?
        {
            if let (Ok(user), Ok(key), Ok(Some(text))) = (
                r.try_get::<String, _>("user_id"),
                r.try_get::<String, _>("gameReleaseKey"),
                r.try_get::<Option<String>, _>("last"),
            ) && let Some(at) = parse_date(&text)
            {
                last_played.insert((user, key), at);
            }
        }
    }

    let installed: HashSet<String> =
        if has_columns(&mut conn, &tables, "InstalledBaseProducts", &["productId"]).await {
            sqlx::query_scalar("SELECT CAST(productId AS TEXT) FROM InstalledBaseProducts")
                .fetch_all(&mut conn)
                .await
                .map_err(db_err)?
                .into_iter()
                .collect()
        } else {
            HashSet::new()
        };
    conn.close().await.map_err(db_err)?;

    let mut accounts: BTreeMap<String, Vec<ReportItem>> = BTreeMap::new();
    for row in owned {
        let user: String = row.try_get("user_id").map_err(db_err)?;
        let key: String = row.try_get("releaseKey").map_err(db_err)?;
        let Some(product_id) = key
            .strip_prefix("gog_")
            .filter(|id| id.chars().all(|c| c.is_ascii_digit()))
        else {
            continue;
        };
        let pair = (user.clone(), key.clone());
        accounts.entry(user).or_default().push(ReportItem {
            platform_game_id: product_id.to_string(),
            title: titles.get(&key).cloned(),
            kind: if dlcs.contains(&key) {
                ProductKind::Dlc
            } else {
                ProductKind::Game
            },
            installed: installed.contains(product_id),
            size_bytes: None,
            playtime_minutes: playtime.get(&pair).copied(),
            last_played_at: last_played.get(&pair).copied(),
            install_path: None,
        });
    }
    let accounts = accounts
        .into_iter()
        .map(|(id, items)| ReportAccount {
            external_account_id: id,
            display_name: None,
            items,
        })
        .collect();
    Ok(finish("gog-galaxy-local", "gog", accounts, Vec::new()))
}

/// Galaxy's schema changes between versions; optional tables are only read when they look as expected.
async fn has_columns(
    conn: &mut SqliteConnection,
    tables: &HashSet<String>,
    table: &str,
    columns: &[&str],
) -> bool {
    if !tables.contains(table) {
        return false;
    }
    let found: Vec<String> = sqlx::query_scalar("SELECT name FROM pragma_table_info(?)")
        .bind(table)
        .fetch_all(conn)
        .await
        .unwrap_or_default();
    columns.iter().all(|c| found.iter().any(|f| f == c))
}

/// Galaxy stores dates as text (`2024-03-01 20:15:00`, optionally with `T` and fractions) or Unix seconds.
fn parse_date(text: &str) -> Option<OffsetDateTime> {
    let t = text.trim();
    if let Ok(secs) = t.parse::<u64>() {
        return unix(secs);
    }
    let base = t.split(['.', '+', 'Z']).next()?.replace('T', " ");
    PrimitiveDateTime::parse(
        &base,
        format_description!("[year]-[month]-[day] [hour]:[minute]:[second]"),
    )
    .ok()
    .map(PrimitiveDateTime::assume_utc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_in_every_known_shape() {
        let a = parse_date("2024-03-01 20:15:00").expect("space");
        assert_eq!(parse_date("2024-03-01T20:15:00.123Z"), Some(a));
        assert_eq!(parse_date(&a.unix_timestamp().to_string()), Some(a));
        assert_eq!(parse_date("not a date"), None);
    }
}
