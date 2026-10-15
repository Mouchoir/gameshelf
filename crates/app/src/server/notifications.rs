//! Sends notifications after sync runs and daily reminders (US7, FR-034 to FR-036).
//!
//! The activity feed is the source: a run's events are grouped into one message per kind and sent to every
//! enabled target that subscribed to that kind. Reminders for the same issue go out at most once a day.

use super::AppState;
use gameshelf_core::{AccountId, ProfileId, account_status::AccountStatus, platform};
use gameshelf_notify::{Message, TargetConfig, text, text::Lang};
use gameshelf_store::ActivityKind;
use gameshelf_sync::RunOutcome;

/// Event kinds a target can subscribe to.
pub const EVENT_KINDS: [&str; 4] = [
    "game_added",
    "game_unavailable",
    "account_needs_reconnect",
    "account_degraded",
];

struct Outgoing {
    kind: &'static str,
    message: Message,
    /// Issues sent at most once a day per target.
    reminder: Option<String>,
}

fn platform_name(key: &str) -> &'static str {
    platform::platform(key).map(|p| p.name).unwrap_or("?")
}

/// Notifies the owner of an account about what one run changed.
pub async fn after_run(state: &AppState, account: AccountId, outcome: &RunOutcome) {
    let run = match outcome {
        RunOutcome::Imported { run, .. } | RunOutcome::Failed { run, .. } => *run,
        RunOutcome::Skipped(_) => return,
    };
    let events = match state.store.activity_for_run(run).await {
        Ok(e) if !e.is_empty() => e,
        Ok(_) => return,
        Err(e) => {
            tracing::warn!(error = %e, "cannot read run events");
            return;
        }
    };
    let Ok(Some(acc)) = state.store.get_account(account).await else {
        return;
    };
    let Ok(Some(profile)) = state.store.get_profile(acc.profile_id).await else {
        return;
    };
    let lang = Lang::from_code(profile.locale.as_str());
    let base = state.base_url.as_deref();
    let pname = platform_name(&acc.platform);
    let account_word = match lang {
        Lang::En => "account",
        Lang::Fr => "compte",
    };

    let mut added = Vec::new();
    let mut gone = Vec::new();
    let mut out = Vec::new();
    for e in &events {
        let title = match e.game_id {
            Some(g) => state.store.game_summary(g).await.ok().flatten().map(|s| s.title),
            None => None,
        };
        let line = title.map(|t| format!("{t} ({pname}, {account_word} {})", acc.label));
        match e.kind {
            ActivityKind::GameAdded => added.extend(line),
            ActivityKind::GameUnavailable => gone.extend(line),
            ActivityKind::AccountNeedsReconnect => out.push(Outgoing {
                kind: "account_needs_reconnect",
                message: text::needs_reconnect(lang, pname, &acc.label, base),
                reminder: Some(format!("reconnect:{account}")),
            }),
            ActivityKind::AccountDegraded => out.push(Outgoing {
                kind: "account_degraded",
                message: text::degraded(lang, pname, &acc.label, base),
                reminder: Some(format!("degraded:{account}")),
            }),
            ActivityKind::AgentSilent | ActivityKind::SyncFailed => {}
        }
    }
    if !added.is_empty() {
        out.push(Outgoing {
            kind: "game_added",
            message: text::games_added(lang, &added, base),
            reminder: None,
        });
    }
    if !gone.is_empty() {
        out.push(Outgoing {
            kind: "game_unavailable",
            message: text::games_unavailable(lang, &gone, base),
            reminder: None,
        });
    }
    deliver(state, profile.id, out).await;
}

/// Reminds owners of accounts that still need reconnecting, at most once a day per target.
pub async fn daily_reminders(state: &AppState) {
    let Ok(profiles) = state.store.list_profiles().await else {
        return;
    };
    for profile in profiles {
        let Ok(accounts) = state.store.list_accounts(profile.id).await else {
            continue;
        };
        let lang = Lang::from_code(profile.locale.as_str());
        let out: Vec<Outgoing> = accounts
            .iter()
            .filter(|a| a.status == AccountStatus::NeedsReconnect)
            .map(|a| Outgoing {
                kind: "account_needs_reconnect",
                message: text::needs_reconnect(
                    lang,
                    platform_name(&a.platform),
                    &a.label,
                    state.base_url.as_deref(),
                ),
                reminder: Some(format!("reconnect:{}", a.id)),
            })
            .collect();
        if !out.is_empty() {
            deliver(state, profile.id, out).await;
        }
    }
}

/// Sends the test message to one target of a profile.
pub async fn send_test(state: &AppState, profile: ProfileId, target: &str) -> Result<(), String> {
    let lang = match state.store.get_profile(profile).await {
        Ok(Some(p)) => Lang::from_code(p.locale.as_str()),
        _ => Lang::En,
    };
    let opened = state
        .store
        .open_target(profile, target)
        .await
        .map_err(|e| e.to_string())?;
    let Some((t, cfg)) = opened else {
        return Err("not found".into());
    };
    let cfg: TargetConfig =
        serde_json::from_slice(&cfg).map_err(|_| "unreadable configuration".to_string())?;
    let result =
        gameshelf_notify::send(&state.http, &cfg, &text::test(lang, state.base_url.as_deref())).await;
    let err = result.as_ref().err().map(ToString::to_string);
    let _ = state.store.record_delivery(&t.id, err.as_deref()).await;
    result.map_err(|e| e.to_string())
}

async fn deliver(state: &AppState, profile: ProfileId, out: Vec<Outgoing>) {
    let Ok(targets) = state.store.list_targets(profile).await else {
        return;
    };
    for target in targets.into_iter().filter(|t| t.enabled) {
        let wanted: Vec<&Outgoing> = out
            .iter()
            .filter(|o| target.events.iter().any(|e| e == o.kind))
            .collect();
        if wanted.is_empty() {
            continue;
        }
        let cfg = match state.store.open_target(profile, &target.id).await {
            Ok(Some((_, cfg))) => match serde_json::from_slice::<TargetConfig>(&cfg) {
                Ok(c) => c,
                Err(_) => continue,
            },
            _ => continue,
        };
        for o in wanted {
            if let Some(issue) = &o.reminder
                && !state
                    .store
                    .claim_reminder(&target.id, issue)
                    .await
                    .unwrap_or(false)
            {
                continue;
            }
            let result = gameshelf_notify::send(&state.http, &cfg, &o.message).await;
            if let Err(e) = &result {
                tracing::warn!(target = %target.name, error = %e, "notification failed");
            }
            let err = result.err().map(|e| e.to_string());
            let _ = state.store.record_delivery(&target.id, err.as_deref()).await;
        }
    }
}
