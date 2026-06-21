//! Persisted overview prefs — mirrors web `localStorage` keys (`fabricHidden`, `fabricApp.kind`).

use fabric_health::KindFilter;
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

const HIDDEN_FILE: &str = "fabricHidden.json";
const KIND_FILE: &str = "fabricApp.kind.json";

pub(crate) fn config_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config/fabric"))
}

fn read_json_array(path: &PathBuf) -> HashSet<String> {
    let Ok(raw) = fs::read_to_string(path) else {
        return HashSet::new();
    };
    serde_json::from_str::<Vec<String>>(&raw)
        .map(|v| v.into_iter().collect())
        .unwrap_or_default()
}

fn write_json_array(path: &PathBuf, values: &HashSet<String>) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let mut list: Vec<_> = values.iter().cloned().collect();
    list.sort();
    if let Ok(json) = serde_json::to_string_pretty(&list) {
        let _ = fs::write(path, json);
    }
}

pub fn load_hidden() -> HashSet<String> {
    config_dir()
        .map(|d| d.join(HIDDEN_FILE))
        .map(|p| read_json_array(&p))
        .unwrap_or_default()
}

pub fn save_hidden(hidden: &HashSet<String>) {
    if let Some(dir) = config_dir() {
        write_json_array(&dir.join(HIDDEN_FILE), hidden);
    }
}

pub fn load_kind() -> KindFilter {
    let Some(dir) = config_dir() else {
        return KindFilter::default();
    };
    let path = dir.join(KIND_FILE);
    let Ok(raw) = fs::read_to_string(path) else {
        return KindFilter::default();
    };
    match raw.trim().trim_matches('"') {
        "lm" => KindFilter::Lm,
        "video" | "recon" => KindFilter::Recon,
        _ => KindFilter::All,
    }
}

pub fn save_kind(kind: KindFilter) {
    let Some(dir) = config_dir() else {
        return;
    };
    let value = match kind {
        KindFilter::All => "all",
        KindFilter::Recon => "video",
        KindFilter::Lm => "lm",
    };
    let _ = fs::write(dir.join(KIND_FILE), format!("\"{value}\""));
}
