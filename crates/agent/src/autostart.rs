//! Starting the agent with the computer, only when the player asks for it (off by default).
//!
//! Windows: a value under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`.
//! Linux: a systemd user service, `~/.config/systemd/user/gameshelf-agent.service`.

#[cfg(windows)]
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const NAME: &str = "gameshelf-agent";

fn exe() -> Result<std::path::PathBuf, String> {
    std::env::current_exe().map_err(|e| e.to_string())
}

#[cfg(windows)]
pub fn enable() -> Result<(), String> {
    use winreg::{RegKey, enums::HKEY_CURRENT_USER};
    let (key, _) = RegKey::predef(HKEY_CURRENT_USER)
        .create_subkey(RUN_KEY)
        .map_err(|e| e.to_string())?;
    let command = format!("\"{}\" run", exe()?.display());
    key.set_value(NAME, &command).map_err(|e| e.to_string())
}

#[cfg(windows)]
pub fn disable() -> Result<(), String> {
    use winreg::{
        RegKey,
        enums::{HKEY_CURRENT_USER, KEY_SET_VALUE},
    };
    let key = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags(RUN_KEY, KEY_SET_VALUE)
        .map_err(|e| e.to_string())?;
    match key.delete_value(NAME) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(windows)]
pub fn is_enabled() -> bool {
    use winreg::{RegKey, enums::HKEY_CURRENT_USER};
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(RUN_KEY)
        .and_then(|k| k.get_value::<String, _>(NAME))
        .is_ok()
}

#[cfg(not(windows))]
fn unit_path() -> Result<std::path::PathBuf, String> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|d| !d.is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))
        .ok_or("no home folder")?;
    Ok(base.join("systemd").join("user").join(format!("{NAME}.service")))
}

#[cfg(not(windows))]
fn systemctl(args: &[&str]) -> Result<(), String> {
    let status = std::process::Command::new("systemctl")
        .arg("--user")
        .args(args)
        .status()
        .map_err(|e| format!("systemctl: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("systemctl --user {} failed", args.join(" ")))
    }
}

#[cfg(not(windows))]
pub fn enable() -> Result<(), String> {
    let path = unit_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let unit = format!(
        "[Unit]\nDescription=gameshelf agent\nAfter=network-online.target\n\n[Service]\nExecStart=\"{}\" run\n\
         Restart=on-failure\nRestartSec=60\n\n[Install]\nWantedBy=default.target\n",
        exe()?.display()
    );
    std::fs::write(&path, unit).map_err(|e| e.to_string())?;
    systemctl(&["daemon-reload"])?;
    systemctl(&["enable", "--now", &format!("{NAME}.service")])
}

#[cfg(not(windows))]
pub fn disable() -> Result<(), String> {
    let path = unit_path()?;
    if path.exists() {
        // The service may already be stopped; removing the unit is what matters.
        let _ = systemctl(&["disable", "--now", &format!("{NAME}.service")]);
        std::fs::remove_file(&path).map_err(|e| e.to_string())?;
        let _ = systemctl(&["daemon-reload"]);
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn is_enabled() -> bool {
    unit_path().is_ok_and(|p| p.exists())
}
