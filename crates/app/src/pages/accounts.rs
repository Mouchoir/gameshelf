//! Accounts: health, sync now, rename, disconnect, and the connection pages (US1, US3, FR-002 to FR-004).

use crate::{
    api::{ConnectWithKey, DisconnectAccount, RenameAccount, SyncNow, accounts, platforms},
    components::{PlatformIcon, StatusBadge, TierChip},
    format,
    i18n::*,
    types::{AccountView, ConnectResult, PlatformView},
};
use gameshelf_core::account_status::AccountStatus;
use leptos::prelude::*;
use leptos_router::hooks::use_params_map;
use std::{collections::HashMap, time::Duration};

#[component]
pub fn AccountsPage() -> impl IntoView {
    let i18n = use_i18n();
    let list = Resource::new(|| (), |_| accounts());
    let sync = ServerAction::<SyncNow>::new();
    let rename = ServerAction::<RenameAccount>::new();
    let disconnect = ServerAction::<DisconnectAccount>::new();
    // Refresh after every action, and every few seconds while a sync runs.
    Effect::new(move |_| {
        sync.version().get();
        rename.version().get();
        disconnect.version().get();
        list.refetch();
    });
    Effect::new(move |_| {
        let syncing = list
            .get()
            .and_then(|r| r.ok())
            .is_some_and(|l| l.iter().any(|a| a.syncing));
        if syncing {
            set_timeout(move || list.refetch(), Duration::from_secs(3));
        }
    });

    view! {
        <section class="accounts">
            <div class="page-head">
                <h1>{t!(i18n, accounts_title)}</h1>
                <a class="button primary" href="/accounts/connect">{t!(i18n, accounts_connect)}</a>
            </div>
            <Transition fallback=move || view! { <p class="loading">{t!(i18n, loading)}</p> }>
                {move || list.get().map(|r| match r {
                    Err(_) => view! { <p class="error">{t!(i18n, error_generic)}</p> }.into_any(),
                    Ok(list) if list.is_empty() => view! { <p class="empty">{t!(i18n, accounts_empty)}</p> }.into_any(),
                    Ok(list) => view! {
                        <ul class="account-list">
                            {list.into_iter().map(|a| view! { <AccountItem a sync rename disconnect/> }).collect_view()}
                        </ul>
                    }.into_any(),
                })}
            </Transition>
        </section>
    }
}

#[component]
fn AccountItem(
    a: AccountView,
    sync: ServerAction<SyncNow>,
    rename: ServerAction<RenameAccount>,
    disconnect: ServerAction<DisconnectAccount>,
) -> impl IntoView {
    let i18n = use_i18n();
    let editing = RwSignal::new(false);
    let confirming = RwSignal::new(false);
    let label = RwSignal::new(a.label.clone());
    let id = a.id;
    let last_sync = a.last_sync_at.map(format::date);
    let needs_reconnect = a.status == AccountStatus::NeedsReconnect;
    view! {
        <li class="account">
            <div class="account-main">
                <PlatformIcon key=a.platform.clone() large=true/>
                <div>
                    <Show when=move || editing.get() fallback=move || view! { <strong>{move || label.get()}</strong> }>
                        <form class="inline" on:submit=move |ev| {
                            ev.prevent_default();
                            rename.dispatch(RenameAccount { account: id, label: label.get_untracked() });
                            editing.set(false);
                        }>
                            <input prop:value=move || label.get() on:input=move |ev| label.set(event_target_value(&ev)) maxlength="64"/>
                            <button type="submit">{t!(i18n, account_save)}</button>
                            <button type="button" class="ghost" on:click=move |_| editing.set(false)>{t!(i18n, account_cancel)}</button>
                        </form>
                    </Show>
                    <div class="muted small">{a.platform_name.clone()}{a.display_name.clone().map(|d| format!(" · {d}"))}</div>
                </div>
                <StatusBadge status=a.status/>
            </div>
            <dl class="facts">
                <div><dt>{t!(i18n, account_games)}</dt><dd>{a.games}</dd></div>
                <div><dt>{t!(i18n, account_last_sync)}</dt><dd>{match last_sync { Some(d) => d.into_any(), None => t!(i18n, never).into_any() }}</dd></div>
                {a.last_error.clone().map(|e| view! { <div class="wide"><dt>{t!(i18n, account_last_error)}</dt><dd class="error">{e}</dd></div> })}
            </dl>
            {needs_reconnect.then(|| {
                let href = format!("/accounts/connect/{}", a.platform);
                view! { <p class="warn-text">{t!(i18n, account_reconnect_hint)}" "<a href=href>{t!(i18n, accounts_connect)}</a></p> }
            })}
            <div class="actions">
                <button type="button" disabled=a.syncing || !a.status.syncs()
                    on:click=move |_| { sync.dispatch(SyncNow { account: id }); }>
                    {if a.syncing { t!(i18n, account_sync_started).into_any() } else { t!(i18n, account_sync_now).into_any() }}
                </button>
                <button type="button" class="ghost" on:click=move |_| editing.set(true)>{t!(i18n, account_rename)}</button>
                <Show when=move || confirming.get() fallback=move || view! {
                    <button type="button" class="ghost danger" on:click=move |_| confirming.set(true)>{t!(i18n, account_disconnect)}</button>
                }>
                    <button type="button" class="ghost" on:click=move |_| { disconnect.dispatch(DisconnectAccount { account: id, delete_data: false }); }>
                        {t!(i18n, account_disconnect_keep)}
                    </button>
                    <button type="button" class="danger" on:click=move |_| { disconnect.dispatch(DisconnectAccount { account: id, delete_data: true }); }>
                        {t!(i18n, account_disconnect_delete)}
                    </button>
                    <button type="button" class="ghost" on:click=move |_| confirming.set(false)>{t!(i18n, account_cancel)}</button>
                </Show>
            </div>
        </li>
    }
}

/// Every platform with its tier and whether it can be connected (FR-002).
#[component]
pub fn ConnectPickerPage() -> impl IntoView {
    let i18n = use_i18n();
    let list = Resource::new(|| (), |_| platforms());
    view! {
        <section class="connect">
            <h1>{t!(i18n, connect_title)}</h1>
            <p class="lead">{t!(i18n, connect_intro)}</p>
            <Suspense fallback=move || view! { <p class="loading">{t!(i18n, loading)}</p> }>
                {move || list.get().map(|r| match r {
                    Err(_) => view! { <p class="error">{t!(i18n, error_generic)}</p> }.into_any(),
                    Ok(mut list) => {
                        list.sort_by_key(|p| (!p.connectable, p.agent_only));
                        view! {
                            <ul class="platform-grid">
                                {list.into_iter().map(|p| view! { <PlatformTile p/> }).collect_view()}
                            </ul>
                        }.into_any()
                    }
                })}
            </Suspense>
        </section>
    }
}

#[component]
fn PlatformTile(p: PlatformView) -> impl IntoView {
    let i18n = use_i18n();
    let status = if p.connectable {
        None
    } else if p.agent_only {
        Some(t!(i18n, connect_via_agent).into_any())
    } else {
        Some(t!(i18n, connect_coming_soon).into_any())
    };
    let body = view! {
        <PlatformIcon key=p.key.clone() large=true/>
        <span class="platform-name">{p.name.clone()}</span>
        <TierChip tier=p.tier/>
        {status.map(|s| view! { <span class="muted small">{s}</span> })}
    };
    if p.connectable {
        view! { <li><a class="platform-tile" href=format!("/accounts/connect/{}", p.key)>{body}</a></li> }
            .into_any()
    } else {
        view! { <li><div class="platform-tile disabled">{body}</div></li> }.into_any()
    }
}

/// The connection form of one platform.
#[component]
pub fn ConnectPlatformPage() -> impl IntoView {
    let i18n = use_i18n();
    let params = use_params_map();
    let key = move || params.read().get("platform").unwrap_or_default();
    let list = Resource::new(|| (), |_| platforms());
    view! {
        <section class="connect">
            <p class="crumbs"><a href="/accounts/connect">{t!(i18n, connect_title)}</a></p>
            <Suspense fallback=move || view! { <p class="loading">{t!(i18n, loading)}</p> }>
                {move || list.get().map(|r| {
                    let found = r.ok().and_then(|l| l.into_iter().find(|p| p.key == key() && p.connectable));
                    match found {
                        Some(p) => view! { <ConnectForm p/> }.into_any(),
                        None => view! { <p>{t!(i18n, connect_error_unknown)}</p> }.into_any(),
                    }
                })}
            </Suspense>
        </section>
    }
}

#[component]
fn ConnectForm(p: PlatformView) -> impl IntoView {
    let i18n = use_i18n();
    let action = ServerAction::<ConnectWithKey>::new();
    let values: RwSignal<HashMap<String, String>> = RwSignal::new(HashMap::new());
    let label = RwSignal::new(String::new());
    let platform_key = p.key.clone();
    let help = p.key == "steam";
    let fields = p.fields.clone();
    let on_submit = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        action.dispatch(ConnectWithKey {
            platform: platform_key.clone(),
            fields: values.get_untracked(),
            label: label.get_untracked(),
        });
    };
    let result = move || action.value().get();
    view! {
        <h1><PlatformIcon key=p.key.clone() large=true/>" "{p.name.clone()}</h1>
        {help.then(|| view! { <p class="lead">{t!(i18n, connect_steam_help)}</p> })}
        <form class="form" on:submit=on_submit>
            {fields.into_iter().map(|f| {
                let name = f.name.clone();
                let name2 = f.name.clone();
                view! {
                    <label>
                        <span><FieldLabel key=f.label_key.clone()/></span>
                        <input
                            name=f.name.clone()
                            type=if f.secret { "password" } else { "text" }
                            required
                            autocomplete="off"
                            spellcheck="false"
                            prop:value=move || values.with(|v| v.get(&name).cloned().unwrap_or_default())
                            on:input=move |ev| { let v = event_target_value(&ev); values.update(|m| { m.insert(name2.clone(), v); }); }
                        />
                    </label>
                }
            }).collect_view()}
            <label>
                <span>{t!(i18n, connect_label)}</span>
                <input maxlength="64" prop:value=move || label.get() on:input=move |ev| label.set(event_target_value(&ev))/>
            </label>
            {move || result().map(|r| match r {
                Ok(ConnectResult::Connected(_)) => view! {
                    <p class="ok-text" role="status">{t!(i18n, connect_done)}" "<a href="/accounts">{t!(i18n, nav_accounts)}</a></p>
                }.into_any(),
                Ok(ConnectResult::Rejected(reason)) => view! {
                    <p class="error" role="alert">{t!(i18n, connect_error_rejected, reason = move || reason.clone())}</p>
                }.into_any(),
                Ok(ConnectResult::Taken(holder)) => view! {
                    <p class="error" role="alert">{t!(i18n, connect_error_taken, holder = move || holder.clone())}</p>
                }.into_any(),
                Ok(ConnectResult::Unknown) => view! { <p class="error" role="alert">{t!(i18n, connect_error_unknown)}</p> }.into_any(),
                Err(_) => view! { <p class="error" role="alert">{t!(i18n, error_generic)}</p> }.into_any(),
            })}
            <button type="submit" class="primary" disabled=move || action.pending().get()>{t!(i18n, connect_submit)}</button>
        </form>
    }
}

/// Localized label of a connector field.
#[component]
fn FieldLabel(key: String) -> impl IntoView {
    let i18n = use_i18n();
    match key.as_str() {
        "connect.steam.steam_id" => t!(i18n, connect_steam_steam_id).into_any(),
        "connect.steam.api_key" => t!(i18n, connect_steam_api_key).into_any(),
        "connect.fake.account" => t!(i18n, connect_fake_account).into_any(),
        other => other.to_string().into_any(),
    }
}
