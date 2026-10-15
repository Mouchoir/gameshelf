//! Game identity, metadata and artwork (research section 4 and D11).
//!
//! Providers use keys supplied by the operator; nothing is embedded (Constitution, Licensing section).

pub mod artwork;
pub mod igdb;

pub use artwork::{ArtworkCache, ArtworkError, StoredImage};
pub use igdb::{Igdb, IgdbError, IgdbGame};
