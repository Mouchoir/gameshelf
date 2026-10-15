//! Server functions. Their bodies only compile on the server (`ssr`); the browser gets HTTP stubs.

use crate::types::*;
use gameshelf_core::{AccountId, GameId, view::LibraryPage};
use leptos::prelude::*;
use leptos::server_fn::codec::Json;
use std::collections::HashMap;

#[cfg(feature = "ssr")]
use crate::server::auth::{require_profile, state};

#[cfg(feature = "ssr")]
fn internal(e: impl std::fmt::Display) -> ServerFnError {
    tracing::error!(error = %e, "server function failed");
    ServerFnError::new("internal error")
}

#[cfg(feature = "ssr")]
fn me(p: &gameshelf_store::Profile) -> Me {
    Me {
        id: p.id,
        username: p.username.clone(),
        display_name: p.display_name.clone(),
        role: p.role,
        locale: p.locale,
    }
}

/// Who is signed in, and whether the instance still needs its first admin.
#[server]
pub async fn get_auth_state() -> Result<AuthState, ServerFnError> {
    let st = state()?;
    let needs_setup = st.store.count_profiles().await.map_err(internal)? == 0;
    let profile = crate::server::auth::current_profile().await?;
    Ok(AuthState {
        me: profile.as_ref().map(me),
        needs_setup,
    })
}

/// Creates the first admin profile and signs it in. Refused once any profile exists.
#[server]
pub async fn setup_instance(
    username: String,
    display_name: String,
    password: String,
) -> Result<(), ServerFnError> {
    let st = state()?;
    if st.store.count_profiles().await.map_err(internal)? > 0 {
        return Err(ServerFnError::new("already set up"));
    }
    let password = secrecy::SecretString::from(password);
    let profile = st
        .store
        .create_profile(gameshelf_store::NewProfile {
            username: username.trim(),
            display_name: &display_name,
            email: None,
            password: &password,
            role: gameshelf_core::Role::Admin,
        })
        .await
        .map_err(|e| match e {
            gameshelf_store::StoreError::Invalid(msg) => ServerFnError::new(msg),
            other => internal(other),
        })?;
    crate::server::auth::sign_in(&profile).await?;
    leptos_axum::redirect("/");
    Ok(())
}

/// Signs in. Returns false for a wrong password or when too many attempts were made.
#[server]
pub async fn login(username: String, password: String) -> Result<bool, ServerFnError> {
    let st = state()?;
    let username = username.trim().to_lowercase();
    let client = crate::server::auth::client_key().await;
    if !st.allow_login_attempt(&[format!("ip:{client}"), format!("user:{username}")]) {
        return Ok(false);
    }
    let password = secrecy::SecretString::from(password);
    match st
        .store
        .verify_login(&username, &password)
        .await
        .map_err(internal)?
    {
        Some(profile) => {
            crate::server::auth::sign_in(&profile).await?;
            leptos_axum::redirect("/");
            Ok(true)
        }
        None => Ok(false),
    }
}

#[server]
pub async fn logout() -> Result<(), ServerFnError> {
    crate::server::auth::sign_out().await?;
    leptos_axum::redirect("/login");
    Ok(())
}

/// Every platform of the registry with its tier and connection method (FR-002).
#[server]
pub async fn platforms() -> Result<Vec<PlatformView>, ServerFnError> {
    use gameshelf_connectors::AuthMethod;
    require_profile().await?;
    let st = state()?;
    let registry = st.syncer.registry();
    Ok(gameshelf_core::platform::PLATFORMS
        .iter()
        .filter(|p| p.key != "manual")
        .map(|p| {
            let connector = registry.get(p.key);
            let auth = connector
                .as_ref()
                .and_then(|c| c.descriptor().auth.first().copied());
            let (fields, help_url, auth_kind, login_url, capture) = match auth {
                Some(AuthMethod::ApiKey { fields, help_url }) => (
                    fields
                        .iter()
                        .map(|f| FieldView {
                            name: f.name.into(),
                            label_key: f.label_key.into(),
                            secret: f.secret,
                        })
                        .collect(),
                    Some(help_url.to_string()),
                    "api_key",
                    None,
                    None,
                ),
                Some(AuthMethod::BrowserLogin { login_url, capture }) => (
                    Vec::new(),
                    None,
                    "browser_login",
                    Some(login_url.to_string()),
                    Some(
                        match capture {
                            gameshelf_connectors::CodeCapture::JsonPage { .. } => "json_page",
                            _ => "redirect_query",
                        }
                        .to_string(),
                    ),
                ),
                _ => (Vec::new(), None, "none", None, None),
            };
            PlatformView {
                key: p.key.into(),
                name: p.name.into(),
                tier: p.tier,
                icon: p.icon.into(),
                connectable: connector.is_some(),
                agent_only: !p.remote && p.local,
                fields,
                help_url,
                auth_kind: auth_kind.to_string(),
                login_url,
                capture,
            }
        })
        .collect())
}

/// Validates the fields with the platform, connects the account and starts its first import.
#[server(input = Json)]
pub async fn connect_with_key(
    platform: String,
    fields: HashMap<String, String>,
    label: String,
) -> Result<ConnectResult, ServerFnError> {
    use gameshelf_sync::ConnectError;
    let profile = require_profile().await?;
    let st = state()?;
    let payload = gameshelf_connectors::CredentialPayload(serde_json::to_value(&fields).map_err(internal)?);
    let label = Some(label.trim()).filter(|l| !l.is_empty());
    match st
        .syncer
        .connect(profile.id, &platform, payload, label)
        .await
        .map_err(internal)?
    {
        Ok(account) => {
            st.spawn_sync(account, gameshelf_store::RunTrigger::Manual);
            Ok(ConnectResult::Connected(account))
        }
        Err(ConnectError::Rejected(e)) => Ok(ConnectResult::Rejected(e.to_string())),
        Err(ConnectError::Taken(holder)) => Ok(ConnectResult::Taken(holder)),
        Err(ConnectError::UnknownPlatform) => Ok(ConnectResult::Unknown),
        Err(ConnectError::NoCode) => Ok(ConnectResult::NoCode),
    }
}

/// Connects an account from what the player pasted after signing in on the platform (FR-003).
#[server]
pub async fn connect_with_code(
    platform: String,
    pasted: String,
    label: String,
) -> Result<ConnectResult, ServerFnError> {
    use gameshelf_sync::ConnectError;
    let profile = require_profile().await?;
    let st = state()?;
    let label = Some(label.trim()).filter(|l| !l.is_empty());
    match st
        .syncer
        .connect_with_code(profile.id, &platform, &pasted, label)
        .await
        .map_err(internal)?
    {
        Ok(account) => {
            st.spawn_sync(account, gameshelf_store::RunTrigger::Manual);
            Ok(ConnectResult::Connected(account))
        }
        Err(ConnectError::Rejected(e)) => Ok(ConnectResult::Rejected(e.to_string())),
        Err(ConnectError::Taken(holder)) => Ok(ConnectResult::Taken(holder)),
        Err(ConnectError::UnknownPlatform) => Ok(ConnectResult::Unknown),
        Err(ConnectError::NoCode) => Ok(ConnectResult::NoCode),
    }
}

/// The signed-in profile's accounts with their health.
#[server]
pub async fn accounts() -> Result<Vec<AccountView>, ServerFnError> {
    let profile = require_profile().await?;
    let st = state()?;
    let mut out = Vec::new();
    for a in st.store.list_accounts(profile.id).await.map_err(internal)? {
        let games = st
            .store
            .count_available_ownerships(a.id)
            .await
            .map_err(internal)?;
        let last = st
            .store
            .list_runs(a.id, 1)
            .await
            .map_err(internal)?
            .into_iter()
            .next();
        let last_error = last
            .filter(|r| r.error_kind.is_some())
            .and_then(|r| r.error_message);
        let credential_expires_at = st
            .store
            .credential_meta(a.id)
            .await
            .map_err(internal)?
            .and_then(|m| m.expires_at);
        out.push(AccountView {
            id: a.id,
            platform_name: gameshelf_core::platform::platform(&a.platform)
                .map(|p| p.name)
                .unwrap_or("?")
                .to_string(),
            platform: a.platform,
            label: a.label,
            display_name: a.display_name,
            status: a.status,
            games,
            last_sync_at: a.last_sync_at,
            credential_expiring: credential_expires_at
                .is_some_and(|t| t - time::OffsetDateTime::now_utc() < time::Duration::days(7)),
            credential_expires_at,
            next_sync_at: if a.status.syncs() && a.sync_enabled {
                st.next_scheduled()
            } else {
                None
            },
            last_error,
            syncing: st.is_syncing(a.id),
            via_agent: a.agent_id.is_some(),
        });
    }
    Ok(out)
}

/// Starts a sync of one account (FR-020).
#[server]
pub async fn sync_now(account: AccountId) -> Result<bool, ServerFnError> {
    let profile = require_profile().await?;
    let st = state()?;
    let owned = st
        .store
        .get_account(account)
        .await
        .map_err(internal)?
        .is_some_and(|a| a.profile_id == profile.id);
    if !owned {
        return Err(ServerFnError::new("not found"));
    }
    Ok(st.spawn_sync(account, gameshelf_store::RunTrigger::Manual))
}

/// Starts a sync of every account of the profile.
#[server]
pub async fn sync_all() -> Result<u32, ServerFnError> {
    let profile = require_profile().await?;
    let st = state()?;
    let mut started = 0;
    for a in st.store.list_accounts(profile.id).await.map_err(internal)? {
        if a.status.syncs() && st.spawn_sync(a.id, gameshelf_store::RunTrigger::Manual) {
            started += 1;
        }
    }
    Ok(started)
}

#[server]
pub async fn rename_account(account: AccountId, label: String) -> Result<(), ServerFnError> {
    let profile = require_profile().await?;
    state()?
        .store
        .set_account_label(account, profile.id, &label)
        .await
        .map_err(internal)
}

/// Disconnects an account (FR-004).
#[server]
pub async fn disconnect_account(account: AccountId, delete_data: bool) -> Result<(), ServerFnError> {
    let profile = require_profile().await?;
    let st = state()?;
    st.store
        .disconnect_account(account, profile.id, delete_data)
        .await
        .map_err(internal)?;
    if delete_data {
        let _ = st.store.delete_orphan_games().await;
        st.rebuild_search().await;
    }
    Ok(())
}

/// The library grid, optionally narrowed by a search (FR-011 to FR-014).
#[server(input = Json)]
pub async fn library(req: LibraryRequest) -> Result<LibraryPage, ServerFnError> {
    let profile = require_profile().await?;
    let st = state()?;
    let mut query = req.query;
    let search = req.search.trim();
    if search.is_empty() {
        return st.store.library(profile.id, &query).await.map_err(internal);
    }
    let hits = st
        .search
        .read()
        .map(|idx| idx.search(search, 300))
        .unwrap_or_default();
    query.games = Some(hits.clone());
    query.offset = 0;
    query.limit = 300;
    let mut page = st.store.library(profile.id, &query).await.map_err(internal)?;
    // Keep the relevance order of the search.
    let rank: HashMap<GameId, usize> = hits.iter().enumerate().map(|(i, g)| (*g, i)).collect();
    page.games
        .sort_by_key(|g| rank.get(&g.game_id).copied().unwrap_or(usize::MAX));
    Ok(page)
}

/// A game with every ownership the viewer may see (FR-016). `None` when the viewer cannot see it.
#[server]
pub async fn fetch_game_detail(game: GameId) -> Result<Option<GameDetail>, ServerFnError> {
    let profile = require_profile().await?;
    let st = state()?;
    let ownerships = st
        .store
        .game_ownerships(profile.id, game)
        .await
        .map_err(internal)?;
    let dlc_rows = st.store.dlc_of(game).await.map_err(internal)?;
    let Some(summary) = st.store.game_summary(game).await.map_err(internal)? else {
        return Ok(None);
    };
    if ownerships.is_empty() {
        return Ok(None);
    }
    let mut names: HashMap<gameshelf_core::ProfileId, String> = HashMap::new();
    let mut rows = Vec::with_capacity(ownerships.len());
    for o in ownerships {
        let owner_name = if o.mine {
            None
        } else {
            if !names.contains_key(&o.owner_profile_id)
                && let Some(p) = st.store.get_profile(o.owner_profile_id).await.map_err(internal)?
            {
                names.insert(p.id, p.display_name);
            }
            names.get(&o.owner_profile_id).cloned()
        };
        rows.push(OwnershipRow {
            ownership: o,
            owner_name,
        });
    }
    let accounts: Vec<AccountId> = rows
        .iter()
        .filter(|r| r.ownership.mine)
        .map(|r| r.ownership.account_id)
        .collect();
    let enrichment = st
        .store
        .enrichment_for(&accounts, game)
        .await
        .map_err(internal)?
        .into_iter()
        .map(|e| EnrichmentRow {
            account_id: e.account_id,
            key: e.key,
            value: match e.value {
                serde_json::Value::String(s) => s,
                other => other.to_string(),
            },
            refreshed_at: e.refreshed_at,
        })
        .collect();
    let hidden_from_family = st
        .store
        .list_exclusions(profile.id)
        .await
        .map_err(internal)?
        .iter()
        .any(|e| {
            e.audience.is_none()
                && e.kind == gameshelf_store::ExclusionKind::Game
                && e.reference == game.to_string()
        });
    Ok(Some(GameDetail {
        hidden_from_family,
        summary,
        ownerships: rows,
        dlc: dlc_rows,
        enrichment,
    }))
}

/// Latest activity of the signed-in profile (FR-024).
#[server]
pub async fn activity() -> Result<Vec<ActivityRow>, ServerFnError> {
    let profile = require_profile().await?;
    let st = state()?;
    let mut out = Vec::new();
    for e in st.store.list_activity(profile.id, 100).await.map_err(internal)? {
        let game_title = match e.game_id {
            Some(g) => st.store.game_summary(g).await.map_err(internal)?.map(|s| s.title),
            None => None,
        };
        let account = match e.account_id {
            Some(a) => st.store.get_account(a).await.map_err(internal)?,
            None => None,
        };
        out.push(ActivityRow {
            kind: e.kind.as_str().to_string(),
            game_id: e.game_id,
            game_title,
            account_label: account.as_ref().map(|a| a.label.clone()),
            platform: account.map(|a| a.platform),
            created_at: e.created_at,
        });
    }
    let _ = st.store.mark_activity_read(profile.id).await;
    Ok(out)
}

/// The profile's notification targets (US7).
#[server]
pub async fn notification_targets() -> Result<Vec<TargetView>, ServerFnError> {
    let profile = require_profile().await?;
    let targets = state()?.store.list_targets(profile.id).await.map_err(internal)?;
    Ok(targets
        .into_iter()
        .map(|t| TargetView {
            id: t.id,
            kind: t.kind,
            name: t.name,
            events: t.events,
            enabled: t.enabled,
            last_sent_at: t.last_sent_at,
            last_error: t.last_error,
        })
        .collect())
}

/// Adds a target. Returns an error message for invalid settings.
#[server]
pub async fn add_target(
    kind: String,
    name: String,
    address: String,
    topic: String,
    token: String,
) -> Result<Option<String>, ServerFnError> {
    use gameshelf_notify::TargetConfig;
    let profile = require_profile().await?;
    let token = Some(token.trim().to_string()).filter(|t| !t.is_empty());
    let config = match kind.as_str() {
        "discord" => TargetConfig::Discord {
            webhook_url: address.trim().to_string(),
        },
        "ntfy" => TargetConfig::Ntfy {
            server: address.trim().to_string(),
            topic: topic.trim().to_string(),
            token,
        },
        "gotify" => TargetConfig::Gotify {
            server: address.trim().to_string(),
            token: token.unwrap_or_default(),
        },
        _ => return Ok(Some("unknown kind".into())),
    };
    if let Err(e) = config.validate() {
        return Ok(Some(e.to_string()));
    }
    let bytes = serde_json::to_vec(&config).map_err(internal)?;
    let events: Vec<String> = crate::server::notifications::EVENT_KINDS
        .iter()
        .map(|s| s.to_string())
        .collect();
    match state()?
        .store
        .create_target(profile.id, config.kind().as_str(), &name, &bytes, &events)
        .await
    {
        Ok(_) => Ok(None),
        Err(gameshelf_store::StoreError::Invalid(msg)) => Ok(Some(msg)),
        Err(e) => Err(internal(e)),
    }
}

#[server]
pub async fn toggle_target_event(target: String, event: String, on: bool) -> Result<(), ServerFnError> {
    let profile = require_profile().await?;
    let st = state()?;
    let Some(t) = st
        .store
        .list_targets(profile.id)
        .await
        .map_err(internal)?
        .into_iter()
        .find(|t| t.id == target)
    else {
        return Err(ServerFnError::new("not found"));
    };
    let mut events = t.events;
    events.retain(|e| e != &event);
    if on && crate::server::notifications::EVENT_KINDS.contains(&event.as_str()) {
        events.push(event);
    }
    st.store
        .set_target_events(profile.id, &target, &events)
        .await
        .map_err(internal)
}

#[server]
pub async fn set_target_enabled(target: String, enabled: bool) -> Result<(), ServerFnError> {
    let profile = require_profile().await?;
    state()?
        .store
        .set_target_enabled(profile.id, &target, enabled)
        .await
        .map_err(internal)
}

#[server]
pub async fn delete_target(target: String) -> Result<(), ServerFnError> {
    let profile = require_profile().await?;
    state()?
        .store
        .delete_target(profile.id, &target)
        .await
        .map_err(internal)
}

/// Sends a test message (FR-036). Returns the error text when it fails.
#[server]
pub async fn test_target(target: String) -> Result<Option<String>, ServerFnError> {
    let profile = require_profile().await?;
    let st = state()?;
    Ok(crate::server::notifications::send_test(&st, profile.id, &target)
        .await
        .err())
}

/// Remembers the interface language of the profile, also used for notifications (FR-045).
#[server]
pub async fn set_language(locale: String) -> Result<(), ServerFnError> {
    let profile = require_profile().await?;
    let pref = gameshelf_core::LocalePref::parse(&locale).unwrap_or_default();
    state()?
        .store
        .set_locale(profile.id, pref)
        .await
        .map_err(internal)
}

#[cfg(feature = "ssr")]
async fn require_admin() -> Result<gameshelf_store::Profile, ServerFnError> {
    let p = require_profile().await?;
    if p.role != gameshelf_core::Role::Admin {
        return Err(ServerFnError::new("not allowed"));
    }
    Ok(p)
}

#[cfg(feature = "ssr")]
fn store_error(e: gameshelf_store::StoreError) -> ServerFnError {
    use gameshelf_store::StoreError as E;
    match e {
        E::Invalid(msg) => ServerFnError::new(msg),
        E::LastAdmin => ServerFnError::new("the last active administrator cannot be removed or disabled"),
        E::ProfileExists => ServerFnError::new("a profile with this username already exists"),
        other => internal(other),
    }
}

/// Every profile, for administrators (FR-031). Never exposes libraries or credentials.
#[server]
pub async fn admin_profiles() -> Result<Vec<ProfileAdminView>, ServerFnError> {
    require_admin().await?;
    Ok(state()?
        .store
        .list_profiles()
        .await
        .map_err(internal)?
        .into_iter()
        .map(|p| ProfileAdminView {
            id: p.id,
            username: p.username,
            display_name: p.display_name,
            role: p.role,
            disabled: p.disabled,
        })
        .collect())
}

#[server]
pub async fn admin_create_profile(
    username: String,
    display_name: String,
    password: String,
    admin: Option<String>,
) -> Result<(), ServerFnError> {
    require_admin().await?;
    let password = secrecy::SecretString::from(password);
    let role = if admin.is_some() {
        gameshelf_core::Role::Admin
    } else {
        gameshelf_core::Role::Member
    };
    state()?
        .store
        .create_profile(gameshelf_store::NewProfile {
            username: username.trim(),
            display_name: &display_name,
            email: None,
            password: &password,
            role,
        })
        .await
        .map(|_| ())
        .map_err(store_error)
}

#[server]
pub async fn admin_set_disabled(
    profile: gameshelf_core::ProfileId,
    disabled: bool,
) -> Result<(), ServerFnError> {
    require_admin().await?;
    state()?
        .store
        .set_disabled(profile, disabled)
        .await
        .map_err(store_error)
}

#[server]
pub async fn admin_delete_profile(profile: gameshelf_core::ProfileId) -> Result<(), ServerFnError> {
    let me = require_admin().await?;
    if me.id == profile {
        return Err(ServerFnError::new("you cannot delete your own profile here"));
    }
    let st = state()?;
    st.store.delete_profile(profile).await.map_err(store_error)?;
    let _ = st.store.delete_orphan_games().await;
    st.rebuild_search().await;
    Ok(())
}

/// Sets a new password and signs the profile out everywhere.
#[server]
pub async fn admin_reset_password(
    profile: gameshelf_core::ProfileId,
    password: String,
) -> Result<(), ServerFnError> {
    require_admin().await?;
    let password = secrecy::SecretString::from(password);
    state()?
        .store
        .set_password(profile, &password)
        .await
        .map_err(store_error)
}

/// Sharing settings of the signed-in profile (FR-032).
#[server]
pub async fn sharing_settings() -> Result<SharingView, ServerFnError> {
    use gameshelf_store::ExclusionKind;
    let me = require_profile().await?;
    let st = state()?;
    let shared = st.store.shared_with(me.id).await.map_err(internal)?;
    let others = st
        .store
        .list_profiles()
        .await
        .map_err(internal)?
        .into_iter()
        .filter(|p| p.id != me.id && !p.disabled)
        .map(|p| AudienceView {
            shared: shared.contains(&p.id),
            id: p.id,
            display_name: p.display_name,
        })
        .collect();
    let exclusions: Vec<_> = st
        .store
        .list_exclusions(me.id)
        .await
        .map_err(internal)?
        .into_iter()
        .filter(|e| e.audience.is_none())
        .collect();
    let hidden = |kind: ExclusionKind, r: &str| exclusions.iter().any(|e| e.kind == kind && e.reference == r);
    let accounts_list = st.store.list_accounts(me.id).await.map_err(internal)?;
    let mut platform_keys: Vec<String> = accounts_list.iter().map(|a| a.platform.clone()).collect();
    platform_keys.sort();
    platform_keys.dedup();
    let platforms = platform_keys
        .into_iter()
        .map(|k| HideablePlatform {
            name: gameshelf_core::platform::platform(&k)
                .map(|p| p.name)
                .unwrap_or("?")
                .to_string(),
            hidden: hidden(ExclusionKind::Platform, &k),
            key: k,
        })
        .collect();
    let accounts = accounts_list
        .iter()
        .map(|a| HideableAccount {
            id: a.id,
            platform: a.platform.clone(),
            label: a.label.clone(),
            hidden: hidden(ExclusionKind::Account, &a.id.to_string()),
        })
        .collect();
    let mut hidden_games = Vec::new();
    for e in exclusions.iter().filter(|e| e.kind == ExclusionKind::Game) {
        if let Ok(id) = e.reference.parse::<GameId>()
            && let Some(s) = st.store.game_summary(id).await.map_err(internal)?
        {
            hidden_games.push(HiddenGame { id, title: s.title });
        }
    }
    Ok(SharingView {
        others,
        platforms,
        accounts,
        hidden_games,
    })
}

#[server]
pub async fn set_sharing(audience: gameshelf_core::ProfileId, shared: bool) -> Result<(), ServerFnError> {
    let me = require_profile().await?;
    state()?
        .store
        .set_sharing(me.id, audience, shared)
        .await
        .map_err(store_error)
}

/// Hides (or shows again) a platform, an account or a game from every profile the viewer shares with.
#[server]
pub async fn set_exclusion(kind: String, reference: String, hidden: bool) -> Result<(), ServerFnError> {
    use gameshelf_store::ExclusionKind;
    let me = require_profile().await?;
    let st = state()?;
    let kind = ExclusionKind::parse(&kind).ok_or_else(|| ServerFnError::new("unknown kind"))?;
    // Only the viewer's own accounts can be excluded; platforms and games are free-form keys.
    if kind == ExclusionKind::Account {
        let owned = st
            .store
            .list_accounts(me.id)
            .await
            .map_err(internal)?
            .iter()
            .any(|a| a.id.to_string() == reference);
        if !owned {
            return Err(ServerFnError::new("not found"));
        }
    }
    if hidden {
        st.store
            .add_exclusion(me.id, None, kind, &reference)
            .await
            .map_err(internal)
    } else {
        st.store
            .remove_exclusion(me.id, kind, &reference, None)
            .await
            .map_err(internal)
    }
}

/// Names of the profiles that share with the viewer, for owner labels.
#[server]
pub async fn sharing_profiles() -> Result<Vec<(gameshelf_core::ProfileId, String)>, ServerFnError> {
    let me = require_profile().await?;
    let st = state()?;
    let mut out = Vec::new();
    for id in st.store.sharing_with_me(me.id).await.map_err(internal)? {
        if let Some(p) = st.store.get_profile(id).await.map_err(internal)? {
            out.push((p.id, p.display_name));
        }
    }
    Ok(out)
}

#[cfg(feature = "ssr")]
async fn owned_agent(agent: gameshelf_core::AgentId) -> Result<gameshelf_store::Agent, ServerFnError> {
    let profile = require_profile().await?;
    state()?
        .store
        .get_agent(agent)
        .await
        .map_err(internal)?
        .filter(|a| a.profile_id == profile.id)
        .ok_or_else(|| ServerFnError::new("not found"))
}

/// The profile's agents (US5).
#[server]
pub async fn agents() -> Result<Vec<AgentView>, ServerFnError> {
    let profile = require_profile().await?;
    let st = state()?;
    let now = time::OffsetDateTime::now_utc();
    let mut out = Vec::new();
    for a in st.store.list_agents(profile.id).await.map_err(internal)? {
        let config = a.config();
        let sources = gameshelf_core::agent::SOURCES
            .iter()
            .filter_map(|(id, _)| config.sources.get_key_value(*id))
            .map(|(id, s)| {
                let reported = a.source_states.get(id);
                AgentSourceView {
                    id: id.clone(),
                    enabled: s.enabled,
                    detected: reported.map(|x| x.detected),
                    last_scan_at: reported.and_then(|x| x.last_scan_at),
                }
            })
            .collect();
        let silent = a.status == gameshelf_store::AgentStatus::Active
            && a.last_seen_at
                .is_none_or(|t| now - t > time::Duration::hours(2 * i64::from(a.scan_hours)));
        out.push(AgentView {
            id: a.id,
            installs: st.store.count_installs(a.id).await.map_err(internal)?,
            name: a.name,
            os: a.os,
            version: a.version,
            status: a.status.as_str().to_string(),
            paused_locally: a.paused_locally,
            autostart: a.autostart,
            scan_hours: a.scan_hours,
            send_install_paths: a.send_install_paths,
            paired_at: a.paired_at,
            last_seen_at: a.last_seen_at,
            silent,
            sources,
        });
    }
    Ok(out)
}

/// Creates a single-use pairing code, valid 10 minutes.
#[server]
pub async fn create_pairing_code() -> Result<PairingCodeView, ServerFnError> {
    let profile = require_profile().await?;
    let st = state()?;
    let (code, expires_at) = st.store.create_pairing_code(profile.id).await.map_err(internal)?;
    let server_url = match &st.base_url {
        Some(u) => u.trim_end_matches('/').to_string(),
        None => {
            let headers = leptos_axum::extract::<axum::http::HeaderMap>()
                .await
                .unwrap_or_default();
            let host = headers
                .get(axum::http::header::HOST)
                .and_then(|h| h.to_str().ok())
                .unwrap_or("localhost");
            format!("{}://{host}", if st.https { "https" } else { "http" })
        }
    };
    Ok(PairingCodeView {
        code,
        expires_at,
        server_url,
    })
}

/// Pauses, resumes or removes an agent. A removed agent stops at its next contact.
#[server]
pub async fn set_agent_state(agent: gameshelf_core::AgentId, action: String) -> Result<(), ServerFnError> {
    use gameshelf_store::AgentStatus;
    let a = owned_agent(agent).await?;
    let status = match action.as_str() {
        "pause" => AgentStatus::Paused,
        "resume" => AgentStatus::Active,
        "revoke" => AgentStatus::Revoked,
        _ => return Err(ServerFnError::new("unknown action")),
    };
    state()?
        .store
        .set_agent_status(a.id, Some(a.profile_id), status)
        .await
        .map_err(internal)
}

/// Changes the scan interval and whether install folders are sent.
#[server]
pub async fn update_agent(
    agent: gameshelf_core::AgentId,
    scan_hours: u32,
    send_install_paths: bool,
) -> Result<(), ServerFnError> {
    let a = owned_agent(agent).await?;
    let settings = gameshelf_store::AgentSettings {
        scan_hours,
        send_install_paths,
        sources: a.config().sources,
    };
    state()?
        .store
        .update_agent_settings(a.id, a.profile_id, &settings)
        .await
        .map_err(|e| match e {
            gameshelf_store::StoreError::Invalid(msg) => ServerFnError::new(msg),
            other => internal(other),
        })
}

/// Turns one local source of an agent on or off.
#[server]
pub async fn set_agent_source(
    agent: gameshelf_core::AgentId,
    source: String,
    enabled: bool,
) -> Result<(), ServerFnError> {
    let a = owned_agent(agent).await?;
    let mut sources = a.config().sources;
    let Some(slot) = sources.get_mut(&source) else {
        return Err(ServerFnError::new("unknown source"));
    };
    slot.enabled = enabled;
    let settings = gameshelf_store::AgentSettings {
        scan_hours: a.scan_hours,
        send_install_paths: a.send_install_paths,
        sources,
    };
    state()?
        .store
        .update_agent_settings(a.id, a.profile_id, &settings)
        .await
        .map_err(internal)
}

/// Deletes an agent with its install records. Games it reported stay in the library.
#[server]
pub async fn delete_agent(agent: gameshelf_core::AgentId) -> Result<(), ServerFnError> {
    let a = owned_agent(agent).await?;
    state()?
        .store
        .delete_agent(a.id, a.profile_id)
        .await
        .map_err(internal)
}
