//! Title normalization for matching, sorting and search (research D7).

use deunicode::deunicode;

/// Leading articles ignored when sorting and searching (English and French).
const ARTICLES: &[&str] = &["the ", "a ", "an ", "le ", "la ", "les ", "l'"];

/// Trademark-like symbols that platforms add inconsistently.
const SYMBOLS: &[char] = &['\u{2122}', '\u{00AE}', '\u{00A9}'];

/// Returns the key used for search and title matching: ASCII, lowercase, punctuation replaced by spaces,
/// whitespace collapsed, leading article removed.
///
/// ```
/// use gameshelf_core::normalize::search_key;
/// assert_eq!(search_key("The Witcher\u{00AE} 3: Wild Hunt"), "witcher 3 wild hunt");
/// assert_eq!(search_key("Pokémon"), "pokemon");
/// ```
pub fn search_key(title: &str) -> String {
    let cleaned: String = title.chars().filter(|c| !SYMBOLS.contains(c)).collect();
    let ascii = deunicode(&cleaned).to_lowercase();
    let mut out = String::with_capacity(ascii.len());
    let mut last_space = true;
    for c in ascii.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
            last_space = false;
        } else if c == '\'' {
            // Keep apostrophes out of words: "Assassin's" -> "assassins".
            continue;
        } else if !last_space {
            out.push(' ');
            last_space = true;
        }
    }
    let trimmed = out.trim_end().to_string();
    strip_article(&trimmed).to_string()
}

/// Returns the title used for alphabetical sorting: original casing and accents kept, leading article
/// removed, symbols dropped.
///
/// ```
/// use gameshelf_core::normalize::sort_title;
/// assert_eq!(sort_title("The Last of Us"), "Last of Us");
/// assert_eq!(sort_title("L'Amerzone"), "Amerzone");
/// ```
pub fn sort_title(title: &str) -> String {
    let cleaned: String = title.chars().filter(|c| !SYMBOLS.contains(c)).collect();
    let trimmed = cleaned.trim();
    let lower = trimmed.to_lowercase();
    if lower.len() != trimmed.len() {
        return trimmed.to_string();
    }
    for article in ARTICLES {
        if lower.starts_with(article) && lower.len() > article.len() {
            let rest = &trimmed[article.len()..];
            return rest.trim_start().to_string();
        }
    }
    trimmed.to_string()
}

fn strip_article(key: &str) -> &str {
    for article in ARTICLES {
        let article = article.trim_end_matches('\'').trim_end();
        if let Some(rest) = key.strip_prefix(article)
            && let Some(rest) = rest.strip_prefix(' ')
            && !rest.is_empty()
        {
            return rest;
        }
    }
    key
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_key_handles_accents_case_and_punctuation() {
        assert_eq!(
            search_key("  Assassin's Creed® Valhalla  "),
            "assassins creed valhalla"
        );
        assert_eq!(search_key("Counter-Strike 2"), "counter strike 2");
        assert_eq!(search_key("Éternal Röad"), "eternal road");
        assert_eq!(
            search_key("The Elder Scrolls V: Skyrim"),
            "elder scrolls v skyrim"
        );
    }

    #[test]
    fn search_key_keeps_titles_that_are_only_an_article() {
        assert_eq!(search_key("The"), "the");
        assert_eq!(search_key("A"), "a");
    }

    #[test]
    fn french_articles_are_stripped() {
        assert_eq!(search_key("Les Sims 4"), "sims 4");
        assert_eq!(search_key("La Mulana"), "mulana");
        assert_eq!(sort_title("Les Sims 4"), "Sims 4");
    }

    #[test]
    fn sort_title_keeps_original_casing() {
        assert_eq!(sort_title("A Plague Tale: Innocence"), "Plague Tale: Innocence");
        assert_eq!(sort_title("Hades"), "Hades");
        assert_eq!(sort_title("ABZU"), "ABZU");
    }
}
