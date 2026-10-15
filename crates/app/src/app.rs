//! Application shell, router and authentication gate.

use crate::{
    api::get_auth_state,
    i18n::*,
    pages,
    types::{AuthState, Me},
};
use leptos::prelude::*;
use leptos_meta::{MetaTags, Stylesheet, Title, provide_meta_context};
use leptos_router::{
    components::{Outlet, ParentRoute, Redirect, Route, Router, Routes},
    path,
};

/// The HTML document rendered by the server around the application.
pub fn shell(options: LeptosOptions) -> impl IntoView {
    view! {
        <!DOCTYPE html>
        <html lang="en">
            <head>
                <meta charset="utf-8"/>
                <meta name="viewport" content="width=device-width, initial-scale=1"/>
                <meta name="color-scheme" content="light dark"/>
                <link rel="icon" href="/favicon.svg" type="image/svg+xml"/>
                <AutoReload options=options.clone()/>
                <HydrationScripts options/>
                <MetaTags/>
            </head>
            <body>
                <App/>
            </body>
        </html>
    }
}

/// Root component.
#[component]
pub fn App() -> impl IntoView {
    provide_meta_context();
    let auth = Resource::new(|| (), |_| get_auth_state());
    provide_context(auth);

    view! {
        <Stylesheet id="leptos" href="/pkg/gameshelf.css"/>
        <Title text="gameshelf"/>
        <I18nContextProvider>
            <Router>
                <Routes fallback=|| view! { <pages::NotFound/> }>
                    <Route path=path!("/setup") view=pages::SetupPage/>
                    <Route path=path!("/login") view=pages::LoginPage/>
                    <ParentRoute path=path!("") view=Protected>
                        <Route path=path!("") view=pages::LibraryPage/>
                        <Route path=path!("/games/:id") view=pages::GamePage/>
                        <Route path=path!("/accounts") view=pages::AccountsPage/>
                        <Route path=path!("/accounts/connect") view=pages::ConnectPickerPage/>
                        <Route path=path!("/accounts/connect/:platform") view=pages::ConnectPlatformPage/>
                        <Route path=path!("/activity") view=pages::ActivityPage/>
                        <Route path=path!("/notifications") view=pages::NotificationsPage/>
                        <Route path=path!("/sharing") view=pages::SharingPage/>
                        <Route path=path!("/admin") view=pages::AdminPage/>
                    </ParentRoute>
                </Routes>
            </Router>
        </I18nContextProvider>
    }
}

/// The auth resource provided by [`App`].
pub type AuthResource = Resource<Result<AuthState, ServerFnError>>;

/// Renders its routes only for a signed-in profile; otherwise sends the visitor to setup or sign-in.
#[component]
fn Protected() -> impl IntoView {
    let auth = expect_context::<AuthResource>();
    let i18n = use_i18n();
    view! {
        <Suspense fallback=move || view! { <p class="loading">{t!(i18n, loading)}</p> }>
            {move || Suspend::new(async move {
                match auth.await {
                    Ok(AuthState { needs_setup: true, .. }) => view! { <Redirect path="/setup"/> }.into_any(),
                    Ok(AuthState { me: Some(me), .. }) => view! { <SignedIn me/> }.into_any(),
                    _ => view! { <Redirect path="/login"/> }.into_any(),
                }
            })}
        </Suspense>
    }
}

#[component]
fn SignedIn(me: Me) -> impl IntoView {
    provide_context(me.clone());
    view! {
        <crate::components::Header me/>
        <main id="main">
            <Outlet/>
        </main>
    }
}
