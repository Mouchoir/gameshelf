//! Rejects fixtures that look like real personal data (Constitution, Principle I).
//!
//! Synthetic Steam ids must use the top of the 32-bit account range (`7656120225523xxxx`), far beyond any
//! account ever allocated.

use regex_lite::Regex;
use std::path::{Path, PathBuf};

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            files(&p, out);
        } else {
            out.push(p);
        }
    }
}

#[test]
fn fixtures_contain_no_real_looking_identifiers_or_secrets() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures");
    let mut all = Vec::new();
    files(&root, &mut all);
    assert!(!all.is_empty(), "no fixtures found under {}", root.display());

    let steam_id = Regex::new(r"7656119\d{10}|76561200\d{9}|76561201\d{9}").expect("regex");
    let email = Regex::new(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}").expect("regex");
    let jwt = Regex::new(r"eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.").expect("regex");
    let user_path = Regex::new(r"(?i)[A-Z]:\\Users\\[^\\]+|/home/[a-z][a-z0-9_-]*/").expect("regex");
    let allowed_emails = ["user@example.com", "player@example.com"];

    let mut problems = Vec::new();
    for path in &all {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        let name = path.strip_prefix(&root).unwrap_or(path).display().to_string();
        if let Some(m) = steam_id.find(&text) {
            problems.push(format!("{name}: plausible real SteamID64 {}", m.as_str()));
        }
        for m in email.find_iter(&text) {
            if !allowed_emails.contains(&m.as_str()) && !m.as_str().ends_with("@example.com") {
                problems.push(format!("{name}: email address {}", m.as_str()));
            }
        }
        if jwt.is_match(&text) {
            problems.push(format!("{name}: token-like value"));
        }
        if let Some(m) = user_path.find(&text) {
            problems.push(format!("{name}: user path {}", m.as_str()));
        }
    }
    assert!(
        problems.is_empty(),
        "fixture hygiene problems:\n{}",
        problems.join("\n")
    );
}
