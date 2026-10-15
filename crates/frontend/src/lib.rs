//! Hydrates the server-rendered page in the browser.

// Release builds keep full Leptos view types (dev builds erase them), which nest deeper than the default limit.
#![recursion_limit = "512"]

use wasm_bindgen::prelude::wasm_bindgen;

#[wasm_bindgen]
pub fn hydrate() {
    console_error_panic_hook::set_once();
    leptos::mount::hydrate_body(gameshelf_app::App);
}
