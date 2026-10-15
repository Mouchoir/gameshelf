//! Game detail: every ownership by platform and account, playtime, achievements, DLC (US1, US4).

use crate::{
    api::{SetExclusion, fetch_game_detail},
    components::{AcquisitionLabel, PlatformIcon},
    format,
    i18n::*,
    pages::NotFound,
    types::{GameDetail, OwnershipRow},
};
use gameshelf_core::{GameId, platform};
use leptos::prelude::*;
use leptos_router::hooks::use_params_map;
use std::collections::BTreeMap;

#[component]
pub fn GamePage() -> impl IntoView {
    let i18n = use_i18n();
    let params = use_params_map();
    let id = move || params.read().get("id").and_then(|s| s.parse::<GameId>().ok());
    let detail = Resource::new(id, |id| async move {
        match id {
            Some(id) => fetch_game_detail(id).await,
            None => Ok(None),
        }
    });
    view! {
        <Suspense fallback=move || view! { <p class="loading">{t!(i18n, loading)}</p> }>
            {move || detail.get().map(|r| match r {
                Ok(Some(d)) => view! { <GameView detail=d/> }.into_any(),
                Ok(None) => view! { <NotFound/> }.into_any(),
                Err(_) => view! { <p class="error">{t!(i18n, error_generic)}</p> }.into_any(),
            })}
        </Suspense>
    }
}

#[component]
fn GameView(detail: GameDetail) -> impl IntoView {
    let i18n = use_i18n();
    let summary = detail.summary;
    let cover = summary.cover.clone();
    let enrichment = detail.enrichment.clone();
    let game_id = summary.id;
    let mine = detail.ownerships.iter().any(|o| o.ownership.mine);
    let hidden = RwSignal::new(detail.hidden_from_family);
    let toggle_hidden = ServerAction::<SetExclusion>::new();
    let mut by_platform: BTreeMap<String, Vec<OwnershipRow>> = BTreeMap::new();
    for row in detail.ownerships {
        by_platform
            .entry(row.ownership.platform.clone())
            .or_default()
            .push(row);
    }
    view! {
        <article class="game">
            <p class="crumbs"><a href="/">{t!(i18n, game_back)}</a></p>
            <div class="game-head">
                <div class="game-cover">
                    {match cover {
                        Some(sha) => view! { <img src=format!("/assets/{sha}") alt=""/> }.into_any(),
                        None => view! { <span class="cover-title">{summary.title.clone()}</span> }.into_any(),
                    }}
                </div>
                <div class="game-info">
                    <h1>{summary.title.clone()}</h1>
                    {summary.release_year.map(|y| view! { <p class="muted">{y}</p> })}
                    {mine.then(|| view! {
                        <button type="button" class="ghost" on:click=move |_| {
                            let next = !hidden.get_untracked();
                            toggle_hidden.dispatch(SetExclusion { kind: "game".into(), reference: game_id.to_string(), hidden: next });
                            hidden.set(next);
                        }>
                            {move || if hidden.get() { t!(i18n, game_unhide).into_any() } else { t!(i18n, game_hide).into_any() }}
                        </button>
                    })}
                    <h2>{t!(i18n, game_owned_on)}</h2>
                    {by_platform.into_iter().map(|(platform_key, rows)| {
                        let name = platform::platform(&platform_key).map(|p| p.name).unwrap_or("?");
                        let enrichment = enrichment.clone();
                        view! {
                            <section class="platform-block">
                                <h3><PlatformIcon key=platform_key.clone() large=true/>" "{name}</h3>
                                <ul class="ownerships">
                                    {rows.into_iter().map(|row| {
                                        let values: Vec<_> = enrichment
                                            .iter()
                                            .filter(|e| e.account_id == row.ownership.account_id)
                                            .cloned()
                                            .collect();
                                        view! { <OwnershipItem row values/> }
                                    }).collect_view()}
                                </ul>
                            </section>
                        }
                    }).collect_view()}
                </div>
            </div>
            {(!detail.dlc.is_empty()).then(|| view! {
                <section class="dlc">
                    <h2>{t!(i18n, game_dlc)}</h2>
                    <ul>{detail.dlc.into_iter().map(|d| view! { <li>{d.title}</li> }).collect_view()}</ul>
                </section>
            })}
        </article>
    }
}

#[component]
fn OwnershipItem(row: OwnershipRow, values: Vec<crate::types::EnrichmentRow>) -> impl IntoView {
    let i18n = use_i18n();
    let o = row.ownership;
    let not_provided = move || view! { <span class="muted">{t!(i18n, game_not_provided)}</span> };
    let playtime = o.playtime_minutes.map(format::playtime);
    let last_played = o.last_played_at.map(format::date);
    let first_seen = format::date(o.first_seen_at);
    let unavailable = o.unavailable_since.map(format::date);
    let gone = unavailable.is_some();
    view! {
        <li class="ownership" class:gone=gone>
            <div class="ownership-head">
                <strong>{o.account_label.clone()}</strong>
                {row.owner_name.map(|n| view! { <span class="chip">{t!(i18n, game_owned_by, name = move || n.clone())}</span> })}
                <span class="chip"><AcquisitionLabel acquisition=o.acquisition/></span>
                {o.edition.clone().map(|e| view! { <span class="chip">{e}</span> })}
            </div>
            <dl class="facts">
                <div><dt>{t!(i18n, game_playtime)}</dt><dd>{match playtime { Some(p) => p.into_any(), None => not_provided().into_any() }}</dd></div>
                <div><dt>{t!(i18n, game_last_played)}</dt><dd>{match last_played { Some(d) => d.into_any(), None => t!(i18n, never).into_any() }}</dd></div>
                {o.achievements.map(|(u, t)| view! {
                    <div><dt>{t!(i18n, game_achievements)}</dt><dd>{format!("{u} / {t}")}</dd></div>
                })}
                <div><dt>{t!(i18n, game_first_seen)}</dt><dd>{first_seen}</dd></div>
                {values.into_iter().map(|v| {
                    let date = format::date(v.refreshed_at);
                    view! {
                        <div>
                            <dt>{match v.key.as_str() { "platform_level" => t!(i18n, game_platform_level).into_any(), other => other.to_string().into_any() }}</dt>
                            <dd>{v.value}" "<small class="muted">{t!(i18n, game_refreshed, date = move || date.clone())}</small></dd>
                        </div>
                    }
                }).collect_view()}
            </dl>
            {unavailable.map(|d| view! { <p class="muted">{t!(i18n, game_unavailable, date = move || d.clone())}</p> })}
        </li>
    }
}
