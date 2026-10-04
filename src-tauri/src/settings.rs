//! Reads and writes a settings file the user picked in the Settings dialog.

use std::fs;
use std::path::Path;

/// Settings files are a few hundred bytes. Refuse anything large so a wrong pick cannot fill memory.
const MAX_BYTES: u64 = 1024 * 1024;

fn json_path(path: &str) -> Result<&Path, String> {
    let path = Path::new(path);
    let is_json = path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("json"));
    if is_json {
        Ok(path)
    } else {
        Err("Settings files end in .json.".into())
    }
}

#[tauri::command(async)]
pub fn settings_export(path: String, text: String) -> Result<(), String> {
    let path = json_path(&path)?;
    fs::write(path, text).map_err(|e| format!("Could not write the settings file: {e}"))
}

#[tauri::command(async)]
pub fn settings_import(path: String) -> Result<String, String> {
    let path = json_path(&path)?;
    let size = fs::metadata(path)
        .map_err(|e| format!("Could not read the settings file: {e}"))?
        .len();
    if size > MAX_BYTES {
        return Err("That file is too large to be a settings file.".into());
    }
    fs::read_to_string(path).map_err(|e| format!("Could not read the settings file: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_json_paths_are_accepted() {
        assert!(json_path("settings.json").is_ok());
        assert!(json_path("SETTINGS.JSON").is_ok());
        assert!(json_path("settings.txt").is_err());
        assert!(json_path("settings").is_err());
    }
}
