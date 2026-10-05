//! Search, file operations, symbols, assets, translations, logs and project lifecycle.

use crate::error::AppError;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::sync::Arc;

use serde::Serialize;
use tauri::State;

use crate::commands::AppState;

use super::*;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetFile {
    pub path: String,
    pub kind: String,
    pub bytes: u64,
    pub used: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MissingAsset {
    pub path: String,
    pub line: u32,
    pub file: String,
    pub what: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetReport {
    pub files: Vec<AssetFile>,
    pub files_total: u32,
    pub missing: Vec<MissingAsset>,
    pub missing_total: u32,
    pub unused: u32,
}

fn asset_kind(name: &str) -> Option<&'static str> {
    let ext = name
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    Some(match ext.as_str() {
        "png" | "jpg" | "jpeg" | "webp" | "gif" | "avif" => "image",
        "ogg" | "mp3" | "wav" | "opus" => "audio",
        "webm" | "mp4" | "ogv" => "movie",
        _ => return None,
    })
}

fn file_ext(name: &str) -> String {
    name.rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .unwrap_or_default()
}

/// How the explorer can show this file. Broader than `asset_kind`, which stays the game media index.
pub(crate) fn preview_kind(name: &str) -> Option<&'static str> {
    Some(match file_ext(name).as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "avif" | "bmp" | "ico" | "svg" => "image",
        "mp3" | "wav" | "ogg" | "opus" | "flac" | "m4a" | "aac" => "audio",
        "mp4" | "webm" | "ogv" | "mov" => "video",
        "txt" | "md" | "markdown" | "rst" | "log" | "json" | "yaml" | "yml" | "toml" | "xml"
        | "html" | "htm" | "css" | "csv" | "tsv" | "ini" | "cfg" | "conf" | "srt" | "vtt"
        | "py" | "js" | "mjs" | "ts" | "lua" | "sh" | "rpy" | "rpym" => "text",
        _ => return None,
    })
}

fn preview_limit(kind: &str) -> u64 {
    match kind {
        "text" => 512_000,
        "video" => 32_000_000,
        _ => 8_000_000,
    }
}

fn norm_path(p: &str) -> String {
    p.replace('\\', "/").trim_start_matches('/').to_string()
}

const ASSET_LIST_CAP: usize = 4000;

/// Archive entries the game would load, highest-priority archive first.
/// A later copy of the same path is dropped. Loose files are applied separately.
fn archived_assets(archives: &[renpy_core::project::LoadedArchive]) -> Vec<(String, u64)> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for loaded in archives {
        let Some(archive) = &loaded.archive else {
            continue;
        };
        for (name, entry) in &archive.entries {
            let rel = norm_path(name);
            if asset_kind(&rel).is_none() {
                continue;
            }
            if !seen.insert(rel.to_ascii_lowercase()) {
                continue;
            }
            out.push((rel, entry.declared_len().unwrap_or(0)));
        }
    }
    out
}

pub(crate) fn append_archived_assets(
    files: &mut Vec<AssetFile>,
    total: &mut u32,
    archived: &[(String, u64)],
) {
    let mut have: HashSet<String> = files.iter().map(|f| f.path.to_ascii_lowercase()).collect();
    for (path, bytes) in archived {
        let rel = norm_path(path);
        let Some(kind) = asset_kind(&rel) else {
            continue;
        };
        if !have.insert(rel.to_ascii_lowercase()) {
            continue;
        }
        *total = total.saturating_add(1);
        if files.len() >= ASSET_LIST_CAP {
            continue;
        }
        files.push(AssetFile {
            path: rel,
            kind: kind.into(),
            bytes: *bytes,
            used: false,
        });
    }
}

fn archive_entry_name<'a>(
    entries: &'a std::collections::BTreeMap<String, renpy_core::rpa::Entry>,
    rel: &str,
) -> Option<&'a str> {
    if let Some((key, _)) = entries.get_key_value(rel) {
        return Some(key.as_str());
    }
    entries
        .keys()
        .find(|name| norm_path(name).eq_ignore_ascii_case(rel))
        .map(String::as_str)
}

#[tauri::command(async)]
pub fn asset_report(state: State<'_, AppState>) -> Result<AssetReport, AppError> {
    let (game_dir, refs, image_uses, present_names, archive_assets) = {
        let guard = crate::util::lock(&state.project);
        let project = guard.as_ref().ok_or_else(crate::util::no_project)?;
        let mut refs = Vec::new();
        let mut image_uses = std::collections::HashSet::new();
        for f in &project.files {
            for q in &f.meta.quoted_paths {
                refs.push((q.path.clone(), q.line, f.rel.clone(), q.kind.to_string()));
            }
            collect_image_uses(&f.stmts, &mut image_uses);
        }
        let archive_assets = archived_assets(&project.archives);
        let mut present_names = Vec::new();
        for loaded in &project.archives {
            if let Some(archive) = &loaded.archive {
                present_names.extend(archive.entries.keys().cloned());
            }
        }
        (
            project.game_dir.clone(),
            refs,
            image_uses,
            present_names,
            archive_assets,
        )
    };

    let mut present: std::collections::HashSet<String> = std::collections::HashSet::new();
    for name in &present_names {
        present.insert(norm_path(name).to_ascii_lowercase());
    }
    let mut files = Vec::new();
    let mut files_total = 0u32;
    walk_assets(&game_dir, &game_dir, &mut files, &mut files_total);
    append_archived_assets(&mut files, &mut files_total, &archive_assets);
    for f in &files {
        present.insert(f.path.to_ascii_lowercase());
    }

    let mut missing = Vec::new();
    let mut missing_total = 0u32;
    let mut used_files: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (path, line, file, what) in &refs {
        let key = norm_path(path).to_ascii_lowercase();
        let found = present.contains(&key)
            || present.contains(&format!("images/{key}"))
            || present.contains(&format!("audio/{key}"));
        if found {
            used_files.insert(key.clone());
            used_files.insert(format!("images/{key}"));
            used_files.insert(format!("audio/{key}"));
        } else {
            missing_total = missing_total.saturating_add(1);
            if missing.len() < 200 {
                missing.push(MissingAsset {
                    path: path.clone(),
                    line: *line,
                    file: file.clone(),
                    what: what.clone(),
                });
            }
        }
    }

    let mut unused = 0u32;
    let image_names: HashSet<String> = image_uses.iter().map(|n| n.to_lowercase()).collect();
    for f in &mut files {
        let key = f.path.to_ascii_lowercase();
        let stem = Path::new(&f.path)
            .file_stem()
            .map(|s| s.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        let referenced =
            used_files.contains(&key) || image_names.contains(&stem) || image_names.contains(&key);
        f.used = referenced;
        if !referenced {
            unused += 1;
        }
    }
    Ok(AssetReport {
        files,
        files_total,
        missing,
        missing_total,
        unused,
    })
}

fn walk_assets(game_dir: &Path, dir: &Path, out: &mut Vec<AssetFile>, total: &mut u32) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name == "cache" || name == "saves" {
            continue;
        }
        let Ok(ty) = e.file_type() else { continue };
        if ty.is_dir() {
            walk_assets(game_dir, &p, out, total);
            continue;
        }
        let Some(kind) = asset_kind(&name) else {
            continue;
        };
        *total = total.saturating_add(1);
        if out.len() >= ASSET_LIST_CAP {
            continue;
        }
        let rel = p
            .strip_prefix(game_dir)
            .unwrap_or(&p)
            .to_string_lossy()
            .replace('\\', "/");
        let bytes = e.metadata().map(|m| m.len()).unwrap_or(0);
        out.push(AssetFile {
            path: rel,
            kind: kind.into(),
            bytes,
            used: false,
        });
    }
}

/// A media path inside `game/`. Rejects absolute paths, drive letters, and `..`.
pub(crate) fn asset_rel(path: &str) -> Result<String, AppError> {
    let rel = dir_rel(path)
        .map_err(|_| "That is not an image or audio file inside game/.".to_string())?;
    if rel.is_empty() || asset_kind(&rel).is_none() {
        return Err("That is not an image or audio file inside game/.".into());
    }
    Ok(rel)
}

#[tauri::command(async)]
pub fn read_asset(
    state: State<'_, AppState>,
    path: String,
) -> Result<tauri::ipc::Response, AppError> {
    let rel = asset_rel(&path)?;
    let guard = crate::util::lock(&state.project);
    let project = guard.as_ref().ok_or_else(crate::util::no_project)?;
    let abs = abs_under(&project.game_dir, &rel);
    if abs.is_file() {
        drop(guard);
        let bytes = fs::read(&abs)?;
        if bytes.len() > 4_000_000 {
            return Err("That file is too large to preview.".into());
        }
        return Ok(tauri::ipc::Response::new(bytes));
    }
    let archived = project.archives.iter().find_map(|loaded| {
        let archive = loaded.archive.as_ref()?;
        let name = archive_entry_name(&archive.entries, &rel)?;
        Some((Arc::clone(archive), name.to_string()))
    });
    drop(guard);
    if let Some((archive, name)) = archived {
        let bytes = archive.read_entry(&name, 4_000_000)?;
        return Ok(tauri::ipc::Response::new(bytes));
    }
    Err(format!("`{rel}` is not in this project.").into())
}

fn strip_game_prefix<'a>(game_prefix: &str, rel: &'a str) -> Option<&'a str> {
    if game_prefix.is_empty() {
        return Some(rel);
    }
    if rel.eq_ignore_ascii_case(game_prefix) {
        return Some("");
    }
    let full = rel.as_bytes();
    let prefix = game_prefix.as_bytes();
    if full.len() > prefix.len()
        && full[..prefix.len()].eq_ignore_ascii_case(prefix)
        && full[prefix.len()] == b'/'
    {
        return rel.get(prefix.len() + 1..);
    }
    None
}

/// Bytes of a previewable file. `path` is relative to the project root, not only `game/`.
#[tauri::command(async)]
pub fn read_preview(
    state: State<'_, AppState>,
    path: String,
) -> Result<tauri::ipc::Response, AppError> {
    let rel = dir_rel(&path)?;
    if rel.is_empty() {
        return Err("That path is not a file.".into());
    }
    let kind = preview_kind(&rel).ok_or_else(|| "That file cannot be previewed.".to_string())?;
    let limit = preview_limit(kind);
    let guard = crate::util::lock(&state.project);
    let project = guard.as_ref().ok_or_else(crate::util::no_project)?;
    let abs = abs_under(&project.root, &rel);
    if abs.is_file() {
        drop(guard);
        let bytes = fs::read(&abs)?;
        if bytes.len() as u64 > limit {
            return Err("That file is too large to preview.".into());
        }
        return Ok(tauri::ipc::Response::new(bytes));
    }
    let game_prefix = project
        .game_dir
        .strip_prefix(&project.root)
        .unwrap_or(project.game_dir.as_path())
        .to_string_lossy()
        .replace('\\', "/");
    let archived = strip_game_prefix(&game_prefix, &rel)
        .filter(|game_rel| !game_rel.is_empty())
        .and_then(|game_rel| {
            project.archives.iter().find_map(|loaded| {
                let archive = loaded.archive.as_ref()?;
                let name = archive_entry_name(&archive.entries, game_rel)?;
                Some((Arc::clone(archive), name.to_string()))
            })
        });
    drop(guard);
    if let Some((archive, name)) = archived {
        let bytes = archive.read_entry(&name, limit)?;
        return Ok(tauri::ipc::Response::new(bytes));
    }
    Err(format!("`{rel}` is not in this project.").into())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirEntry {
    pub name: String,
    pub path: String,
    pub dir: bool,
}

fn dir_rel(rel: &str) -> Result<String, AppError> {
    let rel = rel.trim().replace('\\', "/");
    let rel = rel.trim_matches('/');
    if rel.is_empty() {
        return Ok(String::new());
    }
    if rel.contains(':')
        || rel
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err("That path is not inside the project.".into());
    }
    Ok(rel.to_string())
}

pub(crate) fn hidden_listing(name: &str, dir: bool) -> bool {
    if name.is_empty() || name.starts_with('.') {
        return true;
    }
    if dir && (name.eq_ignore_ascii_case("cache") || name.eq_ignore_ascii_case("saves")) {
        return true;
    }
    let stem = name.rsplit_once('.').map(|(stem, _)| stem).unwrap_or(name);
    matches!(
        stem.to_ascii_lowercase().as_str(),
        "vnide_developer" | "vnide_live" | "vnide_images" | "vnide_autoreload"
    )
}

fn child_path(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_string()
    } else {
        format!("{prefix}/{name}")
    }
}

/// The immediate child of `prefix` named by an archive path, if that path sits under it.
fn immediate_child(prefix: &str, full: &str) -> Option<DirEntry> {
    let full = norm_path(full);
    if full.is_empty() {
        return None;
    }
    let rest = if prefix.is_empty() {
        full.as_str()
    } else {
        let full_b = full.as_bytes();
        let pre_b = prefix.as_bytes();
        if full_b.len() <= pre_b.len()
            || !full_b[..pre_b.len()].eq_ignore_ascii_case(pre_b)
            || full_b[pre_b.len()] != b'/'
        {
            return None;
        }
        let rest = full.get(pre_b.len() + 1..)?;
        if rest.is_empty() {
            return None;
        }
        rest
    };
    let (name, dir) = match rest.split_once('/') {
        Some((head, tail)) if !head.is_empty() && !tail.is_empty() => (head, true),
        Some(_) => return None,
        None => (rest, false),
    };
    if hidden_listing(name, dir) {
        return None;
    }
    Some(DirEntry {
        path: child_path(prefix, name),
        name: name.to_string(),
        dir,
    })
}

pub(crate) fn merge_listing(
    prefix: &str,
    mut disk: Vec<DirEntry>,
    archived: &[String],
) -> Vec<DirEntry> {
    disk.retain(|entry| !hidden_listing(&entry.name, entry.dir));
    let mut seen: HashSet<String> = disk
        .iter()
        .map(|entry| entry.name.to_ascii_lowercase())
        .collect();
    for name in archived {
        let Some(child) = immediate_child(prefix, name) else {
            continue;
        };
        if !seen.insert(child.name.to_ascii_lowercase()) {
            continue;
        }
        disk.push(child);
    }
    disk.sort_by(|a, b| match (a.dir, b.dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a
            .name
            .to_ascii_lowercase()
            .cmp(&b.name.to_ascii_lowercase()),
    });
    disk
}

pub(crate) fn with_game_prefix(game_prefix: &str, name: &str) -> String {
    let name = norm_path(name);
    if game_prefix.is_empty() || name.is_empty() {
        name
    } else {
        format!("{game_prefix}/{name}")
    }
}

fn read_disk_children(root: &Path, rel: &str) -> Result<Vec<DirEntry>, AppError> {
    let dir = if rel.is_empty() {
        root.to_path_buf()
    } else {
        abs_under(root, rel)
    };
    if !dir.exists() {
        return Ok(Vec::new());
    }
    if !dir.is_dir() {
        return Err("That path is not a folder.".into());
    }
    let mut out = Vec::new();
    for entry in fs::read_dir(&dir)?.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Ok(ty) = entry.file_type() else { continue };
        let is_dir = ty.is_dir();
        if hidden_listing(&name, is_dir) {
            continue;
        }
        out.push(DirEntry {
            path: child_path(rel, &name),
            name,
            dir: is_dir,
        });
    }
    Ok(out)
}

/// One folder under the project root. An empty path is that root, so `game/` is one child among the rest.
#[tauri::command(async)]
pub fn list_dir(state: State<'_, AppState>, path: String) -> Result<Vec<DirEntry>, AppError> {
    let rel = dir_rel(&path)?;
    let (root, names) = {
        let guard = crate::util::lock(&state.project);
        let project = guard.as_ref().ok_or_else(crate::util::no_project)?;
        let game_prefix = project
            .game_dir
            .strip_prefix(&project.root)
            .unwrap_or(project.game_dir.as_path())
            .to_string_lossy()
            .replace('\\', "/");
        let mut names = Vec::new();
        for loaded in &project.archives {
            if let Some(archive) = &loaded.archive {
                names.extend(
                    archive
                        .entries
                        .keys()
                        .map(|name| with_game_prefix(&game_prefix, name)),
                );
            }
        }
        (project.root.clone(), names)
    };
    let disk = read_disk_children(&root, &rel)?;
    Ok(merge_listing(&rel, disk, &names))
}

/// Image names used by show/scene, so loose files with the same stem count as used.
fn collect_image_uses(
    stmts: &[renpy_core::ast::Stmt],
    out: &mut std::collections::HashSet<String>,
) {
    renpy_core::analysis::walk_all(stmts, &mut |s| {
        if let renpy_core::ast::Kind::Present {
            cmd, name: Some(n), ..
        } = &s.kind
        {
            if *cmd == "show" || *cmd == "scene" {
                if let Some(tag) = n.split_whitespace().next() {
                    out.insert(tag.to_string());
                }
                out.insert(n.clone());
            }
        }
    });
}
