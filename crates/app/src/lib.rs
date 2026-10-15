//! The gameshelf web application: routes, components and server functions.
//!
//! Compiled twice: with `ssr` inside the server binary and with `hydrate` to WebAssembly.

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
