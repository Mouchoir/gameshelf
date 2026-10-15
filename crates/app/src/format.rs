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

/// Date and time with its offset, for schedules: `2026-10-16 04:00 UTC+02:00`.
pub fn date_time(at: OffsetDateTime) -> String {
    let off = at.offset();
    let zone = if off.is_utc() {
        "UTC".to_string()
    } else {
        let (h, m, _) = off.as_hms();
        let sign = if h < 0 || m < 0 { '-' } else { '+' };
        format!("UTC{sign}{:02}:{:02}", h.unsigned_abs(), m.unsigned_abs())
    };
    format!("{} {:02}:{:02} {zone}", at.date(), at.hour(), at.minute())
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

    #[test]
    fn date_time_shows_the_offset() {
        let at = time::macros::datetime!(2026-10-16 04:00 +02:00);
        assert_eq!(date_time(at), "2026-10-16 04:00 UTC+02:00");
        assert_eq!(
            date_time(time::macros::datetime!(2026-10-16 04:00 UTC)),
            "2026-10-16 04:00 UTC"
        );
    }
}
