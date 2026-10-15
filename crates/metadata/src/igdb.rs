//! IGDB client: canonical ids through `external_games`, titles, release years and covers.

use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::sync::Mutex;

const TOKEN_URL: &str = "https://id.twitch.tv/oauth2/token";
const API_BASE: &str = "https://api.igdb.com/v4";
const IMAGE_BASE: &str = "https://images.igdb.com/igdb/image/upload";
/// IGDB allows 4 requests per second.
const MIN_INTERVAL: Duration = Duration::from_millis(260);
/// Maximum ids per `where ... = (...)` clause.
const BATCH: usize = 200;

#[derive(Debug, thiserror::Error)]
pub enum IgdbError {
    #[error("IGDB authentication failed")]
    Auth,
    #[error("IGDB request failed: {0}")]
    Http(String),
    #[error("unexpected IGDB response: {0}")]
    Parse(String),
}

impl From<reqwest::Error> for IgdbError {
    fn from(e: reqwest::Error) -> Self {
        IgdbError::Http(e.without_url().to_string())
    }
}

/// External game sources of IGDB (`external_game_source`).
pub fn source_id(platform: &str) -> Option<u32> {
    match platform {
        "steam" => Some(1),
        "gog" => Some(5),
        "xbox" => Some(11),
        "epic" => Some(26),
        "itch" => Some(30),
        _ => None,
    }
}

/// What IGDB knows about a game.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IgdbGame {
    pub id: u64,
    pub name: String,
    pub release_year: Option<i32>,
    /// Large cover URL (`t_cover_big_2x`).
    pub cover_url: Option<String>,
    /// Base game when this entry is a DLC or expansion.
    pub parent: Option<u64>,
}

#[derive(Clone)]
pub struct Igdb {
    http: reqwest::Client,
    client_id: String,
    client_secret: SecretString,
    token_url: String,
    api_base: String,
    state: Arc<Mutex<State>>,
}

impl std::fmt::Debug for Igdb {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Igdb").finish_non_exhaustive()
    }
}

#[derive(Default)]
struct State {
    token: Option<(String, std::time::Instant)>,
    last_call: Option<std::time::Instant>,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: u64,
}

#[derive(Deserialize)]
struct ExternalGame {
    game: Option<u64>,
    uid: Option<String>,
}

#[derive(Deserialize)]
struct GameRow {
    id: u64,
    name: Option<String>,
    first_release_date: Option<i64>,
    cover: Option<CoverRow>,
    parent_game: Option<u64>,
    version_parent: Option<u64>,
}

#[derive(Deserialize)]
struct CoverRow {
    image_id: Option<String>,
}

impl Igdb {
    pub fn new(http: reqwest::Client, client_id: String, client_secret: SecretString) -> Self {
        Self::with_urls(http, client_id, client_secret, TOKEN_URL.into(), API_BASE.into())
    }

    pub fn with_urls(
        http: reqwest::Client,
        client_id: String,
        client_secret: SecretString,
        token_url: String,
        api_base: String,
    ) -> Self {
        Self {
            http,
            client_id,
            client_secret,
            token_url,
            api_base,
            state: Arc::default(),
        }
    }

    async fn token(&self) -> Result<String, IgdbError> {
        let mut state = self.state.lock().await;
        if let Some((token, until)) = &state.token
            && std::time::Instant::now() < *until
        {
            return Ok(token.clone());
        }
        let resp = self
            .http
            .post(&self.token_url)
            .form(&[
                ("client_id", self.client_id.as_str()),
                ("client_secret", self.client_secret.expose_secret()),
                ("grant_type", "client_credentials"),
            ])
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(IgdbError::Auth);
        }
        let t: TokenResponse = resp.json().await.map_err(|e| IgdbError::Parse(e.to_string()))?;
        let until = std::time::Instant::now() + Duration::from_secs(t.expires_in.saturating_sub(300));
        state.token = Some((t.access_token.clone(), until));
        Ok(t.access_token)
    }

    async fn query<T: serde::de::DeserializeOwned>(
        &self,
        endpoint: &str,
        body: String,
    ) -> Result<Vec<T>, IgdbError> {
        let token = self.token().await?;
        {
            let mut state = self.state.lock().await;
            if let Some(last) = state.last_call {
                let elapsed = last.elapsed();
                if elapsed < MIN_INTERVAL {
                    tokio::time::sleep(MIN_INTERVAL - elapsed).await;
                }
            }
            state.last_call = Some(std::time::Instant::now());
        }
        let resp = self
            .http
            .post(format!("{}/{endpoint}", self.api_base))
            .header("Client-ID", &self.client_id)
            .bearer_auth(token)
            .body(body)
            .send()
            .await?;
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            self.state.lock().await.token = None;
            return Err(IgdbError::Auth);
        }
        if !resp.status().is_success() {
            return Err(IgdbError::Http(format!("status {}", resp.status().as_u16())));
        }
        resp.json().await.map_err(|e| IgdbError::Parse(e.to_string()))
    }

    /// Maps platform ids (`uid`) of one source to IGDB game ids.
    pub async fn games_by_external(
        &self,
        platform: &str,
        uids: &[String],
    ) -> Result<HashMap<String, u64>, IgdbError> {
        let Some(source) = source_id(platform) else {
            return Ok(HashMap::new());
        };
        let mut out = HashMap::new();
        for chunk in uids.chunks(BATCH) {
            let list: Vec<String> = chunk.iter().map(|u| format!("\"{}\"", escape(u))).collect();
            let body = format!(
                "fields game,uid; where external_game_source = {source} & uid = ({}); limit 500;",
                list.join(",")
            );
            let rows: Vec<ExternalGame> = self.query("external_games", body).await?;
            for r in rows {
                if let (Some(game), Some(uid)) = (r.game, r.uid) {
                    out.entry(uid).or_insert(game);
                }
            }
        }
        Ok(out)
    }

    /// Fetches games by IGDB id.
    pub async fn games(&self, ids: &[u64]) -> Result<Vec<IgdbGame>, IgdbError> {
        let mut out = Vec::new();
        for chunk in ids.chunks(BATCH) {
            let list: Vec<String> = chunk.iter().map(u64::to_string).collect();
            let body = format!(
                "fields name,first_release_date,cover.image_id,parent_game,version_parent; where id = ({}); limit 500;",
                list.join(",")
            );
            let rows: Vec<GameRow> = self.query("games", body).await?;
            out.extend(rows.into_iter().map(|r| {
                IgdbGame {
                    id: r.id,
                    name: r.name.unwrap_or_default(),
                    release_year: r
                        .first_release_date
                        .and_then(|ts| time::OffsetDateTime::from_unix_timestamp(ts).ok())
                        .map(|d| d.year()),
                    cover_url: r
                        .cover
                        .and_then(|c| c.image_id)
                        .map(|id| format!("{IMAGE_BASE}/t_cover_big_2x/{id}.jpg")),
                    parent: r.parent_game.or(r.version_parent),
                }
            }));
        }
        Ok(out)
    }
}

fn escape(s: &str) -> String {
    s.chars()
        .filter(|c| *c != '"' && *c != '\\' && !c.is_control())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{body_string_contains, method, path},
    };

    #[tokio::test]
    async fn maps_steam_ids_and_fetches_covers() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"access_token": "t", "expires_in": 3600})),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v4/external_games"))
            .and(body_string_contains("external_game_source = 1"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!([{"game": 72, "uid": "620"}])),
            )
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v4/games"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {"id": 72, "name": "Portal 2", "first_release_date": 1303171200, "cover": {"image_id": "co1abc"}}
            ])))
            .mount(&server)
            .await;
        let igdb = Igdb::with_urls(
            reqwest::Client::new(),
            "client".into(),
            SecretString::from("secret".to_string()),
            format!("{}/token", server.uri()),
            format!("{}/v4", server.uri()),
        );
        let ids = igdb
            .games_by_external("steam", &["620".to_string()])
            .await
            .expect("external");
        assert_eq!(ids.get("620"), Some(&72));
        let games = igdb.games(&[72]).await.expect("games");
        assert_eq!(games[0].release_year, Some(2011));
        assert_eq!(
            games[0].cover_url.as_deref(),
            Some("https://images.igdb.com/igdb/image/upload/t_cover_big_2x/co1abc.jpg")
        );
        // Token is cached across calls.
        igdb.games(&[72]).await.expect("again");
    }

    #[test]
    fn unknown_platforms_have_no_source() {
        assert_eq!(source_id("battlenet"), None);
        assert_eq!(source_id("steam"), Some(1));
    }

    #[test]
    fn escaping_drops_quotes() {
        assert_eq!(escape("a\"b\\c"), "abc");
    }
}
