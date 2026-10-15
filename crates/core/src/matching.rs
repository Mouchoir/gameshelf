//! Matching platform products to canonical games (FR-007 to FR-010, research section 4).
//!
//! Order of evidence: identifiers (the platform's own id and cross-reference ids), then the normalized title
//! with the edition removed and a compatible release year. Player decisions (merge, split) are applied on top
//! by the caller and always win.

use crate::{GameId, normalize};

/// Edition suffixes grouped under one game (spec edge cases). Remasters, definitive editions and director's
/// cuts are separate products and are deliberately absent.
const EDITIONS: &[&str] = &[
    "game of the year edition",
    "goty edition",
    "goty",
    "digital deluxe edition",
    "deluxe edition",
    "gold edition",
    "ultimate edition",
    "complete edition",
    "premium edition",
    "standard edition",
    "collector's edition",
    "collectors edition",
    "special edition",
    "legendary edition",
    "champion edition",
];

/// Splits an edition suffix off a title: `"Hades II - Deluxe Edition"` gives `("Hades II", Some("Deluxe Edition"))`.
pub fn split_edition(title: &str) -> (String, Option<String>) {
    let trimmed = title.trim();
    let lower = trimmed.to_lowercase();
    if lower.len() != trimmed.len() {
        // Lowercasing changed byte lengths: byte offsets would not line up, keep the title whole.
        return (trimmed.to_string(), None);
    }
    for edition in EDITIONS {
        for sep in [" - ", ": ", " – ", " (", " "] {
            let suffix = format!("{sep}{edition}");
            let candidate = lower.strip_suffix(')').filter(|_| sep == " (").unwrap_or(&lower);
            if candidate.ends_with(&suffix) {
                let cut = candidate.len() - suffix.len();
                let base = trimmed[..cut].trim_end_matches([' ', '-', ':', '–']).trim();
                if base.is_empty() {
                    continue;
                }
                let original = &trimmed[cut + sep.len()..candidate.len()];
                return (base.to_string(), Some(original.trim().to_string()));
            }
        }
    }
    (trimmed.to_string(), None)
}

/// The key used for title matching: normalized title without the edition.
pub fn match_key(title: &str) -> String {
    normalize::search_key(&split_edition(title).0)
}

/// A game found by title.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TitleCandidate {
    pub game: GameId,
    pub release_year: Option<i32>,
}

/// Chooses the game a product belongs to, or `None` to create a new one.
///
/// * `by_identifier`: games already known under one of the product's identifiers, in order of trust.
/// * `by_title`: games whose match key equals the product's.
pub fn choose(
    by_identifier: &[GameId],
    by_title: &[TitleCandidate],
    release_year: Option<i32>,
) -> Option<GameId> {
    if let Some(first) = by_identifier.first() {
        return Some(*first);
    }
    let compatible: Vec<&TitleCandidate> = by_title
        .iter()
        .filter(|c| match (c.release_year, release_year) {
            (Some(a), Some(b)) => (a - b).abs() <= 1,
            _ => true,
        })
        .collect();
    match compatible.as_slice() {
        [only] => Some(only.game),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editions_are_split_off() {
        assert_eq!(
            split_edition("Hades II - Deluxe Edition"),
            ("Hades II".into(), Some("Deluxe Edition".into()))
        );
        assert_eq!(
            split_edition("The Witcher 3: Wild Hunt - Game of the Year Edition"),
            (
                "The Witcher 3: Wild Hunt".into(),
                Some("Game of the Year Edition".into())
            )
        );
        assert_eq!(
            split_edition("Borderlands 2 (GOTY)"),
            ("Borderlands 2".into(), Some("GOTY".into()))
        );
        assert_eq!(split_edition("Elden Ring"), ("Elden Ring".into(), None));
    }

    #[test]
    fn remasters_stay_separate() {
        assert_eq!(match_key("BioShock Remastered"), "bioshock remastered");
        assert_ne!(match_key("BioShock Remastered"), match_key("BioShock"));
        assert_ne!(
            match_key("Age of Empires II: Definitive Edition"),
            match_key("Age of Empires II")
        );
    }

    #[test]
    fn edition_and_standard_share_a_key() {
        assert_eq!(match_key("Hades II - Deluxe Edition"), match_key("Hades II"));
        assert_eq!(match_key("Borderlands 2 GOTY"), match_key("Borderlands 2"));
    }

    #[test]
    fn a_title_that_is_only_an_edition_word_is_kept() {
        assert_eq!(split_edition("Deluxe"), ("Deluxe".into(), None));
    }

    #[test]
    fn identifiers_win_over_titles() {
        let a = GameId::new();
        let b = GameId::new();
        let cands = [TitleCandidate {
            game: b,
            release_year: None,
        }];
        assert_eq!(choose(&[a], &cands, None), Some(a));
    }

    #[test]
    fn a_single_compatible_title_matches() {
        let g = GameId::new();
        assert_eq!(
            choose(
                &[],
                &[TitleCandidate {
                    game: g,
                    release_year: Some(2017)
                }],
                Some(2018)
            ),
            Some(g)
        );
        assert_eq!(
            choose(
                &[],
                &[TitleCandidate {
                    game: g,
                    release_year: Some(2007)
                }],
                Some(2018)
            ),
            None
        );
    }

    #[test]
    fn ambiguous_titles_create_a_new_game() {
        let cands = [
            TitleCandidate {
                game: GameId::new(),
                release_year: None,
            },
            TitleCandidate {
                game: GameId::new(),
                release_year: None,
            },
        ];
        assert_eq!(choose(&[], &cands, None), None);
        assert_eq!(choose(&[], &[], None), None);
    }
}
