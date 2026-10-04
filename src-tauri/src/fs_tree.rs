//! File operations behind the explorer tree: create, rename, move, delete and reveal.
//!
//! Paths are relative to the project folder and use `/`. Every operation stays inside
//! the project, refuses `game/` itself, and tells the script index what it touched.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use renpy_core::Project;
use tauri::{AppHandle, State};

use crate::commands::{self, AppState};
use crate::ide::hidden_listing;

/// Upper bound on files reported to the project index for one operation.
const MAX_TOUCHED: usize = 20_000;

fn no_project() -> String {
    "No project is open.".into()
}

const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// One path segment of a new or renamed entry.
pub(crate) fn check_name(name: &str, dir: bool) -> Result<(), String> {
    if name.is_empty() {
        return Err("Type a name.".into());
    }
    if name != name.trim() {
        return Err("A name cannot start or end with a space.".into());
    }
    if name.len() > 255 {
        return Err("That name is too long.".into());
    }
    if name.chars().any(|c| c.is_control() || "<>:\"/\\|?*".contains(c)) {
        return Err("A name cannot contain < > : \" / \\ | ? * .".into());
    }
    if name.ends_with('.') {
        return Err("A name cannot end with a dot.".into());
    }
    if name.starts_with('.') {
        return Err("Names starting with a dot are hidden in the explorer.".into());
    }
    let stem = name.split('.').next().unwrap_or(name).to_ascii_uppercase();
    if RESERVED.contains(&stem.as_str()) {
        return Err(format!("`{name}` is a reserved name on Windows."));
    }
    if hidden_listing(name, dir) {
        return Err(format!("`{name}` is reserved by the IDE and hidden in the explorer."));
    }
    Ok(())
}

/// An existing path inside the project: no `..`, drive letters or empty parts.
pub(crate) fn clean_rel(rel: &str) -> Result<String, String> {
    let rel = rel.trim().replace('\\', "/");
    let rel = rel.trim_matches('/');
    if rel.is_empty() {
        return Err("That path is the project folder.".into());
    }
    if rel.contains(':') || rel.split('/').any(|p| p.is_empty() || p == "." || p == "..") {
        return Err("That path is not inside the project.".into());
    }
    Ok(rel.to_string())
}

/// A path that may not exist yet: every segment must be a valid name.
fn clean_new(rel: &str, last_is_dir: bool) -> Result<String, String> {
    let rel = clean_rel(rel)?;
    let parts: Vec<&str> = rel.split('/').collect();
    for (i, part) in parts.iter().enumerate() {
        check_name(part, i + 1 < parts.len() || last_is_dir)?;
    }
    Ok(rel)
}

fn abs_of(root: &Path, rel: &str) -> PathBuf {
    if rel.is_empty() {
        return root.to_path_buf();
    }
    root.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR))
}

/// Resolve links on the nearest existing ancestor so a symlink cannot lead out of the project.
fn ensure_inside(root: &Path, abs: &Path) -> Result<(), String> {
    let mut probe = abs;
    while !probe.exists() {
        probe = probe.parent().ok_or("That path is not inside the project.")?;
    }
    let real = probe.canonicalize().map_err(|e| e.to_string())?;
    let base = root.canonicalize().map_err(|e| e.to_string())?;
    if real.starts_with(&base) {
        Ok(())
    } else {
        Err("That path is not inside the project.".into())
    }
}

/// True when `name` is already a child of `parent`, ignoring case. `except` is the entry being renamed.
fn name_taken(parent: &Path, name: &str, except: Option<&Path>) -> bool {
    let Ok(entries) = fs::read_dir(parent) else {
        return false;
    };
    entries.flatten().any(|e| {
        if e.file_name().to_string_lossy().to_lowercase() != name.to_lowercase() {
            return false;
        }
        match except {
            Some(skip) => e.path() != skip,
            None => true,
        }
    })
}

fn files_under(path: &Path, out: &mut Vec<PathBuf>) {
    if out.len() >= MAX_TOUCHED {
        return;
    }
    let Ok(meta) = fs::symlink_metadata(path) else {
        return;
    };
    if meta.is_symlink() {
        return;
    }
    if meta.is_file() {
        out.push(path.to_path_buf());
        return;
    }
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        files_under(&entry.path(), out);
    }
}

/// Paths under `old` as they will be under `new` once it has moved.
fn remapped(old_files: &[PathBuf], old: &Path, new: &Path) -> Vec<PathBuf> {
    old_files
        .iter()
        .filter_map(|p| p.strip_prefix(old).ok().map(|rest| new.join(rest)))
        .collect()
}

fn copy_tree(src: &Path, dst: &Path) -> Result<(), String> {
    let meta = fs::symlink_metadata(src).map_err(|e| e.to_string())?;
    if meta.is_dir() {
        fs::create_dir_all(dst).map_err(|e| e.to_string())?;
        for entry in fs::read_dir(src).map_err(|e| e.to_string())?.flatten() {
            copy_tree(&entry.path(), &dst.join(entry.file_name()))?;
        }
    } else if meta.is_file() {
        fs::copy(src, dst).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Rename, falling back to copy-and-remove when the target is on another volume.
fn relocate(src: &Path, dst: &Path) -> Result<(), String> {
    if fs::rename(src, dst).is_ok() {
        return Ok(());
    }
    if let Err(e) = copy_tree(src, dst) {
        let _ = if dst.is_dir() { fs::remove_dir_all(dst) } else { fs::remove_file(dst) };
        return Err(format!("Could not move {}: {e}", src.display()));
    }
    if src.is_dir() {
        fs::remove_dir_all(src)
    } else {
        fs::remove_file(src)
    }
    .map_err(|e| format!("Could not remove {}: {e}", src.display()))
}

fn require_loose(abs: &Path, rel: &str) -> Result<(), String> {
    if abs.exists() {
        Ok(())
    } else {
        Err(format!(
            "`{rel}` is not a loose file. Archived entries cannot be changed here; save a copy first."
        ))
    }
}

pub(crate) fn do_create(root: &Path, rel: &str, dir: bool) -> Result<Vec<PathBuf>, String> {
    let rel = clean_new(rel, dir)?;
    let abs = abs_of(root, &rel);
    ensure_inside(root, &abs)?;
    let name = rel.rsplit('/').next().unwrap_or(&rel);
    if let Some(parent) = abs.parent() {
        if name_taken(parent, name, None) {
            return Err(format!("`{name}` already exists here."));
        }
        fs::create_dir_all(parent).map_err(|e| format!("Could not create the folder: {e}"))?;
    }
    if dir {
        fs::create_dir(&abs).map_err(|e| format!("Could not create {rel}: {e}"))?;
        return Ok(Vec::new());
    }
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&abs)
        .map_err(|e| format!("Could not create {rel}: {e}"))?;
    Ok(vec![abs])
}

pub(crate) fn do_rename(root: &Path, from: &str, to: &str) -> Result<Vec<PathBuf>, String> {
    let from = clean_rel(from)?;
    let src = abs_of(root, &from);
    require_loose(&src, &from)?;
    ensure_inside(root, &src)?;
    let is_dir = src.is_dir();
    let to = clean_new(to, is_dir)?;
    if to == from {
        return Ok(Vec::new());
    }
    if is_dir && to.to_lowercase().starts_with(&format!("{}/", from.to_lowercase())) {
        return Err("A folder cannot be moved into itself.".into());
    }
    let dst = abs_of(root, &to);
    ensure_inside(root, &dst)?;
    let parent = dst.parent().ok_or("That path is not inside the project.")?;
    if !parent.is_dir() {
        return Err("The destination folder does not exist.".into());
    }
    let name = to.rsplit('/').next().unwrap_or(&to);
    if name_taken(parent, name, Some(&src)) {
        return Err(format!("`{name}` already exists there."));
    }
    let mut old_files = Vec::new();
    files_under(&src, &mut old_files);
    fs::rename(&src, &dst).map_err(|e| format!("Could not rename {from}: {e}"))?;
    let mut touched = remapped(&old_files, &src, &dst);
    touched.extend(old_files);
    Ok(touched)
}

/// Move entries into `dest_dir` (empty for the project folder). Returns touched paths and the new locations.
pub(crate) fn do_move(
    root: &Path,
    sources: &[String],
    dest_dir: &str,
) -> Result<(Vec<PathBuf>, Vec<String>), String> {
    let dest_rel = if dest_dir.trim().trim_matches('/').is_empty() {
        String::new()
    } else {
        clean_rel(dest_dir)?
    };
    let dest_abs = abs_of(root, &dest_rel);
    ensure_inside(root, &dest_abs)?;
    if !dest_abs.is_dir() {
        return Err("The destination is not a folder.".into());
    }
    let mut plan: Vec<(String, PathBuf, PathBuf, String)> = Vec::new();
    let mut names = HashSet::new();
    for source in sources {
        let from = clean_rel(source)?;
        let src = abs_of(root, &from);
        require_loose(&src, &from)?;
        ensure_inside(root, &src)?;
        let name = from.rsplit('/').next().unwrap_or(&from).to_string();
        let to = if dest_rel.is_empty() { name.clone() } else { format!("{dest_rel}/{name}") };
        if to.eq_ignore_ascii_case(&from) {
            continue;
        }
        if src.is_dir()
            && (dest_rel.eq_ignore_ascii_case(&from)
                || dest_rel.to_lowercase().starts_with(&format!("{}/", from.to_lowercase())))
        {
            return Err("A folder cannot be moved into itself.".into());
        }
        if !names.insert(name.to_lowercase()) {
            return Err(format!("More than one `{name}` is being moved to the same place."));
        }
        if name_taken(&dest_abs, &name, None) {
            return Err(format!("`{name}` already exists in the destination."));
        }
        plan.push((from, src, dest_abs.join(&name), to));
    }
    let mut touched = Vec::new();
    let mut moved = Vec::new();
    for (from, src, dst, to) in plan {
        let mut old_files = Vec::new();
        files_under(&src, &mut old_files);
        fs::rename(&src, &dst).map_err(|e| format!("Could not move {from}: {e}"))?;
        touched.extend(remapped(&old_files, &src, &dst));
        touched.extend(old_files);
        moved.push(to);
    }
    Ok((touched, moved))
}

/// Move the entry into the IDE backup folder, so a delete can be undone by hand.
pub(crate) fn do_delete(root: &Path, rel: &str, backup: &Path) -> Result<Vec<PathBuf>, String> {
    let rel = clean_rel(rel)?;
    let src = abs_of(root, &rel);
    require_loose(&src, &rel)?;
    ensure_inside(root, &src)?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let dst = backup
        .join("deleted")
        .join(stamp.to_string())
        .join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Could not make a backup: {e}"))?;
    }
    let mut old_files = Vec::new();
    files_under(&src, &mut old_files);
    relocate(&src, &dst)?;
    Ok(old_files)
}

fn game_rel(project: &Project) -> String {
    project
        .game_dir
        .strip_prefix(&project.root)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default()
}

fn guard_game_folder(project: &Project, rel: &str) -> Result<(), String> {
    let game = game_rel(project);
    let rel = rel.trim().replace('\\', "/");
    let rel = rel.trim_matches('/');
    if !game.is_empty() && rel.eq_ignore_ascii_case(&game) {
        return Err("The game folder cannot be renamed, moved or deleted.".into());
    }
    Ok(())
}

#[tauri::command(async)]
pub fn fs_create(state: State<'_, AppState>, path: String, dir: bool) -> Result<String, String> {
    let rel = clean_new(&path, dir)?;
    let mut guard = state.project.lock().map_err(|e| e.to_string())?;
    let project = guard.as_mut().ok_or_else(no_project)?;
    commands::ensure_game_closed(project)?;
    let touched = do_create(&project.root, &rel, dir)?;
    if !touched.is_empty() {
        project.refresh_paths(&touched);
    }
    Ok(rel)
}

#[tauri::command(async)]
pub fn fs_rename(state: State<'_, AppState>, path: String, new_path: String) -> Result<(), String> {
    let mut guard = state.project.lock().map_err(|e| e.to_string())?;
    let project = guard.as_mut().ok_or_else(no_project)?;
    commands::ensure_game_closed(project)?;
    guard_game_folder(project, &path)?;
    let touched = do_rename(&project.root, &path, &new_path)?;
    if !touched.is_empty() {
        project.refresh_paths(&touched);
    }
    Ok(())
}

#[tauri::command(async)]
pub fn fs_move(
    state: State<'_, AppState>,
    paths: Vec<String>,
    dest_dir: String,
) -> Result<Vec<String>, String> {
    let mut guard = state.project.lock().map_err(|e| e.to_string())?;
    let project = guard.as_mut().ok_or_else(no_project)?;
    commands::ensure_game_closed(project)?;
    for path in &paths {
        guard_game_folder(project, path)?;
    }
    let (touched, moved) = do_move(&project.root, &paths, &dest_dir)?;
    if !touched.is_empty() {
        project.refresh_paths(&touched);
    }
    Ok(moved)
}

#[tauri::command(async)]
pub fn fs_delete(app: AppHandle, state: State<'_, AppState>, path: String) -> Result<(), String> {
    let mut guard = state.project.lock().map_err(|e| e.to_string())?;
    let project = guard.as_mut().ok_or_else(no_project)?;
    commands::ensure_game_closed(project)?;
    guard_game_folder(project, &path)?;
    let backup = commands::backup_dir(&app, &project.root)?;
    let touched = do_delete(&project.root, &path, &backup)?;
    if !touched.is_empty() {
        project.refresh_paths(&touched);
    }
    Ok(())
}

/// Show the entry in the OS file manager, selected when the platform supports it.
#[tauri::command(async)]
pub fn fs_reveal(state: State<'_, AppState>, path: String) -> Result<(), String> {
    let root = {
        let guard = state.project.lock().map_err(|e| e.to_string())?;
        guard.as_ref().ok_or_else(no_project)?.root.clone()
    };
    let rel = if path.trim().trim_matches('/').is_empty() { String::new() } else { clean_rel(&path)? };
    let abs = abs_of(&root, &rel);
    ensure_inside(&root, &abs)?;
    if !abs.exists() {
        return Err(format!("`{rel}` is not a loose file, so there is nothing to show."));
    }
    let mut cmd;
    if cfg!(windows) {
        cmd = std::process::Command::new("explorer");
        let native = abs.to_string_lossy().replace('/', "\\");
        if abs.is_dir() {
            cmd.arg(native);
        } else {
            cmd.arg(format!("/select,{native}"));
        }
    } else if cfg!(target_os = "macos") {
        cmd = std::process::Command::new("open");
        if abs.is_dir() {
            cmd.arg(&abs);
        } else {
            cmd.arg("-R").arg(&abs);
        }
    } else {
        cmd = std::process::Command::new("xdg-open");
        cmd.arg(if abs.is_dir() { abs.as_path() } else { abs.parent().unwrap_or(&root) });
    }
    cmd.spawn().map_err(|e| format!("Could not open the file manager: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "vnide-fs-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(dir.join("game")).unwrap();
        dir
    }

    #[test]
    fn names_are_checked() {
        assert!(check_name("script.rpy", false).is_ok());
        assert!(check_name("", false).is_err());
        assert!(check_name(" a", false).is_err());
        assert!(check_name("a.", false).is_err());
        assert!(check_name(".hidden", false).is_err());
        assert!(check_name("a:b", false).is_err());
        assert!(check_name("con.txt", false).is_err());
        assert!(check_name("vnide_live.rpy", false).is_err());
        assert!(check_name("saves", true).is_err());
    }

    #[test]
    fn paths_must_stay_inside() {
        assert!(clean_rel("game/a.rpy").is_ok());
        assert_eq!(clean_rel("\\game\\a.rpy\\").unwrap(), "game/a.rpy");
        assert!(clean_rel("").is_err());
        assert!(clean_rel("../x").is_err());
        assert!(clean_rel("game//x").is_err());
        assert!(clean_rel("C:/x").is_err());
    }

    #[test]
    fn create_makes_parents_and_refuses_duplicates() {
        let root = scratch("create");
        let touched = do_create(&root, "game/sub/new.rpy", false).unwrap();
        assert_eq!(touched.len(), 1);
        assert!(root.join("game/sub/new.rpy").is_file());
        assert!(do_create(&root, "game/sub/NEW.rpy", false).is_err());
        assert!(do_create(&root, "game/folder", true).is_ok());
        assert!(root.join("game/folder").is_dir());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn rename_reports_old_and_new_files() {
        let root = scratch("rename");
        do_create(&root, "game/a/one.rpy", false).unwrap();
        let touched = do_rename(&root, "game/a", "game/b").unwrap();
        assert!(root.join("game/b/one.rpy").is_file());
        assert!(!root.join("game/a").exists());
        assert!(touched.iter().any(|p| p.ends_with("b/one.rpy") || p.ends_with("b\\one.rpy")));
        assert!(touched.iter().any(|p| p.ends_with("a/one.rpy") || p.ends_with("a\\one.rpy")));
        assert!(do_rename(&root, "game/b", "game/b/inner").is_err());
        assert!(do_rename(&root, "game/missing", "game/x").is_err());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn rename_allows_a_case_only_change() {
        let root = scratch("case");
        do_create(&root, "game/file.txt", false).unwrap();
        do_rename(&root, "game/file.txt", "game/File.txt").unwrap();
        let names: Vec<String> = fs::read_dir(root.join("game"))
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert!(names.contains(&"File.txt".to_string()));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn move_refuses_self_and_collisions() {
        let root = scratch("move");
        do_create(&root, "game/a/x.txt", false).unwrap();
        do_create(&root, "game/b/x.txt", false).unwrap();
        do_create(&root, "game/c", true).unwrap();
        assert!(do_move(&root, &["game/a".into()], "game/a/deeper").is_err());
        assert!(do_move(&root, &["game/a/x.txt".into()], "game/b").is_err());
        let (_, moved) = do_move(&root, &["game/a/x.txt".into()], "game/c").unwrap();
        assert_eq!(moved, vec!["game/c/x.txt".to_string()]);
        assert!(root.join("game/c/x.txt").is_file());
        let (_, up) = do_move(&root, &["game/c/x.txt".into()], "").unwrap();
        assert_eq!(up, vec!["x.txt".to_string()]);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn delete_keeps_a_backup_copy() {
        let root = scratch("delete");
        let backup = scratch("backup");
        do_create(&root, "game/gone/inner.txt", false).unwrap();
        fs::write(root.join("game/gone/inner.txt"), "keep me").unwrap();
        let touched = do_delete(&root, "game/gone", &backup).unwrap();
        assert_eq!(touched.len(), 1);
        assert!(!root.join("game/gone").exists());
        let mut found = Vec::new();
        files_under(&backup, &mut found);
        assert!(found
            .iter()
            .any(|p| fs::read_to_string(p).map(|t| t == "keep me").unwrap_or(false)));
        assert!(do_delete(&root, "game/gone", &backup).is_err());
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&backup);
    }
}
