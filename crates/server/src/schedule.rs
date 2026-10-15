//! Unattended daily sync (FR-019, US3).
//!
//! Every account that can sync is queued at each occurrence of the instance schedule. At start-up, accounts
//! whose last success is older than a day are caught up, so a server that was off at night misses nothing.

use chrono::Utc;
use chrono_tz::Tz;
use croner::Cron;
use gameshelf_app::server::AppState;
use gameshelf_store::RunTrigger;
use std::{str::FromStr, time::Duration};

/// Delay before the start-up catch-up, so the server is fully up first.
const CATCH_UP_DELAY: Duration = Duration::from_secs(60);
/// An account whose last success is older than this is synced at start-up.
const STALE_AFTER: time::Duration = time::Duration::hours(26);

#[derive(Debug, thiserror::Error)]
pub enum ScheduleError {
    #[error("GAMESHELF_SYNC_SCHEDULE is not a valid cron expression: {0}")]
    Cron(String),
    #[error("TZ is not a known time zone: {0}")]
    Zone(String),
}

/// A parsed schedule in the instance time zone.
#[derive(Clone, Debug)]
pub struct Schedule {
    cron: Cron,
    tz: Tz,
}

impl Schedule {
    pub fn parse(expr: &str, tz: Option<&str>) -> Result<Self, ScheduleError> {
        let cron = Cron::from_str(expr).map_err(|e| ScheduleError::Cron(e.to_string()))?;
        let tz = match tz {
            Some(name) => name
                .parse::<Tz>()
                .map_err(|_| ScheduleError::Zone(name.to_string()))?,
            None => Tz::UTC,
        };
        Ok(Self { cron, tz })
    }

    /// The next occurrence strictly after `now`.
    pub fn next_after(&self, now: chrono::DateTime<Utc>) -> Option<chrono::DateTime<Tz>> {
        self.cron
            .find_next_occurrence(&now.with_timezone(&self.tz), false)
            .ok()
    }
}

fn to_offset(at: &chrono::DateTime<Tz>) -> Option<time::OffsetDateTime> {
    use chrono::Offset;
    let offset = time::UtcOffset::from_whole_seconds(at.offset().fix().local_minus_utc()).ok()?;
    time::OffsetDateTime::from_unix_timestamp(at.timestamp())
        .ok()
        .map(|t| t.to_offset(offset))
}

/// Starts the schedule loop and the start-up catch-up.
pub fn spawn(state: AppState, schedule: Schedule) {
    let catch_up = state.clone();
    tokio::spawn(async move {
        tokio::time::sleep(CATCH_UP_DELAY).await;
        queue(&catch_up, RunTrigger::Startup, true).await;
    });
    tokio::spawn(async move {
        loop {
            let now = Utc::now();
            let Some(next) = schedule.next_after(now) else {
                tracing::error!("the sync schedule has no next occurrence; scheduled syncs are stopped");
                state.set_next_scheduled(None);
                return;
            };
            state.set_next_scheduled(to_offset(&next));
            let wait = (next.with_timezone(&Utc) - now)
                .to_std()
                .unwrap_or(Duration::from_secs(1));
            tracing::info!(next = %next, "next scheduled sync");
            tokio::time::sleep(wait).await;
            queue(&state, RunTrigger::Schedule, false).await;
            // Never fire twice for the same occurrence.
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });
}

/// Queues every syncable account; with `stale_only`, only those not synced successfully for a day.
async fn queue(state: &AppState, trigger: RunTrigger, stale_only: bool) {
    let accounts = match state.store.list_syncable_accounts().await {
        Ok(a) => a,
        Err(e) => {
            tracing::error!(error = %e, "cannot list accounts to sync");
            return;
        }
    };
    let now = time::OffsetDateTime::now_utc();
    let mut queued = 0;
    for account in accounts {
        let stale = account.last_success_at.is_none_or(|t| now - t > STALE_AFTER);
        if stale_only && !stale {
            continue;
        }
        if state.spawn_sync(account.id, trigger) {
            queued += 1;
        }
    }
    tracing::info!(queued, ?trigger, "accounts queued for sync");
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn default_schedule_runs_daily_at_four_local_time() {
        let s = Schedule::parse("0 4 * * *", Some("Europe/Paris")).expect("valid");
        let now = Utc
            .with_ymd_and_hms(2026, 10, 15, 12, 0, 0)
            .single()
            .expect("date");
        let next = s.next_after(now).expect("next");
        assert_eq!(next.to_rfc3339(), "2026-10-16T04:00:00+02:00");
        let offset = to_offset(&next).expect("offset");
        assert_eq!(offset.offset().whole_hours(), 2);
    }

    #[test]
    fn invalid_settings_are_refused() {
        assert!(matches!(
            Schedule::parse("not cron", None),
            Err(ScheduleError::Cron(_))
        ));
        assert!(matches!(
            Schedule::parse("0 4 * * *", Some("Mars/Olympus")),
            Err(ScheduleError::Zone(_))
        ));
    }
}
