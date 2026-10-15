//! Localized notification texts (Constitution, Principle VII). English is the reference locale.

use crate::{Message, Priority};

/// Games listed by name in one message before "and N more".
pub const MAX_LISTED: usize = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    En,
    Fr,
}

impl Lang {
    /// `"fr"` gives French; anything else, including `"auto"`, gives English.
    pub fn from_code(code: &str) -> Self {
        if code.eq_ignore_ascii_case("fr") {
            Lang::Fr
        } else {
            Lang::En
        }
    }
}

fn link(base: Option<&str>, path: &str) -> Option<String> {
    base.map(|b| format!("{}{path}", b.trim_end_matches('/')))
}

fn list(lang: Lang, lines: &[String]) -> String {
    let mut out: Vec<String> = lines.iter().take(MAX_LISTED).cloned().collect();
    if lines.len() > MAX_LISTED {
        let more = lines.len() - MAX_LISTED;
        out.push(match lang {
            Lang::En => format!("and {more} more"),
            Lang::Fr => format!("et {more} de plus"),
        });
    }
    out.join("\n")
}

/// New games found by one sync run. `lines` are "Title (Platform, account Label)".
pub fn games_added(lang: Lang, lines: &[String], base: Option<&str>) -> Message {
    let n = lines.len();
    let title = match (lang, n) {
        (Lang::En, 1) => "1 new game".to_string(),
        (Lang::En, _) => format!("{n} new games"),
        (Lang::Fr, 1) => "1 nouveau jeu".to_string(),
        (Lang::Fr, _) => format!("{n} nouveaux jeux"),
    };
    Message {
        title,
        body: list(lang, lines),
        url: link(base, "/activity"),
        priority: Priority::Normal,
    }
}

/// Games no longer available on an account.
pub fn games_unavailable(lang: Lang, lines: &[String], base: Option<&str>) -> Message {
    let n = lines.len();
    let title = match (lang, n) {
        (Lang::En, 1) => "1 game no longer available".to_string(),
        (Lang::En, _) => format!("{n} games no longer available"),
        (Lang::Fr, 1) => "1 jeu n'est plus disponible".to_string(),
        (Lang::Fr, _) => format!("{n} jeux ne sont plus disponibles"),
    };
    Message {
        title,
        body: list(lang, lines),
        url: link(base, "/activity"),
        priority: Priority::Normal,
    }
}

/// An account needs the player to sign in again.
pub fn needs_reconnect(lang: Lang, platform: &str, account: &str, base: Option<&str>) -> Message {
    let (title, body) = match lang {
        Lang::En => (
            format!("{platform}: reconnect \"{account}\""),
            format!(
                "gameshelf can no longer read the {platform} account \"{account}\". Open the Accounts page to reconnect it."
            ),
        ),
        Lang::Fr => (
            format!("{platform} : reconnecter « {account} »"),
            format!(
                "gameshelf n'arrive plus à lire le compte {platform} « {account} ». Ouvrez la page Comptes pour le reconnecter."
            ),
        ),
    };
    Message {
        title,
        body,
        url: link(base, "/accounts"),
        priority: Priority::High,
    }
}

/// An account failed three syncs in a row.
pub fn degraded(lang: Lang, platform: &str, account: &str, base: Option<&str>) -> Message {
    let (title, body) = match lang {
        Lang::En => (
            format!("{platform}: sync trouble for \"{account}\""),
            format!(
                "The last syncs of the {platform} account \"{account}\" failed. Its games stay visible; details are on the Accounts page."
            ),
        ),
        Lang::Fr => (
            format!("{platform} : synchro en difficulté pour « {account} »"),
            format!(
                "Les dernières synchros du compte {platform} « {account} » ont échoué. Ses jeux restent visibles ; le détail est sur la page Comptes."
            ),
        ),
    };
    Message {
        title,
        body,
        url: link(base, "/accounts"),
        priority: Priority::Normal,
    }
}

/// An agent missed two scans in a row.
pub fn agent_silent(lang: Lang, agent: &str, base: Option<&str>) -> Message {
    let (title, body) = match lang {
        Lang::En => (
            format!("Agent \"{agent}\" is silent"),
            format!(
                "The agent on \"{agent}\" has not been in touch for more than two scan intervals. Its games stay visible; check that the computer is on and the agent running."
            ),
        ),
        Lang::Fr => (
            format!("L'agent « {agent} » ne répond plus"),
            format!(
                "L'agent de « {agent} » n'a pas donné de nouvelles depuis plus de deux intervalles d'analyse. Ses jeux restent visibles ; vérifiez que l'ordinateur est allumé et l'agent lancé."
            ),
        ),
    };
    Message {
        title,
        body,
        url: link(base, "/agents"),
        priority: Priority::Normal,
    }
}

/// The "Send test" message.
pub fn test(lang: Lang, base: Option<&str>) -> Message {
    let (title, body) = match lang {
        Lang::En => ("gameshelf test", "Notifications reach this destination."),
        Lang::Fr => ("Test gameshelf", "Les notifications arrivent bien ici."),
    };
    Message {
        title: title.into(),
        body: body.into(),
        url: link(base, "/notifications"),
        priority: Priority::Normal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grouping_lists_ten_then_counts_the_rest() {
        let lines: Vec<String> = (1..=13)
            .map(|i| format!("Game {i} (Steam, account Main)"))
            .collect();
        let m = games_added(Lang::En, &lines, Some("https://gameshelf.example.com/"));
        assert_eq!(m.title, "13 new games");
        assert!(m.body.ends_with("and 3 more"));
        assert_eq!(m.body.lines().count(), 11);
        assert_eq!(m.url.as_deref(), Some("https://gameshelf.example.com/activity"));
    }

    #[test]
    fn french_texts() {
        let m = games_added(Lang::Fr, &["Hades (Steam, compte Principal)".into()], None);
        assert_eq!(m.title, "1 nouveau jeu");
        assert!(m.url.is_none());
        let r = needs_reconnect(Lang::Fr, "Epic Games Store", "Second", None);
        assert_eq!(r.priority, Priority::High);
        assert!(r.title.contains("« Second »"));
    }

    #[test]
    fn unknown_locales_fall_back_to_english() {
        assert_eq!(Lang::from_code("auto"), Lang::En);
        assert_eq!(Lang::from_code("FR"), Lang::Fr);
    }
}
