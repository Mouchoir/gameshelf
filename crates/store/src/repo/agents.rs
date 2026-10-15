//! Agents, pairing codes, agent reports and installs (contracts/agent-api.md, FR-026 to FR-030).
//!
//! Pairing codes and agent tokens are stored as SHA-256 hashes; the clear values are shown once.

use crate::{Result, Store, StoreError, ids::TextId, repo::now};
use base64::Engine;
use gameshelf_core::{
    AgentId, ProfileId,
    agent::{AgentConfig, Heartbeat, SOURCES, SourceConfig, SourceState},
};
use sha2::{Digest, Sha256};
use sqlx::Row;
use std::collections::BTreeMap;
use time::{Duration, OffsetDateTime};

/// How long a pairing code stays valid.
pub const PAIRING_CODE_TTL: Duration = Duration::minutes(10);
/// Letters and digits that cannot be mistaken for one another (no 0, O, 1, I). 32 symbols: one random byte
/// modulo 32 is unbiased.
const CODE_ALPHABET: &[u8; 32] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
/// Reports kept per agent, source and platform.
const REPORTS_KEPT: i64 = 20;
const MAX_EXTRA_PATHS: usize = 8;
const MAX_PATH_LEN: usize = 260;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentStatus {
    Active,
    Paused,
    Revoked,
}

impl AgentStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            AgentStatus::Active => "active",
            AgentStatus::Paused => "paused",
            AgentStatus::Revoked => "revoked",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "active" => Some(AgentStatus::Active),
            "paused" => Some(AgentStatus::Paused),
            "revoked" => Some(AgentStatus::Revoked),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Agent {
    pub id: AgentId,
    pub profile_id: ProfileId,
    pub name: String,
    pub os: String,
    pub version: String,
    pub status: AgentStatus,
    /// Settings chosen in the web app, per source.
    pub sources: BTreeMap<String, SourceConfig>,
    pub scan_hours: u32,
    pub send_install_paths: bool,
    /// As last reported by the agent.
    pub autostart: bool,
    pub paused_locally: bool,
    pub source_states: BTreeMap<String, SourceState>,
    pub paired_at: OffsetDateTime,
    pub last_seen_at: Option<OffsetDateTime>,
}

impl Agent {
    /// What the agent pulls: every known source, with the web app's choices applied.
    pub fn config(&self) -> AgentConfig {
        let mut config = AgentConfig {
            status: self.status.as_str().to_string(),
            scan_hours: self.scan_hours,
            send_install_paths: self.send_install_paths,
            ..AgentConfig::default()
        };
        for (source, value) in &self.sources {
            if let Some(slot) = config.sources.get_mut(source) {
                *slot = value.clone();
            }
        }
        config
    }

    pub fn source_enabled(&self, source: &str) -> bool {
        self.config().sources.get(source).is_some_and(|s| s.enabled)
    }
}

/// Settings the player changes in the web app.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AgentSettings {
    pub scan_hours: u32,
    pub send_install_paths: bool,
    pub sources: BTreeMap<String, SourceConfig>,
}

/// A freshly paired agent with its token, which is never stored in clear.
#[derive(Debug)]
pub struct PairedAgent {
    pub agent: Agent,
    pub token: String,
}

/// A game installed on an agent's machine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewInstall {
    pub platform_game_id: String,
    pub account_external_id: Option<String>,
    pub install_path: Option<String>,
    pub size_bytes: Option<u64>,
}

const COLUMNS: &str = "id, profile_id, name, os, version, status, sources, scan_hours, send_install_paths, autostart, \
    paused_locally, source_states, paired_at, last_seen_at";

fn from_row(r: &sqlx::sqlite::SqliteRow) -> Result<Agent> {
    let status: String = r.try_get("status")?;
    let sources: String = r.try_get("sources")?;
    let states: String = r.try_get("source_states")?;
    Ok(Agent {
        id: AgentId::from_txt(&r.try_get::<String, _>("id")?)?,
        profile_id: ProfileId::from_txt(&r.try_get::<String, _>("profile_id")?)?,
        name: r.try_get("name")?,
        os: r.try_get("os")?,
        version: r.try_get("version")?,
        status: AgentStatus::parse(&status)
            .ok_or_else(|| StoreError::Corrupt(format!("agent status {status:?}")))?,
        sources: serde_json::from_str(&sources).unwrap_or_default(),
        scan_hours: u32::try_from(r.try_get::<i64, _>("scan_hours")?).unwrap_or(6),
        send_install_paths: r.try_get::<i64, _>("send_install_paths")? != 0,
        autostart: r.try_get::<i64, _>("autostart")? != 0,
        paused_locally: r.try_get::<i64, _>("paused_locally")? != 0,
        source_states: serde_json::from_str(&states).unwrap_or_default(),
        paired_at: r.try_get("paired_at")?,
        last_seen_at: r.try_get("last_seen_at")?,
    })
}

fn sha256_hex(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Upper case, without separators or spaces, so `k7q2 m9xd` and `K7Q2-M9XD` are the same code.
fn normalize_code(code: &str) -> String {
    code.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

fn random_bytes<const N: usize>() -> Result<[u8; N]> {
    let mut buf = [0u8; N];
    getrandom::fill(&mut buf).map_err(|_| StoreError::invalid("no randomness available"))?;
    Ok(buf)
}

fn check_os(os: &str) -> Result<()> {
    if matches!(os, "windows" | "linux") {
        Ok(())
    } else {
        Err(StoreError::invalid("unsupported operating system"))
    }
}

fn check_settings(s: &AgentSettings) -> Result<()> {
    if !(1..=168).contains(&s.scan_hours) {
        return Err(StoreError::invalid(
            "the scan interval must be between 1 and 168 hours",
        ));
    }
    for (source, config) in &s.sources {
        if !SOURCES.iter().any(|(id, _)| id == source) {
            return Err(StoreError::invalid(format!("unknown source {source:?}")));
        }
        if config.paths.len() > MAX_EXTRA_PATHS || config.paths.iter().any(|p| p.len() > MAX_PATH_LEN) {
            return Err(StoreError::invalid("too many or too long folders"));
        }
    }
    Ok(())
}

impl Store {
    /// Creates a single-use pairing code for `profile`, valid [`PAIRING_CODE_TTL`]. Returns it formatted for
    /// display (`K7Q2-M9XD`) with its expiry.
    pub async fn create_pairing_code(&self, profile: ProfileId) -> Result<(String, OffsetDateTime)> {
        let bytes = random_bytes::<8>()?;
        let code: String = bytes
            .iter()
            .map(|b| char::from(CODE_ALPHABET[usize::from(*b % 32)]))
            .collect();
        let expires = now() + PAIRING_CODE_TTL;
        sqlx::query("DELETE FROM pairing_codes WHERE expires_at < ?")
            .bind(now() - Duration::days(1))
            .execute(self.pool())
            .await?;
        sqlx::query("INSERT INTO pairing_codes (code_hash, profile_id, expires_at) VALUES (?, ?, ?)")
            .bind(sha256_hex(&code))
            .bind(profile.txt())
            .bind(expires)
            .execute(self.pool())
            .await?;
        Ok((format!("{}-{}", &code[..4], &code[4..]), expires))
    }

    /// Redeems a pairing code and registers the agent. `None` when the code is unknown, expired or used; the
    /// caller must not say which.
    pub async fn pair_agent(
        &self,
        code: &str,
        name: &str,
        os: &str,
        version: &str,
    ) -> Result<Option<PairedAgent>> {
        check_os(os)?;
        let name: String = name.trim().chars().take(64).collect();
        if name.is_empty() {
            return Err(StoreError::invalid("the agent needs a name"));
        }
        let code_hash = sha256_hex(&normalize_code(code));
        let now = now();
        let mut tx = self.pool().begin().await?;
        let row =
            sqlx::query("SELECT profile_id, expires_at, used_at FROM pairing_codes WHERE code_hash = ?")
                .bind(&code_hash)
                .fetch_optional(&mut *tx)
                .await?;
        let Some(row) = row else { return Ok(None) };
        let expires: OffsetDateTime = row.try_get("expires_at")?;
        let used: Option<OffsetDateTime> = row.try_get("used_at")?;
        if used.is_some() || expires <= now {
            return Ok(None);
        }
        let claimed =
            sqlx::query("UPDATE pairing_codes SET used_at = ? WHERE code_hash = ? AND used_at IS NULL")
                .bind(now)
                .bind(&code_hash)
                .execute(&mut *tx)
                .await?;
        if claimed.rows_affected() != 1 {
            return Ok(None);
        }
        let profile = ProfileId::from_txt(&row.try_get::<String, _>("profile_id")?)?;
        let token = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(random_bytes::<32>()?);
        let id = AgentId::new();
        sqlx::query(
            "INSERT INTO agents (id, profile_id, name, os, version, token_hash, status, paired_at, last_seen_at)
             VALUES (?, ?, ?, ?, ?, ?, 'active', ?, ?)",
        )
        .bind(id.txt())
        .bind(profile.txt())
        .bind(&name)
        .bind(os)
        .bind(version.chars().take(32).collect::<String>())
        .bind(sha256_hex(&token))
        .bind(now)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        let agent = self.get_agent(id).await?.ok_or(StoreError::NotFound)?;
        Ok(Some(PairedAgent { agent, token }))
    }

    /// The agent holding `token`, whatever its status.
    pub async fn agent_by_token(&self, token: &str) -> Result<Option<Agent>> {
        let row = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT {COLUMNS} FROM agents WHERE token_hash = ?"
        )))
        .bind(sha256_hex(token.trim()))
        .fetch_optional(self.pool())
        .await?;
        row.as_ref().map(from_row).transpose()
    }

    pub async fn get_agent(&self, id: AgentId) -> Result<Option<Agent>> {
        let row = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT {COLUMNS} FROM agents WHERE id = ?"
        )))
        .bind(id.txt())
        .fetch_optional(self.pool())
        .await?;
        row.as_ref().map(from_row).transpose()
    }

    /// Agents of one profile, most recently paired first.
    pub async fn list_agents(&self, profile: ProfileId) -> Result<Vec<Agent>> {
        let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT {COLUMNS} FROM agents WHERE profile_id = ? ORDER BY paired_at DESC"
        )))
        .bind(profile.txt())
        .fetch_all(self.pool())
        .await?;
        rows.iter().map(from_row).collect()
    }

    /// Active agents of every profile, for silence checks.
    pub async fn list_active_agents(&self) -> Result<Vec<Agent>> {
        let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT {COLUMNS} FROM agents WHERE status = 'active'"
        )))
        .fetch_all(self.pool())
        .await?;
        rows.iter().map(from_row).collect()
    }

    /// Changes settings; only the owner may do it.
    pub async fn update_agent_settings(
        &self,
        id: AgentId,
        owner: ProfileId,
        s: &AgentSettings,
    ) -> Result<()> {
        check_settings(s)?;
        let done = sqlx::query(
            "UPDATE agents SET scan_hours = ?, send_install_paths = ?, sources = ? WHERE id = ? AND profile_id = ?",
        )
        .bind(i64::from(s.scan_hours))
        .bind(s.send_install_paths)
        .bind(serde_json::to_string(&s.sources).unwrap_or_else(|_| "{}".into()))
        .bind(id.txt())
        .bind(owner.txt())
        .execute(self.pool())
        .await?;
        if done.rows_affected() == 0 {
            return Err(StoreError::NotFound);
        }
        Ok(())
    }

    /// Pauses, resumes or revokes an agent. `owner` `None` is the agent acting on itself.
    pub async fn set_agent_status(
        &self,
        id: AgentId,
        owner: Option<ProfileId>,
        status: AgentStatus,
    ) -> Result<()> {
        let done = sqlx::query(
            "UPDATE agents SET status = ? WHERE id = ? AND (? IS NULL OR profile_id = ?) AND status != 'revoked'",
        )
        .bind(status.as_str())
        .bind(id.txt())
        .bind(owner.map(|o| o.txt()))
        .bind(owner.map(|o| o.txt()))
        .execute(self.pool())
        .await?;
        if done.rows_affected() == 0 {
            return Err(StoreError::NotFound);
        }
        Ok(())
    }

    /// Deletes an agent with its reports and install records. Games it reported stay in the library.
    pub async fn delete_agent(&self, id: AgentId, owner: ProfileId) -> Result<()> {
        let done = sqlx::query("DELETE FROM agents WHERE id = ? AND profile_id = ?")
            .bind(id.txt())
            .bind(owner.txt())
            .execute(self.pool())
            .await?;
        if done.rows_affected() == 0 {
            return Err(StoreError::NotFound);
        }
        Ok(())
    }

    /// Records a contact and the agent's version. Any contact ends a silence.
    pub async fn touch_agent(&self, id: AgentId, version: &str) -> Result<()> {
        sqlx::query("UPDATE agents SET last_seen_at = ?, version = ?, silent_since = NULL WHERE id = ?")
            .bind(now())
            .bind(version.chars().take(32).collect::<String>())
            .bind(id.txt())
            .execute(self.pool())
            .await?;
        Ok(())
    }

    /// Stores what the agent says about itself.
    pub async fn record_heartbeat(&self, id: AgentId, hb: &Heartbeat) -> Result<()> {
        let states: BTreeMap<&String, &SourceState> = hb
            .sources
            .iter()
            .filter(|(s, _)| SOURCES.iter().any(|(id, _)| id == s))
            .collect();
        sqlx::query(
            "UPDATE agents SET last_seen_at = ?, version = ?, autostart = ?, paused_locally = ?, source_states = ?,
                silent_since = NULL
             WHERE id = ?",
        )
        .bind(now())
        .bind(hb.version.chars().take(32).collect::<String>())
        .bind(hb.autostart)
        .bind(hb.paused)
        .bind(serde_json::to_string(&states).unwrap_or_else(|_| "{}".into()))
        .bind(id.txt())
        .execute(self.pool())
        .await?;
        Ok(())
    }

    /// Marks an agent silent. Returns true only the first time for a given silence.
    pub async fn mark_agent_silent(&self, id: AgentId) -> Result<bool> {
        let done = sqlx::query("UPDATE agents SET silent_since = ? WHERE id = ? AND silent_since IS NULL")
            .bind(now())
            .bind(id.txt())
            .execute(self.pool())
            .await?;
        Ok(done.rows_affected() == 1)
    }

    /// Content hash of the last report of one source for one platform.
    pub async fn last_report_hash(
        &self,
        agent: AgentId,
        source: &str,
        platform: &str,
    ) -> Result<Option<String>> {
        Ok(sqlx::query_scalar(
            "SELECT content_hash FROM agent_reports WHERE agent_id = ? AND source = ? AND platform = ?
             ORDER BY received_at DESC LIMIT 1",
        )
        .bind(agent.txt())
        .bind(source)
        .bind(platform)
        .fetch_optional(self.pool())
        .await?)
    }

    /// Records a report and keeps only the most recent ones.
    pub async fn record_report(
        &self,
        agent: AgentId,
        source: &str,
        platform: &str,
        content_hash: &str,
        summary: &serde_json::Value,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO agent_reports (id, agent_id, source, platform, content_hash, received_at, summary)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(uuid::Uuid::now_v7().hyphenated().to_string())
        .bind(agent.txt())
        .bind(source)
        .bind(platform)
        .bind(content_hash)
        .bind(now())
        .bind(summary.to_string())
        .execute(self.pool())
        .await?;
        sqlx::query(
            "DELETE FROM agent_reports WHERE agent_id = ? AND source = ? AND platform = ? AND id NOT IN (
                SELECT id FROM agent_reports WHERE agent_id = ? AND source = ? AND platform = ?
                ORDER BY received_at DESC LIMIT ?)",
        )
        .bind(agent.txt())
        .bind(source)
        .bind(platform)
        .bind(agent.txt())
        .bind(source)
        .bind(platform)
        .bind(REPORTS_KEPT)
        .execute(self.pool())
        .await?;
        Ok(())
    }

    /// Replaces the games installed on an agent's machine for one platform.
    pub async fn replace_installs(
        &self,
        agent: AgentId,
        platform: &str,
        installs: &[NewInstall],
    ) -> Result<()> {
        let now = now();
        let mut tx = self.pool().begin().await?;
        sqlx::query("DELETE FROM installs WHERE agent_id = ? AND platform = ?")
            .bind(agent.txt())
            .bind(platform)
            .execute(&mut *tx)
            .await?;
        for i in installs {
            sqlx::query(
                "INSERT OR REPLACE INTO installs (id, agent_id, platform, platform_game_id, account_external_id,
                    install_path, size_bytes, last_seen_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(uuid::Uuid::now_v7().hyphenated().to_string())
            .bind(agent.txt())
            .bind(platform)
            .bind(&i.platform_game_id)
            .bind(&i.account_external_id)
            .bind(&i.install_path)
            .bind(i.size_bytes.and_then(|s| i64::try_from(s).ok()))
            .bind(now)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Number of install records of an agent.
    pub async fn count_installs(&self, agent: AgentId) -> Result<i64> {
        Ok(
            sqlx::query_scalar("SELECT count(*) FROM installs WHERE agent_id = ?")
                .bind(agent.txt())
                .fetch_one(self.pool())
                .await?,
        )
    }

    /// Names of the agent machines where a game is installed, for its owner only.
    pub async fn installed_on(
        &self,
        profile: ProfileId,
        game: gameshelf_core::GameId,
    ) -> Result<Vec<String>> {
        Ok(sqlx::query_scalar(
            "SELECT DISTINCT a.name FROM installs i
               JOIN agents a ON a.id = i.agent_id
               JOIN platform_products p ON p.platform = i.platform AND p.platform_game_id = i.platform_game_id
              WHERE a.profile_id = ? AND p.game_id = ? ORDER BY a.name",
        )
        .bind(profile.txt())
        .bind(game.txt())
        .fetch_all(self.pool())
        .await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_normalized() {
        assert_eq!(normalize_code(" k7q2-m9xd "), "K7Q2M9XD");
        assert_eq!(sha256_hex("abc").len(), 64);
    }

    #[test]
    fn settings_are_checked() {
        let mut s = AgentSettings {
            scan_hours: 6,
            send_install_paths: false,
            sources: BTreeMap::new(),
        };
        assert!(check_settings(&s).is_ok());
        s.scan_hours = 0;
        assert!(check_settings(&s).is_err());
        s.scan_hours = 6;
        s.sources.insert("nope".into(), SourceConfig::default());
        assert!(check_settings(&s).is_err());
    }

    async fn store_with_profile() -> (Store, ProfileId) {
        let store = Store::open_in_memory(&secrecy::SecretBox::new(Box::new([4u8; 32])))
            .await
            .expect("store");
        let pw = secrecy::SecretString::from("a long enough password".to_string());
        let profile = store
            .create_profile(crate::NewProfile {
                username: "player-one",
                display_name: "Player One",
                email: None,
                password: &pw,
                role: gameshelf_core::Role::Admin,
            })
            .await
            .expect("profile")
            .id;
        (store, profile)
    }

    #[tokio::test]
    async fn pairing_codes_are_single_use_and_tokens_identify_the_agent() {
        let (store, profile) = store_with_profile().await;
        let (code, expires) = store.create_pairing_code(profile).await.expect("code");
        assert_eq!(code.len(), 9);
        assert!(expires > now());
        assert!(
            store
                .pair_agent("ZZZZ-ZZZZ", "PC", "windows", "0.1.0")
                .await
                .expect("db")
                .is_none()
        );
        let paired = store
            .pair_agent(&code.to_lowercase().replace('-', " "), "PC", "windows", "0.1.0")
            .await
            .expect("db")
            .expect("paired");
        assert_eq!(paired.agent.profile_id, profile);
        assert!(paired.token.len() >= 43);
        assert!(
            store
                .pair_agent(&code, "PC 2", "linux", "0.1.0")
                .await
                .expect("db")
                .is_none(),
            "single use"
        );

        let by_token = store
            .agent_by_token(&paired.token)
            .await
            .expect("db")
            .expect("agent");
        assert_eq!(by_token.id, paired.agent.id);
        assert!(store.agent_by_token("not-a-token").await.expect("db").is_none());
        let stored: String = sqlx::query_scalar("SELECT token_hash FROM agents")
            .fetch_one(store.pool())
            .await
            .expect("q");
        assert_ne!(stored, paired.token, "tokens are stored hashed");

        store
            .set_agent_status(paired.agent.id, Some(profile), AgentStatus::Revoked)
            .await
            .expect("revoke");
        let revoked = store
            .get_agent(paired.agent.id)
            .await
            .expect("db")
            .expect("agent");
        assert_eq!(revoked.config().status, "revoked");
        assert!(
            store
                .set_agent_status(paired.agent.id, None, AgentStatus::Active)
                .await
                .is_err(),
            "a revoked agent cannot reactivate itself"
        );
    }

    #[tokio::test]
    async fn silence_is_reported_once_until_the_next_contact() {
        let (store, profile) = store_with_profile().await;
        let (code, _) = store.create_pairing_code(profile).await.expect("code");
        let agent = store
            .pair_agent(&code, "PC", "linux", "0.1.0")
            .await
            .expect("db")
            .expect("paired")
            .agent;
        assert!(store.mark_agent_silent(agent.id).await.expect("db"));
        assert!(
            !store.mark_agent_silent(agent.id).await.expect("db"),
            "once per silence"
        );
        store.touch_agent(agent.id, "0.1.0").await.expect("touch");
        assert!(
            store.mark_agent_silent(agent.id).await.expect("db"),
            "a new silence after a contact"
        );
    }

    #[tokio::test]
    async fn expired_codes_are_refused() {
        let (store, profile) = store_with_profile().await;
        let (code, _) = store.create_pairing_code(profile).await.expect("code");
        sqlx::query("UPDATE pairing_codes SET expires_at = ?")
            .bind(now() - Duration::minutes(1))
            .execute(store.pool())
            .await
            .expect("expire");
        assert!(
            store
                .pair_agent(&code, "PC", "windows", "0.1.0")
                .await
                .expect("db")
                .is_none()
        );
        assert!(store.pair_agent(&code, "PC", "macos", "0.1.0").await.is_err());
    }

    #[tokio::test]
    async fn heartbeats_keep_only_known_sources() {
        let (store, profile) = store_with_profile().await;
        let (code, _) = store.create_pairing_code(profile).await.expect("code");
        let agent = store
            .pair_agent(&code, "PC", "linux", "0.1.0")
            .await
            .expect("db")
            .expect("paired")
            .agent;
        let mut sources = BTreeMap::new();
        sources.insert(
            "steam-local".to_string(),
            SourceState {
                enabled: true,
                detected: true,
                last_scan_at: None,
            },
        );
        sources.insert("bogus".to_string(), SourceState::default());
        store
            .record_heartbeat(
                agent.id,
                &Heartbeat {
                    version: "0.2.0".into(),
                    autostart: true,
                    paused: false,
                    sources,
                },
            )
            .await
            .expect("heartbeat");
        let agent = store.get_agent(agent.id).await.expect("db").expect("agent");
        assert_eq!(agent.version, "0.2.0");
        assert!(agent.autostart);
        assert_eq!(
            agent.source_states.keys().collect::<Vec<_>>(),
            vec!["steam-local"]
        );
    }
}
