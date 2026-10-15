//! Profile administration (FR-031). Admins manage profiles but never read unshared libraries.

use crate::{
    api::{AdminCreateProfile, AdminDeleteProfile, AdminResetPassword, AdminSetDisabled, admin_profiles},
    i18n::*,
    pages::auth::clean_error,
    types::{Me, ProfileAdminView},
};
use gameshelf_core::Role;
use leptos::prelude::*;

#[component]
pub fn AdminPage() -> impl IntoView {
    let i18n = use_i18n();
    let me = use_context::<Me>();
    let is_admin = me.as_ref().is_some_and(|m| m.role == Role::Admin);
    let list = Resource::new(|| (), |_| admin_profiles());
    let create = ServerAction::<AdminCreateProfile>::new();
    let disable = ServerAction::<AdminSetDisabled>::new();
    let delete = ServerAction::<AdminDeleteProfile>::new();
    let reset = ServerAction::<AdminResetPassword>::new();
    Effect::new(move |_| {
        create.version().get();
        disable.version().get();
        delete.version().get();
        list.refetch();
    });
    let errors = move || {
        [
            create.value().get(),
            disable.value().get(),
            delete.value().get(),
            reset.value().get(),
        ]
        .into_iter()
        .flatten()
        .find_map(|r| r.err())
        .map(|e| clean_error(&e.to_string()))
    };
    if !is_admin {
        return view! { <crate::pages::NotFound/> }.into_any();
    }
    view! {
        <section class="admin">
            <h1>{t!(i18n, admin_title)}</h1>
            <p class="lead">{t!(i18n, admin_intro)}</p>
            {move || errors().map(|e| view! { <p class="error" role="alert">{e}</p> })}
            <Transition fallback=move || view! { <p class="loading">{t!(i18n, loading)}</p> }>
                {move || list.get().map(|r| match r {
                    Err(_) => view! { <p class="error">{t!(i18n, error_generic)}</p> }.into_any(),
                    Ok(list) => view! {
                        <ul class="account-list">
                            {list.into_iter().map(|p| view! { <ProfileItem p disable delete reset/> }).collect_view()}
                        </ul>
                    }.into_any(),
                })}
            </Transition>
            <h2>{t!(i18n, admin_add)}</h2>
            <ActionForm action=create attr:class="form narrow-form">
                <label><span>{t!(i18n, field_username)}</span>
                    <input name="username" required minlength="3" maxlength="32" pattern="[a-z0-9._\\-]+" autocomplete="off"/></label>
                <label><span>{t!(i18n, field_display_name)}</span><input name="display_name" required maxlength="64"/></label>
                <label><span>{t!(i18n, field_password)}</span>
                    <input name="password" type="password" required minlength="10" autocomplete="new-password"/>
                    <small>{t!(i18n, field_password_hint)}</small></label>
                <label class="toggle"><input type="checkbox" name="admin" value="true"/><span>{t!(i18n, admin_is_admin)}</span></label>
                <button type="submit" class="primary" disabled=move || create.pending().get()>{t!(i18n, admin_create)}</button>
            </ActionForm>
        </section>
    }
    .into_any()
}

#[component]
fn ProfileItem(
    p: ProfileAdminView,
    disable: ServerAction<AdminSetDisabled>,
    delete: ServerAction<AdminDeleteProfile>,
    reset: ServerAction<AdminResetPassword>,
) -> impl IntoView {
    let i18n = use_i18n();
    let id = p.id;
    let confirming = RwSignal::new(false);
    let resetting = RwSignal::new(false);
    let new_password = RwSignal::new(String::new());
    view! {
        <li class="account">
            <div class="account-main">
                <div>
                    <strong>{p.display_name.clone()}</strong>
                    <div class="muted small">{p.username.clone()}</div>
                </div>
                {(p.role == Role::Admin).then(|| view! { <span class="badge ok">{t!(i18n, admin_role_admin)}</span> })}
                {p.disabled.then(|| view! { <span class="badge muted">{t!(i18n, status_disabled)}</span> })}
            </div>
            <div class="actions">
                <button type="button" class="ghost" on:click=move |_| { disable.dispatch(AdminSetDisabled { profile: id, disabled: !p.disabled }); }>
                    {if p.disabled { t!(i18n, admin_enable).into_any() } else { t!(i18n, admin_disable).into_any() }}
                </button>
                <Show when=move || resetting.get() fallback=move || view! {
                    <button type="button" class="ghost" on:click=move |_| resetting.set(true)>{t!(i18n, admin_reset_password)}</button>
                }>
                    <form class="inline" on:submit=move |ev| {
                        ev.prevent_default();
                        reset.dispatch(AdminResetPassword { profile: id, password: new_password.get_untracked() });
                        resetting.set(false);
                        new_password.set(String::new());
                    }>
                        <input type="password" minlength="10" required autocomplete="new-password"
                            prop:value=move || new_password.get() on:input=move |ev| new_password.set(event_target_value(&ev))/>
                        <button type="submit">{t!(i18n, account_save)}</button>
                    </form>
                </Show>
                <Show when=move || confirming.get() fallback=move || view! {
                    <button type="button" class="ghost danger" on:click=move |_| confirming.set(true)>{t!(i18n, notif_delete)}</button>
                }>
                    <span class="warn-text small">{t!(i18n, admin_delete_warning)}</span>
                    <button type="button" class="danger" on:click=move |_| { delete.dispatch(AdminDeleteProfile { profile: id }); }>{t!(i18n, notif_delete)}</button>
                    <button type="button" class="ghost" on:click=move |_| confirming.set(false)>{t!(i18n, account_cancel)}</button>
                </Show>
            </div>
        </li>
    }
}
