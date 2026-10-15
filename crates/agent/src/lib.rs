//! The gameshelf agent: reads the launchers installed on this computer and reports their games to a gameshelf
//! server (contracts/agent-api.md). It only reads launcher files, sends nothing from a source the player
//! turned off, and does nothing until it is paired.

pub mod autostart;
pub mod client;
pub mod config;
pub mod messages;
pub mod scan;
pub mod service;
