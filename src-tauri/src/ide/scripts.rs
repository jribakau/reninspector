//! Search, file operations, symbols, assets, translations, logs and project lifecycle.

use crate::error::AppError;
use std::fs;

use renpy_core::catalog;
use tauri::{AppHandle, State};

use crate::commands::{self, AppState};
use crate::edit::{self, EditImpact};

use super::*;

#[tauri::command(async)]
pub fn create_script(
    state: State<'_, AppState>,
    path: String,
    text: Option<String>,
) -> Result<(), AppError> {
    let rel = script_rel(&path)?;
    crate::util::with_project_mut(&state, |project| {
        commands::ensure_game_closed(project)?;
        if project.file_index(&rel).is_some() {
            return Err(format!("`{rel}` already exists.").into());
        }
        let abs = abs_under(&project.game_dir, &rel);
        if abs.exists() {
            return Err(format!("`{rel}` already exists on disk.").into());
        }
        if let Some(parent) = abs.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("Could not create the folder: {e}"))?;
        }
        let stem = rel
            .rsplit('/')
            .next()
            .unwrap_or("script")
            .trim_end_matches(".rpym")
            .trim_end_matches(".rpy");
        let body = text.unwrap_or_else(|| format!("label {stem}:\n    return\n"));
        fs::write(&abs, body).map_err(|e| format!("Could not write {rel}: {e}"))?;
        project.refresh_paths(std::slice::from_ref(&abs));
        Ok(())
    })
}

#[tauri::command(async)]
pub fn rename_script(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    new_path: String,
) -> Result<(), AppError> {
    let from = script_rel(&path)?;
    let to = script_rel(&new_path)?;
    crate::util::with_project_mut(&state, |project| {
        commands::ensure_game_closed(project)?;
        let idx = project
            .file_index(&from)
            .ok_or_else(|| format!("`{from}` is not a script of this project."))?;
        if !project.files[idx].abs.is_file() {
            return Err(
                "Only a loose script can be renamed. An archived script has to be saved first."
                    .into(),
            );
        }
        if project.file_index(&to).is_some() {
            return Err(format!("`{to}` already exists.").into());
        }
        let src = project.files[idx].abs.clone();
        let dest = abs_under(&project.game_dir, &to);
        let root = commands::backup_dir(&app, &project.root)?;
        let backup = root.join(from.replace('/', std::path::MAIN_SEPARATOR_STR));
        if !backup.is_file() {
            if let Some(parent) = backup.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&src, &backup).map_err(|e| format!("Could not back up {from}: {e}"))?;
        }
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::rename(&src, &dest).map_err(|e| format!("Could not rename {from}: {e}"))?;
        project.refresh_paths(&[src, dest]);
        Ok(())
    })
}

#[tauri::command(async)]
pub fn delete_script(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<(), AppError> {
    let rel = script_rel(&path)?;
    crate::util::with_project_mut(&state, |project| {
        commands::ensure_game_closed(project)?;
        let idx = project
            .file_index(&rel)
            .ok_or_else(|| format!("`{rel}` is not a script of this project."))?;
        let abs = project.files[idx].abs.clone();
        if !abs.is_file() {
            return Err("This script is not a loose file, so there is nothing to delete.".into());
        }
        let root = commands::backup_dir(&app, &project.root)?;
        let backup = root.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
        if !backup.is_file() {
            if let Some(parent) = backup.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&abs, &backup).map_err(|e| format!("Could not back up {rel}: {e}"))?;
        }
        fs::remove_file(&abs).map_err(|e| format!("Could not delete {rel}: {e}"))?;
        project.refresh_paths(std::slice::from_ref(&abs));
        Ok(())
    })
}

#[tauri::command(async)]
pub fn update_translation(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    line: u32,
    text: String,
) -> Result<EditImpact, AppError> {
    if line == 0 {
        return Err("That translation is not tied to a line.".into());
    }
    crate::util::with_project_mut(&state, |project| {
        commands::ensure_game_closed(project)?;
        let mut edit = crate::util::lock(&state.edit);
        let root = commands::backup_dir(&app, &project.root)?;
        let bytes = project.read_script_bytes(&path)?;
        let original = String::from_utf8(bytes)
            .map_err(|_| format!("`{path}` is not UTF-8. Edit it in code."))?;
        let mut lines: Vec<String> = original
            .split('\n')
            .map(|l| l.trim_end_matches('\r').to_string())
            .collect();
        let idx = (line as usize)
            .checked_sub(1)
            .ok_or("Line 0 is not a script line.")?;
        let row = lines
            .get_mut(idx)
            .ok_or_else(|| format!("{path} has no line {line}."))?;
        *row = catalog::replace_quoted(row, &text);
        let mut next = lines.join("\n");
        if original.ends_with('\n') && !next.ends_with('\n') {
            next.push('\n');
        }
        edit::apply_write(project, &mut edit, &root, &path, &next)
    })
}
