//! Installed programs as listed in the Windows "uninstall" registry keys. On other systems the list is empty.

/// One installed program.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UninstallEntry {
    /// Registry key name, often a product id (`Steam App 730`, `Uplay Install 635`, `Riot Game valorant.live`).
    pub key: String,
    pub display_name: String,
    pub publisher: Option<String>,
    pub install_location: Option<String>,
    pub uninstall_string: Option<String>,
    /// `EstimatedSize`, in KiB.
    pub size_kb: Option<u64>,
}

impl UninstallEntry {
    pub fn publisher_contains(&self, needle: &str) -> bool {
        self.publisher
            .as_deref()
            .is_some_and(|p| p.to_lowercase().contains(&needle.to_lowercase()))
    }

    pub fn size_bytes(&self) -> Option<u64> {
        self.size_kb.map(|k| k.saturating_mul(1024))
    }

    pub fn location(&self) -> Option<&std::path::Path> {
        self.install_location
            .as_deref()
            .map(|l| l.trim().trim_matches('"'))
            .filter(|l| !l.is_empty())
            .map(std::path::Path::new)
    }
}

/// Every program listed for the machine and for the current user.
pub fn uninstall_entries() -> Vec<UninstallEntry> {
    #[cfg(windows)]
    {
        windows::uninstall_entries()
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

/// A string value under `HKEY_CURRENT_USER`.
#[cfg(windows)]
pub fn current_user_string(path: &str, value: &str) -> Option<String> {
    use winreg::{RegKey, enums::HKEY_CURRENT_USER};
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(path)
        .ok()?
        .get_value::<String, _>(value)
        .ok()
        .filter(|s| !s.trim().is_empty())
}

#[cfg(windows)]
mod windows {
    use super::UninstallEntry;
    use winreg::{
        HKEY, RegKey,
        enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY},
    };

    const UNINSTALL: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall";

    pub fn uninstall_entries() -> Vec<UninstallEntry> {
        let views: [(HKEY, u32); 3] = [
            (HKEY_LOCAL_MACHINE, KEY_WOW64_64KEY),
            (HKEY_LOCAL_MACHINE, KEY_WOW64_32KEY),
            (HKEY_CURRENT_USER, 0),
        ];
        let mut out: Vec<UninstallEntry> = Vec::new();
        for (hive, view) in views {
            let Ok(root) = RegKey::predef(hive).open_subkey_with_flags(UNINSTALL, KEY_READ | view) else {
                continue;
            };
            for key in root.enum_keys().flatten() {
                let Ok(sub) = root.open_subkey_with_flags(&key, KEY_READ | view) else {
                    continue;
                };
                let text = |name: &str| {
                    sub.get_value::<String, _>(name)
                        .ok()
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                };
                let Some(display_name) = text("DisplayName") else {
                    continue;
                };
                if sub.get_value::<u32, _>("SystemComponent").unwrap_or(0) == 1 {
                    continue;
                }
                if out.iter().any(|e| e.key == key) {
                    continue;
                }
                out.push(UninstallEntry {
                    key,
                    display_name,
                    publisher: text("Publisher"),
                    install_location: text("InstallLocation"),
                    uninstall_string: text("UninstallString"),
                    size_kb: sub.get_value::<u32, _>("EstimatedSize").ok().map(u64::from),
                });
            }
        }
        out
    }
}
