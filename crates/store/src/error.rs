//! Store errors. Messages never contain secrets.

use crate::seal::SealError;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
    #[error("database migration failed: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),
    #[error(transparent)]
    Seal(#[from] SealError),
    #[error("not found")]
    NotFound,
    #[error("{0}")]
    Invalid(String),
    /// The platform account is already connected by a profile.
    #[error("this account is already connected by {holder}")]
    AccountTaken { holder: String },
    #[error("the last active administrator cannot be removed or demoted")]
    LastAdmin,
    #[error("a profile with this username or email already exists")]
    ProfileExists,
    #[error("password hashing failed")]
    Hash,
    #[error("stored data is corrupted: {0}")]
    Corrupt(String),
}

impl StoreError {
    pub(crate) fn invalid(msg: impl Into<String>) -> Self {
        StoreError::Invalid(msg.into())
    }

    /// Whether the error comes from a unique constraint.
    pub(crate) fn is_unique_violation(err: &sqlx::Error) -> bool {
        matches!(err, sqlx::Error::Database(db) if db.is_unique_violation())
    }
}
