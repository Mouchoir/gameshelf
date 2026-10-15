//! Runs the local sources the player allowed and summarizes what would be sent.

use crate::messages::tr;
use gameshelf_core::agent::{Report, SourceConfig, SourceState};
use gameshelf_local::{LocalSource, ScanOptions};
use std::{collections::BTreeMap, path::PathBuf};
use time::OffsetDateTime;

/// What one scan produced.
#[derive(Debug, Default)]
pub struct Scan {
    pub reports: Vec<Report>,
    pub states: BTreeMap<String, SourceState>,
    /// Sources that failed, with the reason.
    pub errors: Vec<(String, String)>,
}

fn options(config: Option<&SourceConfig>, send_install_paths: bool) -> ScanOptions {
    ScanOptions {
        send_install_paths,
        extra_paths: config
            .map(|c| c.paths.iter().map(PathBuf::from).collect())
            .unwrap_or_default(),
    }
}

/// Which sources exist on this computer, without reading them.
pub fn detect(
    sources: &[Box<dyn LocalSource>],
    enabled: &BTreeMap<String, SourceConfig>,
) -> BTreeMap<String, SourceState> {
    sources
        .iter()
        .map(|s| {
            let config = enabled.get(s.id());
            (
                s.id().to_string(),
                SourceState {
                    enabled: config.is_some_and(|c| c.enabled),
                    detected: s.detect(&options(config, false)),
                    last_scan_at: None,
                },
            )
        })
        .collect()
}

/// Reads every enabled source that exists on this computer.
pub async fn run(
    sources: &[Box<dyn LocalSource>],
    enabled: &BTreeMap<String, SourceConfig>,
    send_install_paths: bool,
) -> Scan {
    let mut out = Scan::default();
    for s in sources {
        let config = enabled.get(s.id());
        let opts = options(config, send_install_paths);
        let on = config.is_some_and(|c| c.enabled);
        let detected = s.detect(&opts);
        let mut state = SourceState {
            enabled: on,
            detected,
            last_scan_at: None,
        };
        if on && detected {
            match s.scan(&opts).await {
                Ok(reports) => {
                    state.last_scan_at = Some(OffsetDateTime::now_utc());
                    out.reports.extend(reports);
                }
                Err(e) => out.errors.push((s.id().to_string(), e.to_string())),
            }
        }
        out.states.insert(s.id().to_string(), state);
    }
    out
}

/// A readable summary of what a scan would send.
pub fn summary(scan: &Scan) -> String {
    let mut lines = Vec::new();
    for (id, st) in &scan.states {
        let status = match (st.enabled, st.detected) {
            (false, _) => tr("turned off", "désactivé"),
            (true, false) => tr("not found on this computer", "introuvable sur cet ordinateur"),
            (true, true) => tr("found", "trouvé"),
        };
        lines.push(format!("  {id}: {status}"));
    }
    for r in &scan.reports {
        let mut titles: Vec<&str> = r
            .accounts
            .iter()
            .flat_map(|a| &a.items)
            .chain(&r.machine_items)
            .filter_map(|i| i.title.as_deref())
            .collect();
        titles.sort_unstable();
        titles.dedup();
        let count = r.accounts.iter().map(|a| a.items.len()).sum::<usize>() + r.machine_items.len();
        let installed = r
            .accounts
            .iter()
            .flat_map(|a| &a.items)
            .chain(&r.machine_items)
            .filter(|i| i.installed)
            .count();
        let untitled = r
            .accounts
            .iter()
            .flat_map(|a| &a.items)
            .chain(&r.machine_items)
            .filter(|i| i.title.is_none())
            .count();
        lines.push(format!(
            "  {} -> {}: {count} {}, {installed} {}, {} {}",
            r.source,
            r.platform,
            tr("games", "jeux"),
            tr("installed", "installés"),
            r.accounts.len(),
            tr("account(s)", "compte(s)"),
        ));
        if untitled > 0 {
            lines.push(format!(
                "      {untitled} {}",
                tr(
                    "without a name in the launcher files (the server names them)",
                    "sans nom dans les fichiers du launcher (le serveur les nomme)",
                )
            ));
        }
        let sample: Vec<&str> = titles.iter().take(8).copied().collect();
        if !sample.is_empty() {
            let more = if titles.len() > sample.len() { ", ..." } else { "" };
            lines.push(format!("      {}{more}", sample.join(", ")));
        }
    }
    for (id, e) in &scan.errors {
        lines.push(format!("  {id}: {} {e}", tr("error:", "erreur :")));
    }
    lines.join("\n")
}
