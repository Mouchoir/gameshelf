//! The agent speaks English or French, following the system language. English is the reference text.

use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    En,
    Fr,
}

fn detect() -> Lang {
    let from_env = ["LC_ALL", "LC_MESSAGES", "LANG", "LANGUAGE"]
        .iter()
        .filter_map(|k| std::env::var(k).ok())
        .find(|v| !v.is_empty() && v != "C" && v != "POSIX");
    #[cfg(windows)]
    let from_env = from_env.or_else(|| {
        winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER)
            .open_subkey(r"Control Panel\International")
            .ok()?
            .get_value::<String, _>("LocaleName")
            .ok()
    });
    match from_env {
        Some(v) if v.to_lowercase().starts_with("fr") => Lang::Fr,
        _ => Lang::En,
    }
}

pub fn lang() -> Lang {
    static LANG: OnceLock<Lang> = OnceLock::new();
    *LANG.get_or_init(detect)
}

/// The text in the system language.
pub fn tr(en: &'static str, fr: &'static str) -> &'static str {
    match lang() {
        Lang::En => en,
        Lang::Fr => fr,
    }
}
