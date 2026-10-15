//! In-memory title search (research D7, FR-012, SC-002).
//!
//! The index holds every game of the instance. Results are candidate game ids; the caller then reads them
//! through the store's visibility choke point, so the index itself never decides what a viewer may see.

use crate::{GameId, normalize::search_key};
use nucleo_matcher::{
    Config, Matcher,
    pattern::{CaseMatching, Normalization, Pattern},
};

#[derive(Clone, Debug)]
struct Entry {
    game: GameId,
    key: String,
}

impl AsRef<str> for Entry {
    fn as_ref(&self) -> &str {
        &self.key
    }
}

/// A search index over normalized titles.
#[derive(Clone, Debug, Default)]
pub struct SearchIndex {
    entries: Vec<Entry>,
}

impl SearchIndex {
    /// Builds the index from `(game, normalized title)` pairs.
    pub fn build(items: impl IntoIterator<Item = (GameId, String)>) -> Self {
        let mut entries: Vec<Entry> = items.into_iter().map(|(game, key)| Entry { game, key }).collect();
        entries.sort_by(|a, b| a.key.cmp(&b.key));
        entries.dedup_by(|a, b| a.game == b.game);
        Self { entries }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Returns up to `limit` games matching `query`, best first. Partial words, accents, case and punctuation
    /// are ignored; when nothing matches, single-letter typos are tolerated.
    pub fn search(&self, query: &str, limit: usize) -> Vec<GameId> {
        let q = search_key(query);
        if q.is_empty() || limit == 0 {
            return Vec::new();
        }

        // Exact substring matches first, so "portal" ranks "Portal" above fuzzy hits.
        let mut out: Vec<GameId> = Vec::new();
        let mut substring: Vec<&Entry> = self.entries.iter().filter(|e| e.key.contains(&q)).collect();
        substring.sort_by_key(|e| (!e.key.starts_with(&q), e.key.len()));
        out.extend(substring.iter().take(limit).map(|e| e.game));
        if !out.is_empty() {
            return out;
        }

        let mut matcher = Matcher::new(Config::DEFAULT);
        let pattern = Pattern::parse(&q, CaseMatching::Ignore, Normalization::Smart);
        for (entry, _score) in pattern.match_list(self.entries.iter(), &mut matcher) {
            if out.len() >= limit {
                break;
            }
            if !out.contains(&entry.game) {
                out.push(entry.game);
            }
        }
        if !out.is_empty() {
            return out;
        }

        // Typo fallback: every query word must be within one edit of some title word.
        let words: Vec<&str> = q.split(' ').filter(|w| w.len() >= 3).collect();
        if words.is_empty() {
            return out;
        }
        for entry in &self.entries {
            if out.len() >= limit {
                break;
            }
            let title_words: Vec<&str> = entry.key.split(' ').collect();
            if words.iter().all(|w| {
                title_words
                    .iter()
                    .any(|t| within_one_edit(w, t) || t.starts_with(w))
            }) {
                out.push(entry.game);
            }
        }
        out
    }
}

/// True if `a` and `b` differ by at most one substitution, insertion, deletion or adjacent transposition.
fn within_one_edit(a: &str, b: &str) -> bool {
    let a = a.as_bytes();
    let b = b.as_bytes();
    if a == b {
        return true;
    }
    let (la, lb) = (a.len(), b.len());
    if la.abs_diff(lb) > 1 {
        return false;
    }
    let mut i = 0;
    while i < la.min(lb) && a[i] == b[i] {
        i += 1;
    }
    if la == lb {
        // substitution, or transposition of i and i + 1
        a[i + 1..] == b[i + 1..]
            || (i + 1 < la && a[i] == b[i + 1] && a[i + 1] == b[i] && a[i + 2..] == b[i + 2..])
    } else if la > lb {
        a[i + 1..] == b[i..]
    } else {
        a[i..] == b[i + 1..]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index(titles: &[&str]) -> (SearchIndex, Vec<GameId>) {
        let ids: Vec<GameId> = titles.iter().map(|_| GameId::new()).collect();
        let idx = SearchIndex::build(ids.iter().zip(titles).map(|(id, t)| (*id, search_key(t))));
        (idx, ids)
    }

    #[test]
    fn partial_case_and_accent_insensitive() {
        let (idx, ids) = index(&["Pokémon Legends: Arceus", "Portal 2", "Hades"]);
        assert_eq!(idx.search("POKEMON", 10), vec![ids[0]]);
        assert_eq!(idx.search("legends arc", 10), vec![ids[0]]);
        assert_eq!(idx.search("portal", 10), vec![ids[1]]);
    }

    #[test]
    fn prefix_matches_rank_first() {
        let (idx, ids) = index(&["Super Portal Bros", "Portal 2"]);
        assert_eq!(idx.search("portal", 10)[0], ids[1]);
    }

    #[test]
    fn small_typos_are_tolerated() {
        let (idx, ids) = index(&["The Witcher 3: Wild Hunt", "Balatro"]);
        assert_eq!(idx.search("wticher", 10), vec![ids[0]]);
        assert_eq!(idx.search("blaatro", 10), vec![ids[1]]);
        assert_eq!(idx.search("witcer", 10), vec![ids[0]]);
    }

    #[test]
    fn no_match_is_empty() {
        let (idx, _) = index(&["Hades", "Balatro"]);
        assert!(idx.search("zzzzzz", 10).is_empty());
        assert!(idx.search("   ", 10).is_empty());
    }

    #[test]
    fn edit_distance_helper() {
        assert!(within_one_edit("witcher", "witcher"));
        assert!(within_one_edit("witcer", "witcher"));
        assert!(within_one_edit("wticher", "witcher"));
        assert!(within_one_edit("witchar", "witcher"));
        assert!(!within_one_edit("wicer", "witcher"));
    }

    #[test]
    fn twenty_thousand_titles_search_quickly() {
        let titles: Vec<String> = (0..20_000)
            .map(|i| format!("Generated Title {i} of the Collection"))
            .collect();
        let idx = SearchIndex::build(titles.iter().map(|t| (GameId::new(), search_key(t))));
        let start = std::time::Instant::now();
        let hits = idx.search("title 1999", 50);
        assert!(!hits.is_empty());
        let budget = if cfg!(debug_assertions) { 1000 } else { 200 };
        assert!(
            start.elapsed() < std::time::Duration::from_millis(budget),
            "{:?}",
            start.elapsed()
        );
        // Fuzzy path (no substring hit) stays within budget too.
        let start = std::time::Instant::now();
        let _ = idx.search("gnrtd ttl", 50);
        assert!(
            start.elapsed() < std::time::Duration::from_millis(budget * 2),
            "{:?}",
            start.elapsed()
        );
    }
}
