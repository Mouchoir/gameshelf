//! Reusable components: header, platform icons, account markers, game cards.

use crate::{
    api::{Logout, SetLanguage},
    i18n::*,
    types::Me,
};
use gameshelf_core::{
    Acquisition,
    account_status::AccountStatus,
    platform::{self, Tier},
    view::LibraryGame,
};
use leptos::prelude::*;
use leptos_router::components::A;

/// Top bar with navigation, language switch and sign-out.
#[component]
pub fn Header(me: Me) -> impl IntoView {
    let i18n = use_i18n();
    let is_admin = me.role == gameshelf_core::Role::Admin;
    let logout = ServerAction::<Logout>::new();
    Effect::new(move |_| {
        if let Some(Ok(())) = logout.value().get() {
            let _ = window().location().set_href("/login");
        }
    });
    let save_language = ServerAction::<SetLanguage>::new();
    let switch = move |_| {
        let next = match i18n.get_locale() {
            Locale::en => Locale::fr,
            Locale::fr => Locale::en,
        };
        i18n.set_locale(next);
        let code = match next {
            Locale::en => "en",
            Locale::fr => "fr",
        };
        save_language.dispatch(SetLanguage {
            locale: code.to_string(),
        });
    };
    view! {
        <a class="skip-link" href="#main">{t!(i18n, skip_to_content)}</a>
        <header class="topbar">
            <A href="/" attr:class="brand">"gameshelf"</A>
            <nav aria-label="main">
                <A href="/">{t!(i18n, nav_library)}</A>
                <A href="/accounts">{t!(i18n, nav_accounts)}</A>
                <A href="/activity">{t!(i18n, nav_activity)}</A>
                <A href="/agents">{t!(i18n, nav_agents)}</A>
                <A href="/notifications">{t!(i18n, nav_notifications)}</A>
                <A href="/sharing">{t!(i18n, nav_sharing)}</A>
                {is_admin.then(|| view! { <A href="/admin">{t!(i18n, nav_admin)}</A> })}
            </nav>
            <div class="topbar-end">
                <span class="me">{me.display_name}</span>
                <button type="button" class="ghost" on:click=switch>{t!(i18n, language_switch)}</button>
                <ActionForm action=logout>
                    <button type="submit" class="ghost">{t!(i18n, nav_sign_out)}</button>
                </ActionForm>
            </div>
        </header>
    }
}

/// A platform logo drawn with the current text color (icon files under `/icons/`).
#[component]
pub fn PlatformIcon(#[prop(into)] key: String, #[prop(optional)] large: bool) -> impl IntoView {
    let info = platform::platform(&key);
    let name = info.map(|p| p.name).unwrap_or("?");
    let icon = info.map(|p| p.icon).unwrap_or("manual");
    view! {
        <span
            class=if large { "pi pi-lg" } else { "pi" }
            role="img"
            aria-label=name
            title=name
            style=format!("--icon: url(/icons/{icon}.svg)")
        ></span>
    }
}

/// Marks games held on several accounts (US1): nothing for one account, `×N` for several.
#[component]
pub fn AccountMarker(count: u32) -> impl IntoView {
    let i18n = use_i18n();
    (count > 1).then(|| {
        let label = format!("{count} {}", t_string!(i18n, accounts_word));
        view! { <span class="multi" title=label.clone() aria-label=label>{format!("×{count}")}</span> }
    })
}

/// One tile of the library grid.
#[component]
pub fn GameCard(game: LibraryGame, #[prop(optional)] owners: Vec<String>) -> impl IntoView {
    let href = format!("/games/{}", game.game_id);
    let title = game.title.clone();
    let cover = match game.cover {
        Some(sha) => {
            view! { <img src=format!("/assets/{sha}") alt="" loading="lazy" decoding="async"/> }.into_any()
        }
        None => view! { <span class="cover-title">{title.clone()}</span> }.into_any(),
    };
    let subscription_only = !game.owned;
    let platforms = game.platforms.clone();
    view! {
        <a class="card" href=href>
            <div class="cover" class:faded=subscription_only>{cover}</div>
            <div class="card-meta">
                <span class="card-title">{game.title}</span>
                <span class="card-badges">
                    {platforms.into_iter().map(|p| view! { <PlatformIcon key=p/> }).collect_view()}
                    <AccountMarker count=game.account_count/>
                    {subscription_only.then(|| view! { <SubscriptionChip/> })}
                    {game.acquisitions.contains(&Acquisition::FreeToPlay).then(|| view! { <F2pChip/> })}
                    {(!owners.is_empty()).then(|| view! { <span class="owner">{owners.join(", ")}</span> })}
                </span>
            </div>
        </a>
    }
}

#[component]
fn SubscriptionChip() -> impl IntoView {
    let i18n = use_i18n();
    view! { <span class="chip">{t!(i18n, acq_subscription)}</span> }
}

#[component]
fn F2pChip() -> impl IntoView {
    let i18n = use_i18n();
    let label = t_string!(i18n, acq_free_to_play).to_string();
    view! { <span class="chip" title=label>"F2P"</span> }
}

/// Localized acquisition label.
#[component]
pub fn AcquisitionLabel(acquisition: Acquisition) -> impl IntoView {
    let i18n = use_i18n();
    match acquisition {
        Acquisition::Purchase => t!(i18n, acq_purchase).into_any(),
        Acquisition::FreeClaim => t!(i18n, acq_free_claim).into_any(),
        Acquisition::Subscription => t!(i18n, acq_subscription).into_any(),
        Acquisition::FreeToPlay => t!(i18n, acq_free_to_play).into_any(),
        Acquisition::FamilyShared => t!(i18n, acq_family_shared).into_any(),
        Acquisition::Manual => t!(i18n, acq_manual).into_any(),
    }
}

/// Localized account status badge.
#[component]
pub fn StatusBadge(status: AccountStatus) -> impl IntoView {
    let i18n = use_i18n();
    let class = match status {
        AccountStatus::Active => "badge ok",
        AccountStatus::Pending => "badge",
        AccountStatus::NeedsReconnect => "badge warn",
        AccountStatus::Degraded => "badge warn",
        AccountStatus::Disabled => "badge muted",
    };
    let label = match status {
        AccountStatus::Active => t!(i18n, status_active).into_any(),
        AccountStatus::Pending => t!(i18n, status_pending).into_any(),
        AccountStatus::NeedsReconnect => t!(i18n, status_needs_reconnect).into_any(),
        AccountStatus::Degraded => t!(i18n, status_degraded).into_any(),
        AccountStatus::Disabled => t!(i18n, status_disabled).into_any(),
    };
    view! { <span class=class>{label}</span> }
}

/// Localized stability tier chip (FR-002).
#[component]
pub fn TierChip(tier: Tier) -> impl IntoView {
    let i18n = use_i18n();
    let label = match tier {
        Tier::Official => t!(i18n, tier_official).into_any(),
        Tier::Unofficial => t!(i18n, tier_unofficial).into_any(),
        Tier::Agent => t!(i18n, tier_agent).into_any(),
        Tier::ViaOther => t!(i18n, tier_via_other).into_any(),
        Tier::Manual => t!(i18n, tier_manual).into_any(),
    };
    view! { <span class=format!("chip tier-{}", tier.as_str())>{label}</span> }
}
