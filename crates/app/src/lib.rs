//! The gameshelf web application: routes, components and server functions.
//!
//! Compiled twice: with `ssr` inside the server binary and with `hydrate` to WebAssembly.

// Release builds keep full Leptos view types (dev builds erase them), which nest deeper than the default limit.
#![recursion_limit = "512"]

pub mod api;
pub mod app;
pub mod components;
pub mod format;
pub mod pages;
#[cfg(feature = "ssr")]
pub mod server;
pub mod types;

include!(concat!(env!("OUT_DIR"), "/i18n/mod.rs"));

pub use app::{App, shell};
