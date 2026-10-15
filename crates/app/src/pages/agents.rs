//! Agents: computers that read their local launchers and report the games they find (US5, FR-026 to FR-030).
//! Everything the agent does is visible and switchable here.

use crate::{
    api::{CreatePairingCode, DeleteAgent, SetAgentSource, SetAgentState, UpdateAgent, agents},
    format,
    i18n::*,
    types::{AgentSourceView, AgentView, PairingCodeView},
};
use leptos::prelude::*;

/// Scan intervals offered, in hours.
const INTERVALS: [u32; 7] = [1, 3, 6, 12, 24, 48, 168];

/// Brand name of a single-launcher source; `None` for the multi-launcher one.
fn source_brand(id: &str) -> Option<&'static str> {
    match id {
        "steam-local" => Some("Steam"),
        "epic-local" => Some("Epic Games"),
        "gog-galaxy-local" => Some("GOG Galaxy"),
        "battlenet-local" => Some("Battle.net"),
        "ubisoft-local" => Some("Ubisoft Connect"),
        _ => None,
    }
}

fn os_name(os: &str) -> &'static str {
    match os {
        "windows" => "Windows",
        "linux" => "Linux",
        _ => "?",
    }
}

#[component]
pub fn AgentsPage() -> impl IntoView {
    let i18n = use_i18n();
    let list = Resource::new(|| (), |_| agents());
    let set_state = ServerAction::<SetAgentState>::new();
    let update = ServerAction::<UpdateAgent>::new();
    let source = ServerAction::<SetAgentSource>::new();
    let delete = ServerAction::<DeleteAgent>::new();
    let code = ServerAction::<CreatePairingCode>::new();
    Effect::new(move |_| {
        set_state.version().get();
        update.version().get();
        source.version().get();
        delete.version().get();
        list.refetch();
    });
    view! {
        <section class="agents">
            <h1>{t!(i18n, agents_title)}</h1>
            <p class="lead">{t!(i18n, agents_intro)}</p>
            <Transition fallback=move || view! { <p class="loading">{t!(i18n, loading)}</p> }>
                {move || list.get().map(|r| match r {
                    Err(_) => view! { <p class="error">{t!(i18n, error_generic)}</p> }.into_any(),
                    Ok(list) if list.is_empty() => view! { <p class="empty">{t!(i18n, agents_empty)}</p> }.into_any(),
                    Ok(list) => view! {
                        <ul class="account-list">
                            {list.into_iter().map(|a| view! { <AgentItem a set_state update source delete/> }).collect_view()}
                        </ul>
                    }.into_any(),
                })}
            </Transition>

            <h2>{t!(i18n, agents_add)}</h2>
            <p>{t!(i18n, agents_add_intro)}</p>
            <button type="button" class="primary" disabled=move || code.pending().get()
                on:click=move |_| { code.dispatch(CreatePairingCode {}); }>
                {t!(i18n, agents_create_code)}
            </button>
            {move || code.value().get().map(|r| match r {
                Ok(c) => view! { <PairingCode c/> }.into_any(),
                Err(_) => view! { <p class="error" role="alert">{t!(i18n, error_generic)}</p> }.into_any(),
            })}
        </section>
    }
}

#[component]
fn PairingCode(c: PairingCodeView) -> impl IntoView {
    let i18n = use_i18n();
    let command = format!("gameshelf-agent pair {} {}", c.server_url, c.code);
    let until = format::date_time(c.expires_at);
    view! {
        <div class="pairing" role="status">
            <p class="pairing-code">{c.code.clone()}</p>
            <p>{t!(i18n, agents_code_steps)}</p>
            <pre class="command"><code>{command}</code></pre>
            <p class="muted small">{t!(i18n, agents_code_expires, time = move || until.clone())}</p>
        </div>
    }
}

#[component]
fn AgentItem(
    a: AgentView,
    set_state: ServerAction<SetAgentState>,
    update: ServerAction<UpdateAgent>,
    source: ServerAction<SetAgentSource>,
    delete: ServerAction<DeleteAgent>,
) -> impl IntoView {
    let i18n = use_i18n();
    let id = a.id;
    let revoked = a.status == "revoked";
    let paused = a.status == "paused";
    let confirming = RwSignal::new(false);
    let hours = a.scan_hours;
    let paths = a.send_install_paths;
    let last_seen = a.last_seen_at.map(format::date_time);
    let badge = if revoked {
        view! { <span class="badge muted">{t!(i18n, agents_status_revoked)}</span> }.into_any()
    } else if paused {
        view! { <span class="badge warn">{t!(i18n, agents_status_paused)}</span> }.into_any()
    } else if a.paused_locally {
        view! { <span class="badge warn">{t!(i18n, agents_paused_locally)}</span> }.into_any()
    } else if a.silent {
        view! { <span class="badge warn">{t!(i18n, agents_silent)}</span> }.into_any()
    } else {
        view! { <span class="badge ok">{t!(i18n, agents_status_active)}</span> }.into_any()
    };
    view! {
        <li class="account">
            <div class="account-main">
                <div>
                    <strong>{a.name.clone()}</strong>
                    <div class="muted small">{os_name(&a.os)}" · "{a.version.clone()}</div>
                </div>
                {badge}
            </div>
            <dl class="facts">
                <div><dt>{t!(i18n, agents_last_contact)}</dt><dd>{match last_seen { Some(d) => d.into_any(), None => t!(i18n, never).into_any() }}</dd></div>
                <div><dt>{t!(i18n, agents_installed)}</dt><dd>{a.installs}</dd></div>
                <div><dt>{t!(i18n, agents_autostart)}</dt><dd>{if a.autostart { t!(i18n, yes).into_any() } else { t!(i18n, no).into_any() }}</dd></div>
                <div><dt>{t!(i18n, agents_paired)}</dt><dd>{format::date(a.paired_at)}</dd></div>
            </dl>
            {(!revoked).then(|| view! {
                <div class="agent-settings">
                    <label>
                        <span>{t!(i18n, agents_scan_every)}</span>
                        <select on:change=move |ev| {
                            let h = event_target_value(&ev).parse().unwrap_or(hours);
                            update.dispatch(UpdateAgent { agent: id, scan_hours: h, send_install_paths: paths });
                        }>
                            {INTERVALS.into_iter().map(|h| view! { <option value=h.to_string() selected=h == hours>{format!("{h} h")}</option> }).collect_view()}
                        </select>
                    </label>
                    <label class="toggle">
                        <input type="checkbox" prop:checked=paths on:change=move |ev| {
                            update.dispatch(UpdateAgent { agent: id, scan_hours: hours, send_install_paths: event_target_checked(&ev) });
                        }/>
                        <span>{t!(i18n, agents_send_paths)}</span>
                    </label>
                    <small class="muted">{t!(i18n, agents_send_paths_hint)}</small>
                </div>
                <fieldset class="events">
                    <legend>{t!(i18n, agents_sources)}</legend>
                    {a.sources.clone().into_iter().map(|s| view! { <SourceToggle agent=id s source/> }).collect_view()}
                </fieldset>
            })}
            <div class="actions">
                {(!revoked).then(|| {
                    let action = if paused { "resume" } else { "pause" };
                    view! {
                        <button type="button" on:click=move |_| { set_state.dispatch(SetAgentState { agent: id, action: action.to_string() }); }>
                            {if paused { t!(i18n, agents_resume).into_any() } else { t!(i18n, agents_pause).into_any() }}
                        </button>
                    }
                })}
                <Show when=move || confirming.get() fallback=move || view! {
                    <button type="button" class="ghost danger" on:click=move |_| confirming.set(true)>
                        {if revoked { t!(i18n, agents_delete).into_any() } else { t!(i18n, agents_revoke).into_any() }}
                    </button>
                }>
                    <span class="warn-text small">{if revoked { t!(i18n, agents_delete_warning).into_any() } else { t!(i18n, agents_revoke_warning).into_any() }}</span>
                    <button type="button" class="danger" on:click=move |_| {
                        if revoked {
                            delete.dispatch(DeleteAgent { agent: id });
                        } else {
                            set_state.dispatch(SetAgentState { agent: id, action: "revoke".into() });
                        }
                        confirming.set(false);
                    }>{if revoked { t!(i18n, agents_delete).into_any() } else { t!(i18n, agents_revoke).into_any() }}</button>
                    <button type="button" class="ghost" on:click=move |_| confirming.set(false)>{t!(i18n, account_cancel)}</button>
                </Show>
            </div>
        </li>
    }
}

#[component]
fn SourceToggle(
    agent: gameshelf_core::AgentId,
    s: AgentSourceView,
    source: ServerAction<SetAgentSource>,
) -> impl IntoView {
    let i18n = use_i18n();
    let id = s.id.clone();
    let name = match source_brand(&s.id) {
        Some(brand) => brand.into_any(),
        None => t!(i18n, agents_source_launchers).into_any(),
    };
    let detected = match s.detected {
        Some(true) => {
            Some(view! { <span class="ok-text small">{t!(i18n, agents_detected)}</span> }.into_any())
        }
        Some(false) => {
            Some(view! { <span class="muted small">{t!(i18n, agents_not_detected)}</span> }.into_any())
        }
        None => None,
    };
    view! {
        <label class="toggle">
            <input type="checkbox" prop:checked=s.enabled on:change=move |ev| {
                source.dispatch(SetAgentSource { agent, source: id.clone(), enabled: event_target_checked(&ev) });
            }/>
            <span>{name}</span>
            {detected}
        </label>
    }
}
