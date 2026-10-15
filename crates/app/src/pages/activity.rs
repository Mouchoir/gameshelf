//! Activity feed (FR-024).

use crate::{api::activity, components::PlatformIcon, format, i18n::*};
use leptos::prelude::*;

#[component]
pub fn ActivityPage() -> impl IntoView {
    let i18n = use_i18n();
    let list = Resource::new(|| (), |_| activity());
    view! {
        <section class="activity">
            <h1>{t!(i18n, activity_title)}</h1>
            <Suspense fallback=move || view! { <p class="loading">{t!(i18n, loading)}</p> }>
                {move || list.get().map(|r| match r {
                    Err(_) => view! { <p class="error">{t!(i18n, error_generic)}</p> }.into_any(),
                    Ok(list) if list.is_empty() => view! { <p class="empty">{t!(i18n, activity_empty)}</p> }.into_any(),
                    Ok(list) => view! {
                        <ul class="feed">
                            {list.into_iter().map(|e| {
                                let what = match e.kind.as_str() {
                                    "game_added" => t!(i18n, activity_game_added).into_any(),
                                    "game_unavailable" => t!(i18n, activity_game_unavailable).into_any(),
                                    "account_needs_reconnect" => t!(i18n, activity_account_needs_reconnect).into_any(),
                                    "account_degraded" => t!(i18n, activity_account_degraded).into_any(),
                                    "agent_silent" => t!(i18n, activity_agent_silent).into_any(),
                                    _ => t!(i18n, activity_sync_failed).into_any(),
                                };
                                let game = match (e.game_id, e.game_title) {
                                    (Some(id), Some(title)) => Some(view! { <a href=format!("/games/{id}")>{title}</a> }),
                                    _ => None,
                                };
                                view! {
                                    <li>
                                        <time>{format::date(e.created_at)}</time>
                                        <span class="chip">{what}</span>
                                        {game}
                                        {e.platform.map(|p| view! { <PlatformIcon key=p/> })}
                                        {e.account_label.map(|l| view! { <span class="muted">{l}</span> })}
                                    </li>
                                }
                            }).collect_view()}
                        </ul>
                    }.into_any(),
                })}
            </Suspense>
        </section>
    }
}
