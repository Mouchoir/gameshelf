//! Conversions between domain identifiers and their text form in SQLite.

use crate::StoreError;
use uuid::Uuid;

/// Parses a hyphenated UUID read from the database.
pub(crate) fn parse_uuid(s: &str) -> Result<Uuid, StoreError> {
    Uuid::parse_str(s).map_err(|_| StoreError::Corrupt(format!("invalid id {s:?}")))
}

macro_rules! text_id {
    ($($name:ident),*) => {
        $(
            impl crate::ids::TextId for gameshelf_core::$name {
                fn txt(&self) -> String {
                    self.0.hyphenated().to_string()
                }

                fn from_txt(s: &str) -> Result<Self, StoreError> {
                    parse_uuid(s).map(gameshelf_core::$name)
                }
            }
        )*
    };
}

/// Text form of an identifier.
pub(crate) trait TextId: Sized {
    fn txt(&self) -> String;
    fn from_txt(s: &str) -> Result<Self, StoreError>;
}

text_id!(
    ProfileId,
    AccountId,
    GameId,
    ProductId,
    OwnershipId,
    AgentId,
    RunId
);

/// Parses an optional id column.
pub(crate) fn opt_id<T: TextId>(s: Option<String>) -> Result<Option<T>, StoreError> {
    s.as_deref().map(T::from_txt).transpose()
}
