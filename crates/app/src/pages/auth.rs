//! First-run setup and sign-in.

use crate::{
    api::{Login, SetupInstance},
    i18n::*,
};
use leptos::prelude::*;

#[component]
pub fn SetupPage() -> impl IntoView {
    let i18n = use_i18n();
    let action = ServerAction::<SetupInstance>::new();
    Effect::new(move |_| {
        if let Some(Ok(())) = action.value().get() {
            let _ = window().location().set_href("/");
        }
    });
    let error = move || {
        action
            .value()
            .get()
            .and_then(|r| r.err())
            .map(|e| clean_error(&e.to_string()))
    };
    view! {
        <main id="main" class="auth">
            <h1>{t!(i18n, setup_title)}</h1>
            <p class="lead">{t!(i18n, setup_intro)}</p>
            <ActionForm action=action attr:class="form">
                <label>
                    <span>{t!(i18n, field_username)}</span>
                    <input name="username" required autocomplete="username" minlength="3" maxlength="32" pattern="[a-z0-9._\\-]+"/>
                    <small>{t!(i18n, field_username_hint)}</small>
                </label>
                <label>
                    <span>{t!(i18n, field_display_name)}</span>
                    <input name="display_name" required maxlength="64" autocomplete="nickname"/>
                </label>
                <label>
                    <span>{t!(i18n, field_password)}</span>
                    <input name="password" type="password" required minlength="10" autocomplete="new-password"/>
                    <small>{t!(i18n, field_password_hint)}</small>
                </label>
                {move || error().map(|e| view! { <p class="error" role="alert">{e}</p> })}
                <button type="submit" class="primary" disabled=move || action.pending().get()>
                    {t!(i18n, setup_submit)}
                </button>
            </ActionForm>
        </main>
    }
}

#[component]
pub fn LoginPage() -> impl IntoView {
    let i18n = use_i18n();
    let action = ServerAction::<Login>::new();
    Effect::new(move |_| {
        if let Some(Ok(true)) = action.value().get() {
            let _ = window().location().set_href("/");
        }
    });
    let failed = move || matches!(action.value().get(), Some(Ok(false)) | Some(Err(_)));
    view! {
        <main id="main" class="auth">
            <h1>{t!(i18n, login_title)}</h1>
            <ActionForm action=action attr:class="form">
                <label>
                    <span>{t!(i18n, field_username)}</span>
                    <input name="username" required autocomplete="username"/>
                </label>
                <label>
                    <span>{t!(i18n, field_password)}</span>
                    <input name="password" type="password" required autocomplete="current-password"/>
                </label>
                <Show when=failed>
                    <p class="error" role="alert">{t!(i18n, login_failed)}</p>
                </Show>
                <button type="submit" class="primary" disabled=move || action.pending().get()>
                    {t!(i18n, login_submit)}
                </button>
            </ActionForm>
        </main>
    }
}

/// Strips the server function error prefix for display.
pub(crate) fn clean_error(e: &str) -> String {
    e.rsplit(": ").next().unwrap_or(e).to_string()
}
