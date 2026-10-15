//! The library: search-as-you-type, sort, filters and the cover grid (US1, US2).

use crate::{
    api::{accounts, library},
    components::{GameCard, PlatformIcon},
    i18n::*,
    types::LibraryRequest,
};
use gameshelf_core::view::{LibraryQuery, Sort};
use leptos::prelude::*;
use std::time::Duration;

/// Tiles fetched per page.
const PAGE: u32 = 120;
/// Delay before a search is sent while typing (research D7).
const DEBOUNCE: Duration = Duration::from_millis(120);

#[component]
pub fn LibraryPage() -> impl IntoView {
    let i18n = use_i18n();
    let typed = RwSignal::new(String::new());
    let search = RwSignal::new(String::new());
    let sort = RwSignal::new(Sort::Title);
    let platform = RwSignal::new(None::<String>);
    let show_subscriptions = RwSignal::new(false);
    let show_unavailable = RwSignal::new(false);
    let limit = RwSignal::new(PAGE);

    let on_input = move |ev| {
        let value = event_target_value(&ev);
        typed.set(value.clone());
        set_timeout(
            move || {
                if typed.get_untracked() == value {
                    search.set(value);
                    limit.set(PAGE);
                }
            },
            DEBOUNCE,
        );
    };

    let request = Memo::new(move |_| LibraryRequest {
        search: search.get(),
        query: LibraryQuery {
            platforms: platform.get().into_iter().collect(),
            include_subscription_only: show_subscriptions.get(),
            include_unavailable: show_unavailable.get(),
            sort: sort.get(),
            descending: matches!(sort.get(), Sort::Added | Sort::LastPlayed | Sort::Playtime),
            limit: limit.get(),
            ..Default::default()
        },
    });
    let page = Resource::new(move || request.get(), library);
    let my_accounts = Resource::new(|| (), |_| accounts());

    let platforms = move || {
        let mut keys: Vec<String> = my_accounts
            .get()
            .and_then(|r| r.ok())
            .unwrap_or_default()
            .into_iter()
            .map(|a| a.platform)
            .collect();
        keys.sort();
        keys.dedup();
        keys
    };

    view! {
        <section class="library">
            <div class="toolbar">
                <label class="search">
                    <span class="visually-hidden">{t!(i18n, library_search_label)}</span>
                    <input
                        type="search"
                        autofocus
                        autocomplete="off"
                        placeholder=move || t_string!(i18n, library_search_placeholder).to_string()
                        prop:value=move || typed.get()
                        on:input=on_input
                    />
                </label>
                <label class="select">
                    <span>{t!(i18n, sort_label)}</span>
                    <select on:change=move |ev| {
                        sort.set(match event_target_value(&ev).as_str() {
                            "added" => Sort::Added,
                            "last_played" => Sort::LastPlayed,
                            "playtime" => Sort::Playtime,
                            _ => Sort::Title,
                        })
                    }>
                        <option value="title">{t!(i18n, sort_title)}</option>
                        <option value="added">{t!(i18n, sort_added)}</option>
                        <option value="last_played">{t!(i18n, sort_last_played)}</option>
                        <option value="playtime">{t!(i18n, sort_playtime)}</option>
                    </select>
                </label>
                <div class="chips" role="group" aria-label=move || t_string!(i18n, filter_platform_label).to_string()>
                    <button
                        type="button"
                        class="chip-btn"
                        aria-pressed=move || platform.get().is_none().to_string()
                        on:click=move |_| platform.set(None)
                    >
                        {t!(i18n, filter_all_platforms)}
                    </button>
                    <Transition fallback=|| ()>
                        {move || platforms().into_iter().map(|key| {
                            let k = key.clone();
                            let k2 = key.clone();
                            view! {
                                <button
                                    type="button"
                                    class="chip-btn"
                                    aria-pressed=move || (platform.get().as_deref() == Some(k.as_str())).to_string()
                                    on:click=move |_| platform.set(Some(k2.clone()))
                                >
                                    <PlatformIcon key=key/>
                                </button>
                            }
                        }).collect_view()}
                    </Transition>
                </div>
                <label class="toggle">
                    <input type="checkbox" prop:checked=move || show_subscriptions.get()
                        on:change=move |ev| show_subscriptions.set(event_target_checked(&ev))/>
                    <span>{t!(i18n, filter_show_subscriptions)}</span>
                </label>
                <label class="toggle">
                    <input type="checkbox" prop:checked=move || show_unavailable.get()
                        on:change=move |ev| show_unavailable.set(event_target_checked(&ev))/>
                    <span>{t!(i18n, filter_show_unavailable)}</span>
                </label>
            </div>
            <Transition fallback=move || view! { <p class="loading">{t!(i18n, loading)}</p> }>
                {move || page.get().map(|result| match result {
                    Err(_) => view! { <p class="error">{t!(i18n, error_generic)}</p> }.into_any(),
                    Ok(page) if page.games.is_empty() && !search.get_untracked().trim().is_empty() => {
                        let query = search.get_untracked();
                        view! {
                            <div class="empty not-owned" role="status">
                                <h2>{t!(i18n, library_not_owned)}</h2>
                                <p>{t!(i18n, library_not_owned_body, query = move || query.clone())}</p>
                            </div>
                        }.into_any()
                    }
                    Ok(page) if page.games.is_empty() => view! {
                        <div class="empty">
                            <p>{t!(i18n, library_empty)}</p>
                            <a class="button primary" href="/accounts/connect">{t!(i18n, library_connect_cta)}</a>
                        </div>
                    }.into_any(),
                    Ok(page) => {
                        let total = page.total;
                        let shown = u32::try_from(page.games.len()).unwrap_or(0);
                        view! {
                            <p class="count" role="status">
                                {if total == 1 { t!(i18n, library_count_one).into_any() } else { t!(i18n, library_count, count = move || total).into_any() }}
                            </p>
                            <div class="grid">
                                {page.games.into_iter().map(|game| view! { <GameCard game/> }).collect_view()}
                            </div>
                            {(shown < total && search.get_untracked().trim().is_empty()).then(|| view! {
                                <div class="more">
                                    <button type="button" on:click=move |_| limit.update(|l| *l += PAGE)>
                                        {t!(i18n, load_more)}
                                    </button>
                                </div>
                            })}
                        }.into_any()
                    }
                })}
            </Transition>
        </section>
    }
}
