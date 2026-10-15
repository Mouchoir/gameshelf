//! Platform account health (data-model.md, "Platform account" state machine).

use serde::{Deserialize, Serialize};

/// Consecutive failed runs (other than authentication failures) after which an account is degraded.
pub const DEGRADED_AFTER_FAILURES: u32 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountStatus {
    Pending,
    Active,
    NeedsReconnect,
    Degraded,
    Disabled,
}

impl AccountStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            AccountStatus::Pending => "pending",
            AccountStatus::Active => "active",
            AccountStatus::NeedsReconnect => "needs_reconnect",
            AccountStatus::Degraded => "degraded",
            AccountStatus::Disabled => "disabled",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(AccountStatus::Pending),
            "active" => Some(AccountStatus::Active),
            "needs_reconnect" => Some(AccountStatus::NeedsReconnect),
            "degraded" => Some(AccountStatus::Degraded),
            "disabled" => Some(AccountStatus::Disabled),
            _ => None,
        }
    }

    /// Whether scheduled syncs should run for an account in this state.
    pub fn syncs(self) -> bool {
        matches!(self, AccountStatus::Active | AccountStatus::Degraded)
    }
}

/// Something that happened to an account.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccountEvent {
    /// A new credential was validated.
    CredentialValidated,
    /// A run succeeded.
    RunSucceeded,
    /// A run failed for a reason other than authentication. `consecutive` counts this failure.
    RunFailed { consecutive: u32 },
    /// The credential expired or was rejected.
    AuthFailed,
    /// The player disabled the account.
    Disabled,
    /// The player enabled the account again.
    Enabled,
}

/// Applies an event and returns the next state.
pub fn transition(current: AccountStatus, event: AccountEvent) -> AccountStatus {
    use AccountEvent as E;
    use AccountStatus as S;
    match (current, event) {
        (_, E::Disabled) => S::Disabled,
        (S::Disabled, E::Enabled) => S::Active,
        (S::Disabled, _) => S::Disabled,
        (_, E::CredentialValidated) => S::Active,
        (_, E::AuthFailed) => S::NeedsReconnect,
        (S::NeedsReconnect, _) => S::NeedsReconnect,
        (S::Pending, E::RunSucceeded) | (S::Active, E::RunSucceeded) | (S::Degraded, E::RunSucceeded) => {
            S::Active
        }
        (state, E::RunFailed { consecutive }) => {
            if consecutive >= DEGRADED_AFTER_FAILURES && state != S::Pending {
                S::Degraded
            } else {
                state
            }
        }
        (state, E::Enabled) => state,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use AccountEvent as E;
    use AccountStatus as S;

    #[test]
    fn pending_becomes_active_once_validated() {
        assert_eq!(transition(S::Pending, E::CredentialValidated), S::Active);
    }

    #[test]
    fn auth_failure_needs_reconnect_until_new_credential() {
        let s = transition(S::Active, E::AuthFailed);
        assert_eq!(s, S::NeedsReconnect);
        assert_eq!(transition(s, E::RunSucceeded), S::NeedsReconnect);
        assert_eq!(transition(s, E::CredentialValidated), S::Active);
    }

    #[test]
    fn three_failures_degrade_and_success_recovers() {
        let mut s = S::Active;
        for n in 1..DEGRADED_AFTER_FAILURES {
            s = transition(s, E::RunFailed { consecutive: n });
            assert_eq!(s, S::Active);
        }
        s = transition(
            s,
            E::RunFailed {
                consecutive: DEGRADED_AFTER_FAILURES,
            },
        );
        assert_eq!(s, S::Degraded);
        assert_eq!(transition(s, E::RunSucceeded), S::Active);
    }

    #[test]
    fn disabled_ignores_everything_but_enable() {
        let s = transition(S::Active, E::Disabled);
        assert_eq!(s, S::Disabled);
        assert_eq!(transition(s, E::RunSucceeded), S::Disabled);
        assert_eq!(transition(s, E::CredentialValidated), S::Disabled);
        assert_eq!(transition(s, E::Enabled), S::Active);
    }

    #[test]
    fn only_active_and_degraded_accounts_sync() {
        assert!(S::Active.syncs());
        assert!(S::Degraded.syncs());
        assert!(!S::NeedsReconnect.syncs());
        assert!(!S::Disabled.syncs());
        assert!(!S::Pending.syncs());
    }
}
