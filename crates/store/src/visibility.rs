//! The visibility choke point (research D8, FR-033, SC-011).
//!
//! Every read of ownerships for the web layer goes through the `visible_ownerships` common table expression
//! below. A viewer sees ownerships of their own accounts, plus those of profiles sharing with them, minus the
//! owner's exclusions (by game, platform or account, global or for this viewer). Exclusions are applied before
//! any aggregation, so counts and filters never reveal excluded items.

use crate::{
    Result, Store,
    ids::{TextId, opt_id},
};
use gameshelf_core::{AccountId, Acquisition, GameId, OwnershipId, ProfileId};
use sqlx::{Row, sqlite::SqliteArguments};
use time::OffsetDateTime;

/// Common table expressions. `?1` is the viewer's profile id.
pub(crate) const VISIBLE_CTE: &str = "
visible_accounts AS (
    SELECT a.id, a.profile_id, a.platform, a.label
    FROM platform_accounts a
    WHERE a.profile_id = ?1
       OR EXISTS (SELECT 1 FROM sharing_rules r
                  WHERE r.owner_profile_id = a.profile_id AND r.audience_profile_id = ?1)
),
visible_ownerships AS (
    SELECT o.id, o.account_id, o.product_id, o.acquisition, o.first_seen_at, o.last_seen_at,
           o.unavailable_since, o.playtime_minutes, o.last_played_at, o.achievements_unlocked,
           o.achievements_total, o.note, va.profile_id AS owner_profile_id, va.platform, va.label AS account_label,
           p.game_id, p.title AS product_title, p.platform_game_id, p.edition, p.kind AS product_kind
    FROM ownerships o
    JOIN visible_accounts va ON va.id = o.account_id
    JOIN platform_products p ON p.id = o.product_id
    WHERE va.profile_id = ?1
       OR NOT EXISTS (
            SELECT 1 FROM sharing_exclusions e
            WHERE e.owner_profile_id = va.profile_id
              AND (e.audience_profile_id IS NULL OR e.audience_profile_id = ?1)
              AND ((e.kind = 'account' AND e.ref = va.id)
                OR (e.kind = 'platform' AND e.ref = va.platform)
                OR (e.kind = 'game' AND e.ref = p.game_id)))
)";

/// Sort orders of the library (FR-014).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sort {
    #[default]
    Title,
    Added,
    LastPlayed,
    Playtime,
}

/// Filters and paging of the library (FR-014). Empty lists mean "no filter".
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LibraryQuery {
    pub platforms: Vec<String>,
    pub accounts: Vec<AccountId>,
    pub owners: Vec<ProfileId>,
    pub acquisitions: Vec<Acquisition>,
    /// Restrict to these games (search results).
    pub games: Option<Vec<GameId>>,
    pub include_subscription_only: bool,
    pub include_unavailable: bool,
    pub include_free_to_play: bool,
    pub include_tools: bool,
    pub installed_only: bool,
    pub tag: Option<String>,
    pub sort: Sort,
    pub descending: bool,
    pub offset: u32,
    pub limit: u32,
}

impl Default for LibraryQuery {
    fn default() -> Self {
        Self {
            platforms: Vec::new(),
            accounts: Vec::new(),
            owners: Vec::new(),
            acquisitions: Vec::new(),
            games: None,
            include_subscription_only: false,
            include_unavailable: false,
            include_free_to_play: true,
            include_tools: false,
            installed_only: false,
            tag: None,
            sort: Sort::Title,
            descending: false,
            offset: 0,
            limit: 120,
        }
    }
}

/// One tile of the library grid.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LibraryGame {
    pub game_id: GameId,
    pub title: String,
    pub cover: Option<String>,
    pub platforms: Vec<String>,
    /// Visible accounts holding the game (all profiles).
    pub account_count: u32,
    /// Accounts of the viewer holding the game.
    pub own_account_count: u32,
    /// Other profiles whose shared libraries hold the game.
    pub other_owners: Vec<ProfileId>,
    pub acquisitions: Vec<Acquisition>,
    /// True when at least one visible ownership is a real copy (not subscription or family sharing).
    pub owned: bool,
    pub added_at: Option<OffsetDateTime>,
    pub last_played_at: Option<OffsetDateTime>,
    pub playtime_minutes: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LibraryPage {
    pub games: Vec<LibraryGame>,
    pub total: u32,
}

/// One ownership as shown in a game's detail.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct VisibleOwnership {
    pub id: OwnershipId,
    pub account_id: AccountId,
    pub account_label: String,
    pub platform: String,
    pub owner_profile_id: ProfileId,
    pub mine: bool,
    pub product_title: String,
    pub platform_game_id: String,
    pub product_kind: String,
    pub acquisition: Acquisition,
    pub edition: Option<String>,
    pub first_seen_at: OffsetDateTime,
    pub unavailable_since: Option<OffsetDateTime>,
    pub playtime_minutes: Option<u32>,
    pub last_played_at: Option<OffsetDateTime>,
    pub achievements: Option<(u32, u32)>,
}

struct Sql {
    text: String,
    args: Vec<Arg>,
}

enum Arg {
    Text(String),
    Int(i64),
}

impl Sql {
    fn new(viewer: ProfileId) -> Self {
        Self {
            text: String::new(),
            args: vec![Arg::Text(viewer.txt())],
        }
    }

    fn push(&mut self, s: &str) {
        self.text.push_str(s);
    }

    fn bind_text(&mut self, v: String) -> String {
        self.args.push(Arg::Text(v));
        format!("?{}", self.args.len())
    }

    fn bind_int(&mut self, v: i64) -> String {
        self.args.push(Arg::Int(v));
        format!("?{}", self.args.len())
    }

    fn in_list(&mut self, column: &str, values: impl IntoIterator<Item = String>) {
        let placeholders: Vec<String> = values.into_iter().map(|v| self.bind_text(v)).collect();
        if placeholders.is_empty() {
            return;
        }
        self.push(&format!(" AND {column} IN ({})", placeholders.join(", ")));
    }

    fn arguments(&self) -> SqliteArguments {
        use sqlx::Arguments;
        let mut args = SqliteArguments::default();
        for a in &self.args {
            let _ = match a {
                Arg::Text(s) => args.add(s.clone()),
                Arg::Int(i) => args.add(*i),
            };
        }
        args
    }
}

fn split_list(s: Option<String>) -> Vec<String> {
    s.map(|s| {
        s.split(',')
            .filter(|x| !x.is_empty())
            .map(str::to_string)
            .collect()
    })
    .unwrap_or_default()
}

impl Store {
    /// The library grid for a viewer (FR-011, FR-014).
    pub async fn library(&self, viewer: ProfileId, q: &LibraryQuery) -> Result<LibraryPage> {
        let mut sql = Sql::new(viewer);
        sql.push(&format!(
            "WITH {VISIBLE_CTE}, filtered AS (SELECT vo.* FROM visible_ownerships vo WHERE 1 = 1"
        ));
        if !q.include_unavailable {
            sql.push(" AND vo.unavailable_since IS NULL");
        }
        if !q.include_free_to_play {
            sql.push(" AND vo.acquisition <> 'free_to_play'");
        }
        sql.in_list("vo.platform", q.platforms.iter().cloned());
        sql.in_list("vo.account_id", q.accounts.iter().map(|a| a.txt()));
        sql.in_list("vo.owner_profile_id", q.owners.iter().map(|p| p.txt()));
        sql.in_list(
            "vo.acquisition",
            q.acquisitions.iter().map(|a| a.as_str().to_string()),
        );
        if let Some(games) = &q.games {
            if games.is_empty() {
                return Ok(LibraryPage {
                    games: Vec::new(),
                    total: 0,
                });
            }
            sql.in_list("vo.game_id", games.iter().map(|g| g.txt()));
        }
        if q.installed_only {
            sql.push(
                " AND EXISTS (SELECT 1 FROM installs i JOIN platform_accounts ia ON ia.id = vo.account_id
                   WHERE i.platform = vo.platform AND i.platform_game_id = vo.platform_game_id)",
            );
        }
        if let Some(tag) = &q.tag {
            let t = sql.bind_text(tag.clone());
            sql.push(&format!(
                " AND vo.game_id IN (SELECT gt.game_id FROM game_tags gt JOIN tags t ON t.id = gt.tag_id
                   WHERE gt.profile_id = ?1 AND t.name = {t})"
            ));
        }
        sql.push("), grouped AS (
            SELECT g.id AS game_id, g.title, g.sort_title,
                   (SELECT a.sha256 FROM assets a WHERE a.id = g.cover_asset_id) AS cover,
                   group_concat(DISTINCT f.platform) AS platforms,
                   count(DISTINCT f.account_id) AS account_count,
                   count(DISTINCT CASE WHEN f.owner_profile_id = ?1 THEN f.account_id END) AS own_account_count,
                   group_concat(DISTINCT CASE WHEN f.owner_profile_id <> ?1 THEN f.owner_profile_id END) AS other_owners,
                   group_concat(DISTINCT f.acquisition) AS acquisitions,
                   sum(CASE WHEN f.acquisition NOT IN ('subscription', 'family_shared') THEN 1 ELSE 0 END) AS owned_rows,
                   min(f.first_seen_at) AS added_at,
                   max(f.last_played_at) AS last_played_at,
                   sum(f.playtime_minutes) AS playtime
            FROM filtered f JOIN games g ON g.id = f.game_id
            WHERE g.kind IN ('game'");
        if q.include_tools {
            sql.push(", 'tool'");
        }
        sql.push(") GROUP BY g.id");
        if !q.include_subscription_only {
            sql.push(" HAVING owned_rows > 0");
        }
        sql.push(")");

        let base_len = sql.text.len();
        sql.push(" SELECT count(*) FROM grouped");
        let total: i64 = sqlx::query_scalar_with(sqlx::AssertSqlSafe(sql.text.clone()), sql.arguments())
            .fetch_one(self.pool())
            .await?;
        sql.text.truncate(base_len);

        let dir = if q.descending { "DESC" } else { "ASC" };
        let order = match q.sort {
            Sort::Title => format!("sort_title COLLATE NOCASE {dir}"),
            Sort::Added => format!("added_at {dir}, sort_title COLLATE NOCASE"),
            Sort::LastPlayed => {
                format!("last_played_at IS NULL, last_played_at {dir}, sort_title COLLATE NOCASE")
            }
            Sort::Playtime => format!("playtime IS NULL, playtime {dir}, sort_title COLLATE NOCASE"),
        };
        let limit = sql.bind_int(i64::from(q.limit.clamp(1, 500)));
        let offset = sql.bind_int(i64::from(q.offset));
        sql.push(&format!(
            " SELECT * FROM grouped ORDER BY {order} LIMIT {limit} OFFSET {offset}"
        ));
        let rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.text.clone()), sql.arguments())
            .fetch_all(self.pool())
            .await?;

        let mut games = Vec::with_capacity(rows.len());
        for r in &rows {
            let n = |col: &str| -> Result<u32> { Ok(u32::try_from(r.try_get::<i64, _>(col)?).unwrap_or(0)) };
            let playtime: Option<i64> = r.try_get("playtime")?;
            games.push(LibraryGame {
                game_id: GameId::from_txt(&r.try_get::<String, _>("game_id")?)?,
                title: r.try_get("title")?,
                cover: r.try_get("cover")?,
                platforms: split_list(r.try_get("platforms")?),
                account_count: n("account_count")?,
                own_account_count: n("own_account_count")?,
                other_owners: split_list(r.try_get("other_owners")?)
                    .iter()
                    .map(|s| ProfileId::from_txt(s))
                    .collect::<Result<_>>()?,
                acquisitions: split_list(r.try_get("acquisitions")?)
                    .iter()
                    .filter_map(|s| Acquisition::parse(s))
                    .collect(),
                owned: r.try_get::<i64, _>("owned_rows")? > 0,
                added_at: r.try_get("added_at")?,
                last_played_at: r.try_get("last_played_at")?,
                playtime_minutes: playtime.and_then(|p| u32::try_from(p).ok()),
            });
        }
        Ok(LibraryPage {
            games,
            total: u32::try_from(total).unwrap_or(u32::MAX),
        })
    }

    /// Visible ownerships of one game, the viewer's own accounts first (FR-016, US6 scenario 4).
    /// An empty result means the viewer may not see the game at all.
    pub async fn game_ownerships(&self, viewer: ProfileId, game: GameId) -> Result<Vec<VisibleOwnership>> {
        let sql = format!(
            "WITH {VISIBLE_CTE}
             SELECT * FROM visible_ownerships WHERE game_id = ?2
             ORDER BY owner_profile_id <> ?1, platform, account_label COLLATE NOCASE"
        );
        let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(viewer.txt())
            .bind(game.txt())
            .fetch_all(self.pool())
            .await?;
        rows.iter()
            .map(|r| {
                let acq: String = r.try_get("acquisition")?;
                let owner = ProfileId::from_txt(&r.try_get::<String, _>("owner_profile_id")?)?;
                let pt: Option<i64> = r.try_get("playtime_minutes")?;
                let au: Option<i64> = r.try_get("achievements_unlocked")?;
                let at: Option<i64> = r.try_get("achievements_total")?;
                Ok(VisibleOwnership {
                    id: OwnershipId::from_txt(&r.try_get::<String, _>("id")?)?,
                    account_id: AccountId::from_txt(&r.try_get::<String, _>("account_id")?)?,
                    account_label: r.try_get("account_label")?,
                    platform: r.try_get("platform")?,
                    owner_profile_id: owner,
                    mine: owner == viewer,
                    product_title: r.try_get("product_title")?,
                    platform_game_id: r.try_get("platform_game_id")?,
                    product_kind: r.try_get("product_kind")?,
                    acquisition: Acquisition::parse(&acq).unwrap_or(Acquisition::Purchase),
                    edition: r.try_get("edition")?,
                    first_seen_at: r.try_get("first_seen_at")?,
                    unavailable_since: r.try_get("unavailable_since")?,
                    playtime_minutes: pt.and_then(|p| u32::try_from(p).ok()),
                    last_played_at: r.try_get("last_played_at")?,
                    achievements: match (au, at) {
                        (Some(u), Some(t)) => {
                            Some((u32::try_from(u).unwrap_or(0), u32::try_from(t).unwrap_or(0)))
                        }
                        _ => None,
                    },
                })
            })
            .collect()
    }

    /// Ids and normalized titles of every game the viewer may see, for the search index.
    pub async fn visible_game_keys(&self, viewer: ProfileId) -> Result<Vec<(GameId, String, String)>> {
        let sql = format!(
            "WITH {VISIBLE_CTE}
             SELECT DISTINCT g.id, g.title, g.search_key FROM visible_ownerships vo JOIN games g ON g.id = vo.game_id
             WHERE g.kind IN ('game', 'tool')"
        );
        let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(viewer.txt())
            .fetch_all(self.pool())
            .await?;
        rows.iter()
            .map(|r| {
                Ok((
                    GameId::from_txt(&r.try_get::<String, _>("id")?)?,
                    r.try_get("title")?,
                    r.try_get("search_key")?,
                ))
            })
            .collect()
    }

    /// Parent game of a DLC, if any (used to keep DLC out of the grid).
    pub async fn game_parent(&self, game: GameId) -> Result<Option<GameId>> {
        let parent: Option<Option<String>> =
            sqlx::query_scalar("SELECT parent_game_id FROM games WHERE id = ?")
                .bind(game.txt())
                .fetch_optional(self.pool())
                .await?;
        opt_id(parent.flatten())
    }
}
