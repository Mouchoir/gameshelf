//! `ubisoft-local`: games installed through Ubisoft Connect, from their `Uplay Install <id>` uninstall entries.
//! The id is Ubisoft's product id. Games are reported for the machine.

use crate::{
    LocalError, LocalSource, ScanOptions, finish, installed_item,
    registry::{UninstallEntry, uninstall_entries},
};
use gameshelf_core::agent::{Report, ReportItem};
use std::collections::BTreeMap;

const KEY_PREFIX: &str = "Uplay Install ";

#[derive(Clone, Debug, Default)]
pub struct Ubisoft {
    entries: Option<Vec<UninstallEntry>>,
}

impl Ubisoft {
    pub fn with_entries(entries: Vec<UninstallEntry>) -> Self {
        Self {
            entries: Some(entries),
        }
    }

    fn games(&self) -> Vec<(String, UninstallEntry)> {
        self.entries
            .clone()
            .unwrap_or_else(uninstall_entries)
            .into_iter()
            .filter_map(|e| {
                let id = e.key.strip_prefix(KEY_PREFIX)?.trim().to_string();
                id.chars().all(|c| c.is_ascii_digit()).then_some((id, e))
            })
            .collect()
    }
}

#[async_trait::async_trait]
impl LocalSource for Ubisoft {
    fn id(&self) -> &'static str {
        "ubisoft-local"
    }

    fn detect(&self, _opts: &ScanOptions) -> bool {
        !self.games().is_empty()
    }

    async fn scan(&self, opts: &ScanOptions) -> Result<Vec<Report>, LocalError> {
        let games = self.games();
        if games.is_empty() {
            return Err(LocalError::NotFound);
        }
        let mut items: BTreeMap<String, ReportItem> = BTreeMap::new();
        for (id, e) in games {
            items.entry(id.clone()).or_insert_with(|| {
                installed_item(
                    id,
                    Some(e.display_name.clone()),
                    e.size_bytes(),
                    e.location(),
                    opts,
                )
            });
        }
        Ok(vec![finish(
            "ubisoft-local",
            "ubisoft",
            Vec::new(),
            items.into_values().collect(),
        )])
    }
}
