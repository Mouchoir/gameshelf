//! Repositories, one module per aggregate (data-model.md).

pub mod accounts;
pub mod activity;
pub mod catalog;
pub mod credentials;
pub mod ownership;
pub mod prefs;
pub mod profiles;
pub mod runs;

/// Current time in UTC, truncated to whole microseconds so values round-trip through text columns.
pub(crate) fn now() -> time::OffsetDateTime {
    let t = time::OffsetDateTime::now_utc();
    t.replace_nanosecond((t.nanosecond() / 1_000) * 1_000)
        .unwrap_or(t)
}
