//! IPC commands exposed to the frontend.

use crate::error::AppError;
use std::sync::Arc;

use renpy_core::{sha256_file, ArchiveDigest};
use tauri::{AppHandle, Emitter, State};

use super::*;

fn digest_key(digest: &ArchiveDigest) -> String {
    format!(
        "{}|{}|{}",
        digest.path,
        digest.bytes,
        digest.modified.as_deref().unwrap_or("")
    )
}

/// SHA-256 of each top-level archive. Results are cached next to layout data
/// and reused while the path, size and modification time stay the same.
#[tauri::command(async)]
pub fn archive_fingerprints(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<ArchiveDigest>, AppError> {
    let (root, game_dir, mut digests) = {
        let guard = crate::util::lock(&state.project);
        let project = guard.as_ref().ok_or_else(crate::util::no_project)?;
        let game = project.game_info();
        (
            project.root.clone(),
            project.game_dir.clone(),
            game.archives,
        )
    };
    let path = crate::util::fingerprint_cache_path(&app, &root)?;
    let mut cached: std::collections::BTreeMap<String, String> = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    let mut changed = false;
    for digest in &mut digests {
        let key = digest_key(digest);
        if let Some(hit) = cached.get(&key) {
            digest.sha256 = Some(hit.clone());
            continue;
        }
        let abs = game_dir.join(digest.path.replace('/', std::path::MAIN_SEPARATOR_STR));
        let hash = sha256_file(&abs)?;
        cached.insert(key, hash.clone());
        digest.sha256 = Some(hash);
        changed = true;
    }
    if changed {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = serde_json::to_string(&cached)?;
        std::fs::write(path, text)?;
    }
    Ok(digests)
}

/// Layout positions are cached in the app data directory, never in the project.
#[tauri::command(async)]
pub fn layout_cache_get(app: AppHandle, key: String) -> Result<Option<String>, AppError> {
    let path = crate::util::layout_cache_path(&app, &key)?;
    Ok(std::fs::read_to_string(path).ok())
}

#[tauri::command(async)]
pub fn layout_cache_put(app: AppHandle, key: String, data: String) -> Result<(), AppError> {
    let path = crate::util::layout_cache_path(&app, &key)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, data).map_err(crate::error::AppError::from)
}

#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RpaProgress {
    pub done: u64,
    pub total: u64,
    pub label: String,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveEntryInfo {
    pub name: String,
    pub len: u64,
    pub kind: String,
}

fn entry_kind(name: &str) -> String {
    let ext = name
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "rpy" | "rpym" => "script",
        "rpyc" | "rpymc" => "compiled",
        "png" | "jpg" | "jpeg" | "webp" | "gif" | "avif" => "image",
        "ogg" | "mp3" | "wav" | "opus" => "audio",
        _ => "other",
    }
    .into()
}

fn inside_dir(parent: &std::path::Path, child: &std::path::Path) -> bool {
    let parent = parent
        .to_string_lossy()
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_ascii_lowercase();
    let child = child
        .to_string_lossy()
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_ascii_lowercase();
    child == parent || child.starts_with(&(parent + "/"))
}

#[tauri::command(async)]
pub fn archive_list(
    state: State<'_, AppState>,
    path: String,
) -> Result<Vec<ArchiveEntryInfo>, AppError> {
    crate::util::with_project(&state, |project| {
        let loaded = project
            .archives
            .iter()
            .find(|a| a.info.path == path)
            .ok_or_else(|| format!("`{path}` is not an archive of this project."))?;
        if let Some(err) = &loaded.info.error {
            return Err(err.clone().into());
        }
        let archive = loaded
            .archive
            .as_ref()
            .ok_or_else(|| format!("`{path}` is not open."))?;
        let mut out: Vec<ArchiveEntryInfo> = archive
            .entries
            .iter()
            .map(|(name, entry)| ArchiveEntryInfo {
                name: name.clone(),
                len: entry.declared_len().unwrap_or(0),
                kind: entry_kind(name),
            })
            .collect();
        out.sort_by_key(|a| a.name.to_lowercase());
        Ok(out)
    })
}

#[tauri::command(async)]
pub fn archive_read_entry(
    state: State<'_, AppState>,
    archive: String,
    name: String,
) -> Result<tauri::ipc::Response, AppError> {
    let opened = {
        let guard = crate::util::lock(&state.project);
        let project = guard.as_ref().ok_or_else(crate::util::no_project)?;
        project
            .archives
            .iter()
            .find(|a| a.info.path == archive)
            .and_then(|a| a.archive.as_ref())
            .map(Arc::clone)
            .ok_or_else(|| format!("`{archive}` is not open."))?
    };
    let bytes = opened.read_entry(&name, renpy_core::rpa::PREVIEW_MAX)?;
    Ok(tauri::ipc::Response::new(bytes))
}

#[tauri::command]
pub fn archive_cancel(state: State<'_, AppState>) -> Result<(), AppError> {
    state
        .cancel
        .store(true, std::sync::atomic::Ordering::Relaxed);
    Ok(())
}

#[tauri::command]
pub async fn archive_extract(
    app: AppHandle,
    state: State<'_, AppState>,
    archive: String,
    dest: String,
    names: Option<Vec<String>>,
    allow_inside_game: bool,
) -> Result<u32, AppError> {
    let (archive_path, game_dir, root) = {
        let guard = crate::util::lock(&state.project);
        let project = guard.as_ref().ok_or_else(crate::util::no_project)?;
        let rel = archive.clone();
        let path = project
            .game_dir
            .join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
        if !project.archives.iter().any(|a| a.info.path == archive) {
            return Err(format!("`{archive}` is not an archive of this project.").into());
        }
        (path, project.game_dir.clone(), project.root.clone())
    };
    let dest = if dest.trim().is_empty() {
        let stem = std::path::Path::new(&archive)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "archive".into());
        root.join("extracted").join(stem)
    } else {
        std::path::PathBuf::from(&dest)
    };
    if inside_dir(&game_dir, &dest) {
        if !allow_inside_game {
            return Err(
                "That folder is inside game/. Every extracted file would override the archive. Choose another folder, or extract anyway."
                    .into(),
            );
        }
        let guard = crate::util::lock(&state.project);
        ensure_game_closed(guard.as_ref().ok_or_else(crate::util::no_project)?)?;
    }
    state
        .cancel
        .store(false, std::sync::atomic::Ordering::Relaxed);
    let cancel = state.cancel.clone();
    let app_progress = app.clone();
    let filter: Option<std::collections::HashSet<String>> = names.map(|n| n.into_iter().collect());
    let report = tauri::async_runtime::spawn_blocking(move || {
        let opened = renpy_core::rpa::Archive::open(&archive_path)?;
        let progress = move |done, total| {
            let _ = app_progress.emit(
                "rpa:progress",
                RpaProgress {
                    done,
                    total,
                    label: format!("Extracting {done}/{total}"),
                },
            );
        };
        renpy_core::rpa::extract(
            &opened,
            &dest,
            &renpy_core::rpa::ExtractOptions {
                names: filter.as_ref(),
                overwrite: true,
                progress: Some(&progress),
                cancel: Some(&cancel),
            },
        )
        .map_err(crate::error::AppError::from)
    })
    .await??;
    Ok(report.files)
}

#[tauri::command]
pub async fn archive_build(
    state: State<'_, AppState>,
    src_dir: String,
    out_path: String,
) -> Result<u32, AppError> {
    let out = std::path::PathBuf::from(&out_path);
    let src = std::path::PathBuf::from(&src_dir);
    {
        let guard = crate::util::lock(&state.project);
        let project = guard.as_ref().ok_or_else(crate::util::no_project)?;
        if inside_dir(&project.game_dir, &out) {
            ensure_game_closed(project)?;
        }
    }
    state
        .cancel
        .store(false, std::sync::atomic::Ordering::Relaxed);
    let cancel = state.cancel.clone();
    let out_for_build = out.clone();
    let report = tauri::async_runtime::spawn_blocking(move || {
        renpy_core::rpa::build_from_dir(&src, &out_for_build, Some(&cancel))
            .map_err(crate::error::AppError::from)
    })
    .await??;
    if let Ok(guard) = state.project.lock() {
        if let Some(project) = guard.as_ref() {
            if inside_dir(&project.game_dir, &out) {
                drop(guard);
                let mut guard = crate::util::lock(&state.project);
                if let Some(project) = guard.as_mut() {
                    project.refresh_archives();
                }
            }
        }
    }
    Ok(report.files)
}
