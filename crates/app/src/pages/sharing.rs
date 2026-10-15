//! Sharing with other profiles, with exclusions by platform, account or game (US6).

use crate::{
    api::{SetExclusion, SetSharing, sharing_settings},
    components::PlatformIcon,
    i18n::*,
    types::SharingView,
};
use leptos::prelude::*;

#[component]
pub fn SharingPage() -> impl IntoView {
    let i18n = use_i18n();
    let settings = Resource::new(|| (), |_| sharing_settings());
    let share = ServerAction::<SetSharing>::new();
    let exclude = ServerAction::<SetExclusion>::new();
    Effect::new(move |_| {
        share.version().get();
        exclude.version().get();
        settings.refetch();
    });
    view! {
        <section class="sharing">
            <h1>{t!(i18n, sharing_title)}</h1>
            <p class="lead">{t!(i18n, sharing_intro)}</p>
            <Transition fallback=move || view! { <p class="loading">{t!(i18n, loading)}</p> }>
                {move || settings.get().map(|r| match r {
                    Err(_) => view! { <p class="error">{t!(i18n, error_generic)}</p> }.into_any(),
                    Ok(v) => view! { <SharingForm v share exclude/> }.into_any(),
                })}
            </Transition>
        </section>
    }
}

#[component]
fn SharingForm(
    v: SharingView,
    share: ServerAction<SetSharing>,
    exclude: ServerAction<SetExclusion>,
) -> impl IntoView {
    let i18n = use_i18n();
    let SharingView {
        others,
        platforms,
        accounts,
        hidden_games,
    } = v;
    view! {
        <h2>{t!(i18n, sharing_with)}</h2>
        {if others.is_empty() {
            view! { <p class="muted">{t!(i18n, sharing_no_profiles)}</p> }.into_any()
        } else {
            view! {
                <ul class="plain-list">
                    {others.into_iter().map(|p| {
                        let id = p.id;
                        view! {
                            <li>
                                <label class="toggle">
                                    <input type="checkbox" prop:checked=p.shared on:change=move |ev| {
                                        share.dispatch(SetSharing { audience: id, shared: event_target_checked(&ev) });
                                    }/>
                                    <span>{p.display_name}</span>
                                </label>
                            </li>
                        }
                    }).collect_view()}
                </ul>
            }.into_any()
        }}

        <h2>{t!(i18n, sharing_hide_platforms)}</h2>
        <ul class="plain-list">
            {platforms.into_iter().map(|p| {
                let key = p.key.clone();
                view! {
                    <li>
                        <label class="toggle">
                            <input type="checkbox" prop:checked=p.hidden on:change=move |ev| {
                                exclude.dispatch(SetExclusion { kind: "platform".into(), reference: key.clone(), hidden: event_target_checked(&ev) });
                            }/>
                            <PlatformIcon key=p.key.clone()/>
                            <span>{p.name}</span>
                        </label>
                    </li>
                }
            }).collect_view()}
        </ul>

        <h2>{t!(i18n, sharing_hide_accounts)}</h2>
        <ul class="plain-list">
            {accounts.into_iter().map(|a| {
                let id = a.id.to_string();
                view! {
                    <li>
                        <label class="toggle">
                            <input type="checkbox" prop:checked=a.hidden on:change=move |ev| {
                                exclude.dispatch(SetExclusion { kind: "account".into(), reference: id.clone(), hidden: event_target_checked(&ev) });
                            }/>
                            <PlatformIcon key=a.platform.clone()/>
                            <span>{a.label}</span>
                        </label>
                    </li>
                }
            }).collect_view()}
        </ul>

        <h2>{t!(i18n, sharing_hidden_games)}</h2>
        {if hidden_games.is_empty() {
            view! { <p class="muted">{t!(i18n, sharing_hidden_games_none)}</p> }.into_any()
        } else {
            view! {
                <ul class="plain-list">
                    {hidden_games.into_iter().map(|g| {
                        let id = g.id.to_string();
                        view! {
                            <li>
                                <a href=format!("/games/{}", g.id)>{g.title}</a>
                                " "
                                <button type="button" class="ghost" on:click=move |_| {
                                    exclude.dispatch(SetExclusion { kind: "game".into(), reference: id.clone(), hidden: false });
                                }>{t!(i18n, sharing_unhide)}</button>
                            </li>
                        }
                    }).collect_view()}
                </ul>
            }.into_any()
        }}
    }
}
