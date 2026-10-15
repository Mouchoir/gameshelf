//! Server-only parts of the application: shared state and authentication helpers.

pub mod agent_api;
pub mod auth;
pub mod notifications;
pub mod state;

pub use state::AppState;
