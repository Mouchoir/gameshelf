//! Domain types and pure logic of gameshelf.
//!
//! Nothing in this crate performs I/O, so matching, search, visibility and state transitions can be
//! tested without a database or network (Constitution, Principle II and research decision D1).

pub mod account_status;
pub mod agent;
pub mod matching;
pub mod model;
pub mod normalize;
pub mod platform;
pub mod search;
pub mod view;

pub use model::*;
