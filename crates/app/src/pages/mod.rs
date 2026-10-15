//! Pages, one module per area.

mod accounts;
mod activity;
mod auth;
mod game;
mod library;
mod notifications;

pub use accounts::{AccountsPage, ConnectPickerPage, ConnectPlatformPage};
pub use activity::ActivityPage;
pub use auth::{LoginPage, SetupPage};
pub use game::GamePage;
pub use library::LibraryPage;
pub use notifications::NotificationsPage;

use crate::i18n::*;
use leptos::prelude::*;

/// Shown for unknown routes and for games the viewer may not see (SC-011).
#[component]
pub fn NotFound() -> impl IntoView {
    #[cfg(feature = "ssr")]
    {
        if let Some(resp) = use_context::<leptos_axum::ResponseOptions>() {
            resp.set_status(axum::http::StatusCode::NOT_FOUND);
        }
    }
    let i18n = use_i18n();
    view! {
        <main id="main" class="narrow">
            <h1>{t!(i18n, not_found_title)}</h1>
            <p>{t!(i18n, not_found_body)}</p>
            <p><a href="/">{t!(i18n, game_back)}</a></p>
        </main>
    }
}
