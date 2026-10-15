//! Notification destinations: Discord, ntfy, Gotify (US7).

use crate::{
    api::{AddTarget, DeleteTarget, SetTargetEnabled, TestTarget, ToggleTargetEvent, notification_targets},
    format,
    i18n::*,
    types::TargetView,
};
use leptos::prelude::*;

const EVENTS: [&str; 5] = [
    "game_added",
    "game_unavailable",
    "account_needs_reconnect",
    "account_degraded",
    "agent_silent",
];

#[component]
pub fn NotificationsPage() -> impl IntoView {
    let i18n = use_i18n();
    let list = Resource::new(|| (), |_| notification_targets());
    let add = ServerAction::<AddTarget>::new();
    let toggle_event = ServerAction::<ToggleTargetEvent>::new();
    let enable = ServerAction::<SetTargetEnabled>::new();
    let delete = ServerAction::<DeleteTarget>::new();
    Effect::new(move |_| {
        add.version().get();
        toggle_event.version().get();
        enable.version().get();
        delete.version().get();
        list.refetch();
    });

    let kind = RwSignal::new("ntfy".to_string());
    let add_error = move || add.value().get().and_then(|r| r.ok()).flatten();

    view! {
        <section class="notifications">
            <h1>{t!(i18n, notif_title)}</h1>
            <p class="lead">{t!(i18n, notif_intro)}</p>
            <Transition fallback=move || view! { <p class="loading">{t!(i18n, loading)}</p> }>
                {move || list.get().map(|r| match r {
                    Err(_) => view! { <p class="error">{t!(i18n, error_generic)}</p> }.into_any(),
                    Ok(list) if list.is_empty() => view! { <p class="empty">{t!(i18n, notif_empty)}</p> }.into_any(),
                    Ok(list) => view! {
                        <ul class="account-list">
                            {list.into_iter().map(|t| view! { <TargetItem t toggle_event enable delete/> }).collect_view()}
                        </ul>
                    }.into_any(),
                })}
            </Transition>

            <h2>{t!(i18n, notif_add)}</h2>
            <ActionForm action=add attr:class="form narrow-form">
                <label>
                    <span>{t!(i18n, notif_kind)}</span>
                    <select name="kind" on:change=move |ev| kind.set(event_target_value(&ev))>
                        <option value="ntfy" selected>"ntfy"</option>
                        <option value="discord">"Discord"</option>
                        <option value="gotify">"Gotify"</option>
                    </select>
                </label>
                <label>
                    <span>{t!(i18n, notif_name)}</span>
                    <input name="name" required maxlength="64"/>
                </label>
                <label>
                    <span>{move || if kind.get() == "discord" { t!(i18n, notif_webhook_url).into_any() } else { t!(i18n, notif_server).into_any() }}</span>
                    <input name="address" type="url" required
                        placeholder=move || match kind.get().as_str() {
                            "discord" => "https://discord.com/api/webhooks/...",
                            "gotify" => "https://gotify.example.com",
                            _ => "https://ntfy.sh",
                        }/>
                </label>
                <label class:hidden=move || kind.get() != "ntfy">
                    <span>{t!(i18n, notif_topic)}</span>
                    <input name="topic" maxlength="64"/>
                </label>
                <label class:hidden=move || kind.get() == "discord">
                    <span>{move || if kind.get() == "gotify" { t!(i18n, notif_token).into_any() } else { t!(i18n, notif_token_optional).into_any() }}</span>
                    <input name="token" type="password" autocomplete="off"/>
                </label>
                {move || add_error().map(|e| view! { <p class="error" role="alert">{e}</p> })}
                <button type="submit" class="primary" disabled=move || add.pending().get()>{t!(i18n, notif_save)}</button>
            </ActionForm>
        </section>
    }
}

#[component]
fn TargetItem(
    t: TargetView,
    toggle_event: ServerAction<ToggleTargetEvent>,
    enable: ServerAction<SetTargetEnabled>,
    delete: ServerAction<DeleteTarget>,
) -> impl IntoView {
    let i18n = use_i18n();
    let test = ServerAction::<TestTarget>::new();
    let id = t.id.clone();
    let enabled = t.enabled;
    let last_sent = t.last_sent_at.map(format::date);
    let kind_name = match t.kind.as_str() {
        "discord" => "Discord",
        "gotify" => "Gotify",
        _ => "ntfy",
    };
    view! {
        <li class="account">
            <div class="account-main">
                <div>
                    <strong>{t.name.clone()}</strong>
                    <div class="muted small">{kind_name}</div>
                </div>
                <label class="toggle">
                    <input type="checkbox" prop:checked=enabled on:change={
                        let id = id.clone();
                        move |ev| { enable.dispatch(SetTargetEnabled { target: id.clone(), enabled: event_target_checked(&ev) }); }
                    }/>
                    <span>{t!(i18n, notif_enabled)}</span>
                </label>
            </div>
            <fieldset class="events">
                <legend>{t!(i18n, notif_events)}</legend>
                {EVENTS.into_iter().map(|e| {
                    let on = t.events.iter().any(|x| x == e);
                    let id = id.clone();
                    view! {
                        <label class="toggle">
                            <input type="checkbox" prop:checked=on on:change=move |ev| {
                                toggle_event.dispatch(ToggleTargetEvent { target: id.clone(), event: e.to_string(), on: event_target_checked(&ev) });
                            }/>
                            <span>{match e {
                                "game_added" => t!(i18n, event_game_added).into_any(),
                                "game_unavailable" => t!(i18n, event_game_unavailable).into_any(),
                                "account_needs_reconnect" => t!(i18n, event_account_needs_reconnect).into_any(),
                                "agent_silent" => t!(i18n, event_agent_silent).into_any(),
                                _ => t!(i18n, event_account_degraded).into_any(),
                            }}</span>
                        </label>
                    }
                }).collect_view()}
            </fieldset>
            <dl class="facts">
                <div><dt>{t!(i18n, notif_last_sent)}</dt><dd>{match last_sent { Some(d) => d.into_any(), None => t!(i18n, never).into_any() }}</dd></div>
                {t.last_error.clone().map(|e| view! { <div class="wide"><dt>{t!(i18n, account_last_error)}</dt><dd class="error">{e}</dd></div> })}
            </dl>
            <div class="actions">
                <button type="button" disabled=move || test.pending().get() on:click={
                    let id = id.clone();
                    move |_| { test.dispatch(TestTarget { target: id.clone() }); }
                }>{t!(i18n, notif_test)}</button>
                {move || test.value().get().map(|r| match r {
                    Ok(None) => view! { <span class="ok-text">{t!(i18n, notif_test_ok)}</span> }.into_any(),
                    Ok(Some(reason)) => view! { <span class="error">{t!(i18n, notif_test_failed, reason = move || reason.clone())}</span> }.into_any(),
                    Err(_) => view! { <span class="error">{t!(i18n, error_generic)}</span> }.into_any(),
                })}
                <button type="button" class="ghost danger" on:click={
                    let id = id.clone();
                    move |_| { delete.dispatch(DeleteTarget { target: id.clone() }); }
                }>{t!(i18n, notif_delete)}</button>
            </div>
        </li>
    }
}
