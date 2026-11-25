//! Workspace persistence: a single JSON file in the user's data directory.

use std::path::PathBuf;

use crate::model::Workspace;

/// `RUSTMAN_DATA_DIR` overrides the location (portable mode / testing).
pub fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("RUSTMAN_DATA_DIR") {
        return PathBuf::from(dir);
    }
    dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("rustman")
}

pub fn workspace_path() -> PathBuf {
    data_dir().join("workspace.json")
}

pub fn load() -> Workspace {
    let path = workspace_path();
    match std::fs::read_to_string(&path) {
        Ok(text) => match serde_json::from_str(&text) {
            Ok(ws) => ws,
            Err(e) => {
                // Keep the broken file around instead of silently overwriting it.
                let backup = path.with_extension(format!("broken-{}.json", chrono::Local::now().format("%Y%m%d%H%M%S")));
                let _ = std::fs::copy(&path, &backup);
                eprintln!("Cannot parse {}: {e}. Backup saved to {}", path.display(), backup.display());
                Workspace::default()
            }
        },
        Err(_) => Workspace::default(),
    }
}

/// Atomic write: write to a temp file and rename it over the old one.
pub fn save(ws: &Workspace) -> Result<(), String> {
    let path = workspace_path();
    std::fs::create_dir_all(data_dir()).map_err(|e| e.to_string())?;
    let json = serde_json::to_vec_pretty(ws).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
}
