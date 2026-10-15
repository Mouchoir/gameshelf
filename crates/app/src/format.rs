//! Formatting helpers shared by pages.

use time::OffsetDateTime;

/// Playtime as hours above two hours, minutes below: `"124 h"`, `"45 min"`.
pub fn playtime(minutes: u32) -> String {
    if minutes >= 120 {
        format!("{} h", minutes / 60)
    } else if minutes >= 60 {
        format!("{} h {:02}", minutes / 60, minutes % 60)
    } else {
        format!("{minutes} min")
    }
}

/// Calendar date in ISO form, readable in every language.
pub fn date(at: OffsetDateTime) -> String {
    at.date().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playtime_formats() {
        assert_eq!(playtime(0), "0 min");
        assert_eq!(playtime(45), "45 min");
        assert_eq!(playtime(75), "1 h 15");
        assert_eq!(playtime(7440), "124 h");
    }
}
