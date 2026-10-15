//! Identifiers and enumerations shared by every crate (data-model.md).

use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

macro_rules! id_type {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub Uuid);

        impl $name {
            /// Creates a new time-ordered identifier (UUIDv7).
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }

        impl std::str::FromStr for $name {
            type Err = uuid::Error;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Uuid::parse_str(s).map(Self)
            }
        }
    };
}

id_type!(
    /// A person using the instance.
    ProfileId
);
id_type!(
    /// One account on one platform.
    AccountId
);
id_type!(
    /// A canonical game.
    GameId
);
id_type!(
    /// A product as listed by one platform.
    ProductId
);
id_type!(
    /// The fact that one account has access to one product.
    OwnershipId
);
id_type!(
    /// A paired agent.
    AgentId
);
id_type!(
    /// One sync execution.
    RunId
);

/// How an account came to have access to a product (Constitution, Principle II).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Acquisition {
    Purchase,
    FreeClaim,
    Subscription,
    FreeToPlay,
    FamilyShared,
    Manual,
}

impl Acquisition {
    pub const ALL: [Acquisition; 6] = [
        Acquisition::Purchase,
        Acquisition::FreeClaim,
        Acquisition::Subscription,
        Acquisition::FreeToPlay,
        Acquisition::FamilyShared,
        Acquisition::Manual,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Acquisition::Purchase => "purchase",
            Acquisition::FreeClaim => "free_claim",
            Acquisition::Subscription => "subscription",
            Acquisition::FreeToPlay => "free_to_play",
            Acquisition::FamilyShared => "family_shared",
            Acquisition::Manual => "manual",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.as_str() == s)
    }

    /// Whether this acquisition means the account really owns a copy.
    /// Subscription and family sharing grant access without ownership.
    pub fn counts_as_owned(self) -> bool {
        !matches!(self, Acquisition::Subscription | Acquisition::FamilyShared)
    }
}

/// What a platform product is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductKind {
    Game,
    Dlc,
    Bundle,
    Tool,
    Unknown,
}

impl ProductKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ProductKind::Game => "game",
            ProductKind::Dlc => "dlc",
            ProductKind::Bundle => "bundle",
            ProductKind::Tool => "tool",
            ProductKind::Unknown => "unknown",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [
            ProductKind::Game,
            ProductKind::Dlc,
            ProductKind::Bundle,
            ProductKind::Tool,
            ProductKind::Unknown,
        ]
        .into_iter()
        .find(|k| k.as_str() == s)
    }
}

/// Role of a profile on the instance (FR-031).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Admin,
    Member,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Admin => "admin",
            Role::Member => "member",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "admin" => Some(Role::Admin),
            "member" => Some(Role::Member),
            _ => None,
        }
    }
}

/// Interface language preference (FR-045).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum LocalePref {
    #[default]
    Auto,
    En,
    Fr,
}

impl LocalePref {
    pub fn as_str(self) -> &'static str {
        match self {
            LocalePref::Auto => "auto",
            LocalePref::En => "en",
            LocalePref::Fr => "fr",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "auto" => Some(LocalePref::Auto),
            "en" => Some(LocalePref::En),
            "fr" => Some(LocalePref::Fr),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acquisition_round_trips_through_strings() {
        for a in Acquisition::ALL {
            assert_eq!(Acquisition::parse(a.as_str()), Some(a));
        }
        assert_eq!(Acquisition::parse("stolen"), None);
    }

    #[test]
    fn subscription_and_family_sharing_are_not_ownership() {
        assert!(Acquisition::Purchase.counts_as_owned());
        assert!(Acquisition::FreeToPlay.counts_as_owned());
        assert!(!Acquisition::Subscription.counts_as_owned());
        assert!(!Acquisition::FamilyShared.counts_as_owned());
    }

    #[test]
    fn ids_are_time_ordered() {
        let a = GameId::new();
        let b = GameId::new();
        assert!(a < b);
    }
}
