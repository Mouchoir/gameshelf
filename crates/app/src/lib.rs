//! The gameshelf web application: routes, components and server functions.
//!
//! Compiled twice: with `ssr` inside the server binary and with `hydrate` to WebAssembly.

pub mod app;

pub use app::{App, shell};
