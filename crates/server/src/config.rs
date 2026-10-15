//! Server configuration (contracts/configuration.md).
//!
//! Values come from `GAMESHELF_*` environment variables, optionally from a TOML file named by
//! `GAMESHELF_CONFIG`; environment variables win.

use base64::Engine as _;
use figment::{
    Figment,
    providers::{Env, Format, Toml},
};
use secrecy::{ExposeSecret, SecretBox, SecretString};
use serde::{Deserialize, Deserializer};
use std::{net::SocketAddr, path::PathBuf};
use url::Url;

/// Length of the operator key that seals secrets.
pub const SECRET_KEY_LEN: usize = 32;

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("invalid configuration: {0}")]
    Invalid(String),
    #[error(
        "no secret key configured. Set GAMESHELF_SECRET_KEY (or GAMESHELF_SECRET_KEY_FILE) to 32 random bytes \
         encoded in base64, for example the output of `openssl rand -base64 32`"
    )]
    MissingSecretKey,
    #[error("the secret key must be 32 bytes encoded in base64 ({0})")]
    BadSecretKey(String),
    #[error("cannot read the secret key file: {0}")]
    SecretKeyFile(std::io::Error),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    #[default]
    Json,
    Pretty,
}

/// Raw configuration as deserialized from the providers.
#[derive(Debug, Deserialize)]
struct Raw {
    secret_key: Option<SecretString>,
    secret_key_file: Option<PathBuf>,
    #[serde(default = "default_data_dir")]
    data_dir: PathBuf,
    bind: Option<SocketAddr>,
    base_url: Option<Url>,
    #[serde(default, deserialize_with = "comma_list")]
    trusted_proxies: Vec<String>,
    proxy_auth_header: Option<String>,
    #[serde(default = "default_schedule")]
    sync_schedule: String,
    igdb_client_id: Option<String>,
    igdb_client_secret: Option<SecretString>,
    steamgriddb_api_key: Option<SecretString>,
    xbox_client_id: Option<String>,
    #[serde(default = "default_log")]
    log: String,
    #[serde(default)]
    log_format: LogFormat,
    metrics_bind: Option<SocketAddr>,
    #[serde(default)]
    dev_fake_connector: bool,
}

fn default_data_dir() -> PathBuf {
    PathBuf::from("/data")
}

fn default_schedule() -> String {
    "0 4 * * *".to_string()
}

fn default_log() -> String {
    "info".to_string()
}

fn comma_list<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany {
        One(String),
        Many(Vec<String>),
    }
    Ok(match Option::<OneOrMany>::deserialize(d)? {
        None => Vec::new(),
        Some(OneOrMany::One(s)) => s
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect(),
        Some(OneOrMany::Many(v)) => v,
    })
}

/// Validated configuration. Some fields are consumed by later milestones (proxy sign-in, Xbox, metrics).
#[derive(Debug)]
#[allow(dead_code)]
pub struct Config {
    pub secret_key: SecretBox<[u8; SECRET_KEY_LEN]>,
    pub data_dir: PathBuf,
    /// Listen address; `None` means use the Leptos site address.
    pub bind: Option<SocketAddr>,
    pub base_url: Option<Url>,
    pub trusted_proxies: Vec<String>,
    pub proxy_auth_header: Option<String>,
    pub sync_schedule: String,
    pub time_zone: Option<String>,
    pub igdb_client_id: Option<String>,
    pub igdb_client_secret: Option<SecretString>,
    pub steamgriddb_api_key: Option<SecretString>,
    pub xbox_client_id: Option<String>,
    pub log: String,
    pub log_format: LogFormat,
    pub metrics_bind: Option<SocketAddr>,
    pub dev_fake_connector: bool,
}

impl Config {
    /// Loads and validates the configuration from the process environment.
    pub fn load() -> Result<Self, ConfigError> {
        let mut figment = Figment::new();
        if let Ok(path) = std::env::var("GAMESHELF_CONFIG") {
            figment = figment.merge(Toml::file(path));
        }
        figment = figment.merge(Env::prefixed("GAMESHELF_").ignore(&["CONFIG"]));
        Self::from_figment(figment, std::env::var("TZ").ok())
    }

    fn from_figment(figment: Figment, time_zone: Option<String>) -> Result<Self, ConfigError> {
        let raw: Raw = figment
            .extract()
            .map_err(|e| ConfigError::Invalid(e.to_string()))?;
        let secret_key = load_key(raw.secret_key, raw.secret_key_file)?;
        if let Some(url) = &raw.base_url
            && !matches!(url.scheme(), "http" | "https")
        {
            return Err(ConfigError::Invalid(
                "GAMESHELF_BASE_URL must be an http or https URL".into(),
            ));
        }
        Ok(Self {
            secret_key,
            data_dir: raw.data_dir,
            bind: raw.bind,
            base_url: raw.base_url,
            trusted_proxies: raw.trusted_proxies,
            proxy_auth_header: raw.proxy_auth_header.filter(|h| !h.trim().is_empty()),
            sync_schedule: raw.sync_schedule,
            time_zone: time_zone.filter(|t| !t.is_empty()),
            igdb_client_id: raw.igdb_client_id.filter(|s| !s.is_empty()),
            igdb_client_secret: raw.igdb_client_secret,
            steamgriddb_api_key: raw.steamgriddb_api_key,
            xbox_client_id: raw.xbox_client_id.filter(|s| !s.is_empty()),
            log: raw.log,
            log_format: raw.log_format,
            metrics_bind: raw.metrics_bind,
            dev_fake_connector: raw.dev_fake_connector && cfg!(debug_assertions),
        })
    }

    /// Whether cookies must carry the `Secure` attribute.
    pub fn https(&self) -> bool {
        self.base_url.as_ref().is_some_and(|u| u.scheme() == "https")
    }
}

fn load_key(
    inline: Option<SecretString>,
    file: Option<PathBuf>,
) -> Result<SecretBox<[u8; SECRET_KEY_LEN]>, ConfigError> {
    let encoded: SecretString = match (inline, file) {
        (Some(k), _) => k,
        (None, Some(path)) => {
            let text = std::fs::read_to_string(path).map_err(ConfigError::SecretKeyFile)?;
            SecretString::from(text.trim().to_string())
        }
        (None, None) => return Err(ConfigError::MissingSecretKey),
    };
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded.expose_secret().trim())
        .map_err(|e| ConfigError::BadSecretKey(e.to_string()))?;
    let key: [u8; SECRET_KEY_LEN] = bytes
        .try_into()
        .map_err(|v: Vec<u8>| ConfigError::BadSecretKey(format!("got {} bytes", v.len())))?;
    Ok(SecretBox::new(Box::new(key)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use figment::providers::Serialized;

    const KEY: &str = "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=";

    fn figment(pairs: &[(&str, &str)]) -> Figment {
        let map: std::collections::BTreeMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        Figment::new().merge(Serialized::defaults(map))
    }

    #[test]
    fn missing_key_is_refused_with_guidance() {
        let err = Config::from_figment(figment(&[]), None).expect_err("must fail");
        assert!(matches!(err, ConfigError::MissingSecretKey));
        assert!(err.to_string().contains("openssl rand -base64 32"));
    }

    #[test]
    fn short_key_is_refused() {
        let err = Config::from_figment(figment(&[("secret_key", "c2hvcnQ=")]), None).expect_err("must fail");
        assert!(matches!(err, ConfigError::BadSecretKey(_)));
    }

    #[test]
    fn defaults_follow_the_contract() {
        let cfg = Config::from_figment(figment(&[("secret_key", KEY)]), None).expect("valid");
        assert_eq!(cfg.data_dir, PathBuf::from("/data"));
        assert_eq!(cfg.sync_schedule, "0 4 * * *");
        assert_eq!(cfg.log, "info");
        assert_eq!(cfg.log_format, LogFormat::Json);
        assert!(cfg.trusted_proxies.is_empty());
        assert!(!cfg.https());
    }

    #[test]
    fn trusted_proxies_accept_a_comma_list() {
        let cfg = Config::from_figment(
            figment(&[
                ("secret_key", KEY),
                ("trusted_proxies", "192.0.2.0/24, 198.51.100.7/32"),
            ]),
            None,
        )
        .expect("valid");
        assert_eq!(cfg.trusted_proxies, vec!["192.0.2.0/24", "198.51.100.7/32"]);
    }

    #[test]
    fn https_base_url_enables_secure_cookies() {
        let cfg = Config::from_figment(
            figment(&[("secret_key", KEY), ("base_url", "https://gameshelf.example.com")]),
            None,
        )
        .expect("valid");
        assert!(cfg.https());
    }
}
