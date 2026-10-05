//! IPC commands exposed to the frontend.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use renpy_core::engine::ImageRun;
use renpy_core::{
    sha256_file, ArchiveDigest, DiagReport, EngineRun, LabelGraph, Project, ProjectInfo, ProjectMap,
};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::edit::{self, EditImpact, EditState};
use crate::launch::{self, LaunchReport};
use crate::watch::{self, WatchHandle};

#[derive(Default)]
pub struct AppState {
    pub project: Mutex<Option<Project>>,
    pub watcher: Mutex<Option<WatchHandle>>,
    pub edit: Mutex<EditState>,
    /// Undo and redo for edits made from the flow. Taken after `project` and `edit`.
    pub scene_history: Mutex<crate::ide::SceneHistory>,
    pub cancel: Arc<AtomicBool>,
    pub live: Mutex<Option<crate::live::Session>>,
    /// Set while an SDK download should stop.
    pub sdk_cancel: Arc<AtomicBool>,
    pub sdk_busy: AtomicBool,
    /// Set while a launcher build should stop.
    pub build_cancel: Arc<AtomicBool>,
    /// The launcher process for the current build, so it can be killed.
    pub build: Arc<Mutex<Option<std::process::Child>>>,
}

pub(crate) fn ensure_game_closed(project: &Project) -> Result<(), String> {
    let Some(launcher) = &project.launcher else {
        return Ok(());
    };
    let name = launcher
        .exe
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if crate::launch::game_running(&name) {
        return Err(format!(
            "{name} is running. Close the game before changing files in game/."
        ));
    }
    Ok(())
}

fn reset_editing(state: &State<'_, AppState>) -> Result<(), String> {
    state
        .edit
        .lock()
        .map_err(|e| e.to_string())?
        .pending
        .clear();
    state
        .scene_history
        .lock()
        .map_err(|e| e.to_string())?
        .clear();
    Ok(())
}

pub(crate) fn backup_dir(app: &AppHandle, root: &std::path::Path) -> Result<PathBuf, String> {
    let data = app.path().app_data_dir().map_err(|e| e.to_string())?;
    Ok(edit::backup_root(&data, root))
}

fn no_project() -> String {
    "No project is open.".into()
}

#[tauri::command]
pub fn initial_project() -> Option<String> {
    if let Ok(p) = std::env::var("VN_IDE_PROJECT") {
        if !p.trim().is_empty() {
            return Some(p);
        }
    }
    std::env::args().nth(1).filter(|a| !a.starts_with('-'))
}

/// Optional label graph to open right after the project loads (`VN_IDE_LABEL`).
#[tauri::command]
pub fn initial_label() -> Option<String> {
    std::env::var("VN_IDE_LABEL")
        .ok()
        .filter(|l| !l.trim().is_empty())
}

/// Optional comma-separated startup actions (`VN_IDE_ACTION`): `engine`, `lint`,
/// `problems`, `run`, `play`. Used for scripted checks; unset in normal use.
#[tauri::command]
pub fn initial_actions() -> Vec<String> {
    std::env::var("VN_IDE_ACTION")
        .map(|v| {
            v.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

#[tauri::command(async)]
pub fn open_project(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<ProjectInfo, String> {
    crate::live::stop_session(&app, std::time::Duration::from_millis(400));
    let cache = app
        .path()
        .app_data_dir()
        .ok()
        .map(|dir| dir.join("rpyc-cache"));
    let mut project = Project::open_with(Path::new(&path), cache.as_deref())?;
    launch::remove_leftover_shims(&project);
    let restored = edit::recover_interrupted_saves(&project.game_dir);
    if !restored.is_empty() {
        project.refresh_paths(&restored);
    }
    // Reuse an engine dump taken earlier when the scripts have not changed since.
    if let Some(run) = load_cached_engine(&app, &project) {
        project.set_engine(run);
    }
    if let Some(run) = load_cached_images(&app, &project) {
        project.set_stage_images(run);
    }
    let info = project.info();
    let game_dir = project.game_dir.clone();
    // Stop the old watcher before swapping projects.
    *state.watcher.lock().map_err(|e| e.to_string())? = None;
    reset_editing(&state)?;
    *state.project.lock().map_err(|e| e.to_string())? = Some(project);
    match watch::start(app, game_dir) {
        Ok(handle) => *state.watcher.lock().map_err(|e| e.to_string())? = Some(handle),
        Err(e) => log::warn!("file watcher unavailable: {e}"),
    }
    Ok(info)
}

#[tauri::command(async)]
pub fn get_project_info(state: State<'_, AppState>) -> Result<Option<ProjectInfo>, String> {
    let guard = state.project.lock().map_err(|e| e.to_string())?;
    Ok(guard.as_ref().map(|p| p.info()))
}

#[tauri::command(async)]
pub fn get_project_map(state: State<'_, AppState>) -> Result<ProjectMap, String> {
    let analysis = {
        let guard = state.project.lock().map_err(|e| e.to_string())?;
        guard.as_ref().ok_or_else(no_project)?.analysis.clone()
    };
    Ok(analysis.map.clone())
}

#[tauri::command(async)]
pub fn get_diagnostics(state: State<'_, AppState>) -> Result<DiagReport, String> {
    let analysis = {
        let guard = state.project.lock().map_err(|e| e.to_string())?;
        guard.as_ref().ok_or_else(no_project)?.analysis.clone()
    };
    Ok(analysis.diagnostics.clone())
}

#[tauri::command(async)]
pub fn get_label_graph(
    state: State<'_, AppState>,
    name: String,
    detail: bool,
) -> Result<LabelGraph, String> {
    let request = {
        let guard = state.project.lock().map_err(|e| e.to_string())?;
        let project = guard.as_ref().ok_or_else(no_project)?;
        if let Some(hit) = project.cached_label_graph(&name, detail) {
            return Ok(hit);
        }
        project
            .graph_request(&name, detail)
            .ok_or_else(|| format!("Label `{name}` not found."))?
    };
    let graph = request
        .build()
        .ok_or_else(|| format!("Label `{name}` not found."))?;
    if let Ok(guard) = state.project.lock() {
        if let Some(project) = guard.as_ref() {
            project.store_label_graph(&request, &graph);
        }
    }
    Ok(graph)
}

#[tauri::command(async)]
pub fn get_label_lines(
    state: State<'_, AppState>,
    name: String,
) -> Result<renpy_core::flow::LabelLines, String> {
    let guard = state.project.lock().map_err(|e| e.to_string())?;
    let project = guard.as_ref().ok_or_else(no_project)?;
    project
        .label_lines(&name)
        .ok_or_else(|| format!("Label `{name}` not found."))
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupPatch {
    pub path: String,
    pub find: String,
    pub replace: String,
}

/// Optional scripted revert (`VN_IDE_REVERT` = path relative to `game/`). Checks only.
#[tauri::command]
pub fn initial_revert() -> Option<String> {
    std::env::var("VN_IDE_REVERT")
        .ok()
        .filter(|s| !s.trim().is_empty())
}

/// Optional scripted edit (`VN_IDE_PATCH` = `file|find|replace`) used by checks.
/// Unset in normal use. Applied by the frontend after the project opens.
#[tauri::command]
pub fn initial_patch() -> Option<StartupPatch> {
    let value = std::env::var("VN_IDE_PATCH").ok()?;
    let mut parts = value.splitn(3, '|');
    let path = parts.next()?.trim().to_string();
    let find = parts.next()?.to_string();
    let replace = parts.next()?.to_string();
    if path.is_empty() || find.is_empty() {
        return None;
    }
    Some(StartupPatch {
        path,
        find,
        replace,
    })
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditStatus {
    pub modified: Vec<String>,
    /// Saved scripts that were recovered from a `.rpyc` and can be packaged.
    pub decompiled: Vec<String>,
}

#[tauri::command(async)]
pub fn edit_status(app: AppHandle, state: State<'_, AppState>) -> Result<EditStatus, String> {
    let guard = state.project.lock().map_err(|e| e.to_string())?;
    let project = guard.as_ref().ok_or_else(no_project)?;
    let root = backup_dir(&app, &project.root)?;
    Ok(EditStatus {
        modified: edit::modified_files(project, &root),
        decompiled: edit::decompiled_edits(project, &root),
    })
}

#[tauri::command(async)]
pub fn write_file(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    text: String,
) -> Result<EditImpact, String> {
    // Project lock first, then the edit lock: the watcher takes them in this order.
    // Analysis runs after both locks drop so a label-graph read is not blocked.
    let (before, job) = {
        let mut guard = state.project.lock().map_err(|e| e.to_string())?;
        let project = guard.as_mut().ok_or_else(no_project)?;
        ensure_game_closed(project)?;
        let mut edit = state.edit.lock().map_err(|e| e.to_string())?;
        let root = backup_dir(&app, &project.root)?;
        let Some(before) = edit::prepare_write(project, &mut edit, &root, &path, &text)? else {
            return Ok(edit::EditImpact::default());
        };
        (before, project.fork_analysis())
    };
    let analysis = job.run();
    let mut guard = state.project.lock().map_err(|e| e.to_string())?;
    let project = guard.as_mut().ok_or_else(no_project)?;
    // Report the effect of this edit from the run that followed it, even if a
    // newer edit has since taken over the stored analysis.
    project.publish_analysis(job.epoch, std::sync::Arc::clone(&analysis));
    Ok(edit::diff_impact(&before, &analysis))
}

#[tauri::command(async)]
pub fn revert_file(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<EditImpact, String> {
    let mut guard = state.project.lock().map_err(|e| e.to_string())?;
    let project = guard.as_mut().ok_or_else(no_project)?;
    ensure_game_closed(project)?;
    let mut edit = state.edit.lock().map_err(|e| e.to_string())?;
    let root = backup_dir(&app, &project.root)?;
    edit::apply_revert(project, &mut edit, &root, &path)
}

/// Syntax of an unsaved buffer. Does not re-run project analysis.
#[tauri::command(async)]
pub fn check_syntax(text: String) -> Vec<renpy_core::SyntaxIssue> {
    renpy_core::parser::check_syntax(&text)
}

/// Raw bytes of a project script (path relative to `game/`), sent as an ArrayBuffer.
#[tauri::command(async)]
pub fn read_file(state: State<'_, AppState>, path: String) -> Result<tauri::ipc::Response, String> {
    enum Loaded {
        Bytes(Vec<u8>),
        Loose(PathBuf),
    }
    let loaded = {
        let guard = state.project.lock().map_err(|e| e.to_string())?;
        let project = guard.as_ref().ok_or_else(no_project)?;
        let Some(idx) = project.file_index(&path) else {
            return Err(format!("`{path}` is not a script of this project."));
        };
        let file = &project.files[idx];
        if let Some(text) = &file.source {
            Loaded::Bytes(text.as_bytes().to_vec())
        } else if matches!(file.origin, renpy_core::Origin::Archived { .. }) {
            Loaded::Bytes(project.read_script_bytes(&path)?)
        } else {
            Loaded::Loose(file.abs.clone())
        }
    };
    let bytes = match loaded {
        Loaded::Bytes(bytes) => bytes,
        Loaded::Loose(abs) => {
            std::fs::read(&abs).map_err(|e| format!("Could not read {}: {e}", abs.display()))?
        }
    };
    Ok(tauri::ipc::Response::new(bytes))
}

#[tauri::command(async)]
pub fn launch_game(
    app: AppHandle,
    state: State<'_, AppState>,
    launcher: Option<String>,
) -> Result<LaunchReport, String> {
    crate::live::stop_session(&app, std::time::Duration::from_millis(800));
    let source = {
        let guard = state.project.lock().map_err(|e| e.to_string())?;
        let project = guard.as_ref().ok_or_else(no_project)?;
        launch::LaunchSource::capture(project)
    };
    launch::launch(&source, launcher, None, launch::LaunchExtra::default())
}

#[tauri::command(async)]
pub fn warp_to(
    app: AppHandle,
    state: State<'_, AppState>,
    file: String,
    line: u32,
    launcher: Option<String>,
) -> Result<LaunchReport, String> {
    crate::live::stop_session(&app, std::time::Duration::from_millis(800));
    let (source, warp, label) = {
        let guard = state.project.lock().map_err(|e| e.to_string())?;
        let project = guard.as_ref().ok_or_else(no_project)?;
        if project.file_index(&file).is_none() {
            return Err(format!("`{file}` is not a script of this project."));
        }
        let label = crate::live::enclosing_label(project, &file, line);
        let (file, line) = project.engine_spec(&file, line);
        (launch::LaunchSource::capture(project), (file, line), label)
    };
    let mut extra = launch::LaunchExtra::default();
    extra.warp_label = label;
    launch::launch(&source, launcher, Some(warp), extra)
}

fn cache_name(key: &str) -> String {
    use sha2::{Digest, Sha256};
    let dig = Sha256::digest(key.as_bytes());
    dig.iter().take(8).map(|b| format!("{b:02x}")).collect()
}

fn engine_cache_path(app: &AppHandle, root: &std::path::Path) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("engine");
    Ok(dir.join(format!(
        "{}.json",
        cache_name(&root.to_string_lossy().to_lowercase())
    )))
}

fn load_cached_engine(app: &AppHandle, project: &Project) -> Option<EngineRun> {
    let path = engine_cache_path(app, &project.root).ok()?;
    let text = std::fs::read_to_string(path).ok()?;
    let mut run: EngineRun = serde_json::from_str(&text).ok()?;
    if run.key != project.source_key() {
        return None;
    }
    run.from_cache = true;
    Some(run)
}

fn image_cache_path(app: &AppHandle, root: &std::path::Path) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("stage-images");
    Ok(dir.join(format!(
        "{}.json",
        cache_name(&root.to_string_lossy().to_lowercase())
    )))
}

fn load_cached_images(app: &AppHandle, project: &Project) -> Option<ImageRun> {
    let path = image_cache_path(app, &project.root).ok()?;
    let text = std::fs::read_to_string(path).ok()?;
    let mut run: ImageRun = serde_json::from_str(&text).ok()?;
    // Kept even when the scripts have changed since. A newer `image` statement
    // wins for its own name until a fresh dump replaces this one.
    run.from_cache = true;
    Some(run)
}

pub(crate) fn save_cached_images(app: &AppHandle, root: &std::path::Path, run: &ImageRun) {
    if let Ok(path) = image_cache_path(app, root) {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(text) = serde_json::to_string(run) {
            let _ = std::fs::write(path, text);
        }
    }
}

fn save_cached_engine(app: &AppHandle, root: &std::path::Path, run: &EngineRun) {
    if let Ok(path) = engine_cache_path(app, root) {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(text) = serde_json::to_string(run) {
            let _ = std::fs::write(path, text);
        }
    }
}

fn engine_target(
    state: &State<'_, AppState>,
    launcher: Option<String>,
) -> Result<renpy_core::EngineTarget, String> {
    let guard = state.project.lock().map_err(|e| e.to_string())?;
    let project = guard.as_ref().ok_or_else(no_project)?;
    match launcher.filter(|s| !s.trim().is_empty()) {
        Some(path) => Ok(renpy_core::EngineTarget {
            root: project.root.clone(),
            game_dir: project.game_dir.clone(),
            launcher: renpy_core::Launcher {
                exe: PathBuf::from(path),
                prefix_args: Vec::new(),
            },
            key: project.source_key(),
            engine_version: project.engine_version.clone(),
        }),
        None => project.engine_target(),
    }
}

/// Run the game's bundled engine with `--json-dump` and merge the result.
#[tauri::command]
pub async fn engine_dump(
    app: AppHandle,
    state: State<'_, AppState>,
    launcher: Option<String>,
) -> Result<ProjectInfo, String> {
    let target = engine_target(&state, launcher)?;
    let t = target.clone();
    let run = tauri::async_runtime::spawn_blocking(move || {
        renpy_core::engine::run_json_dump(&t.root, &t.game_dir, &t.launcher, t.key.clone())
    })
    .await
    .map_err(|e| e.to_string())??;
    save_cached_engine(&app, &target.root, &run);
    let mut guard = state.project.lock().map_err(|e| e.to_string())?;
    let project = guard.as_mut().ok_or_else(no_project)?;
    if project.root != target.root {
        return Err("The project was switched while the engine was running.".into());
    }
    project.set_engine(run);
    Ok(project.info())
}

/// Run the game's engine through init and read the images it registered.
#[tauri::command]
pub async fn stage_images(
    app: AppHandle,
    state: State<'_, AppState>,
    launcher: Option<String>,
) -> Result<ProjectInfo, String> {
    if state
        .live
        .lock()
        .map(|slot| slot.is_some())
        .unwrap_or(false)
    {
        return Err(
            "The game is running. Stop it before asking the engine for a new image list.".into(),
        );
    }
    let target = engine_target(&state, launcher)?;
    let init_key = {
        let guard = state.project.lock().map_err(|e| e.to_string())?;
        guard.as_ref().ok_or_else(no_project)?.init_key()
    };
    let exe_name = target
        .launcher
        .exe
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let running = launch::game_running(&exe_name);
    let t = target.clone();
    let run = tauri::async_runtime::spawn_blocking(move || {
        renpy_core::engine::run_image_dump(&t.root, &t.game_dir, &t.launcher, init_key, running)
    })
    .await
    .map_err(|e| e.to_string())??;
    save_cached_images(&app, &target.root, &run);
    let mut guard = state.project.lock().map_err(|e| e.to_string())?;
    let project = guard.as_mut().ok_or_else(no_project)?;
    if project.root != target.root {
        return Err("The project was switched while the engine was running.".into());
    }
    project.set_stage_images(run);
    Ok(project.info())
}

/// Run the engine's own `lint` and add its findings to the problems list.
#[tauri::command]
pub async fn engine_lint(
    state: State<'_, AppState>,
    launcher: Option<String>,
) -> Result<ProjectInfo, String> {
    let target = engine_target(&state, launcher)?;
    let t = target.clone();
    let (items, notes) = tauri::async_runtime::spawn_blocking(move || {
        renpy_core::engine::run_lint(&t.root, &t.game_dir, &t.launcher)
    })
    .await
    .map_err(|e| e.to_string())??;
    let mut guard = state.project.lock().map_err(|e| e.to_string())?;
    let project = guard.as_mut().ok_or_else(no_project)?;
    if project.root != target.root {
        return Err("The project was switched while the engine was running.".into());
    }
    let _ = notes;
    project.set_lint(items);
    Ok(project.info())
}

fn cache_file(app: &AppHandle, key: &str) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("layouts");
    Ok(dir.join(format!("{}.json", cache_name(key))))
}

fn fingerprint_cache_path(app: &AppHandle, root: &Path) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("layouts");
    Ok(dir.join(format!(
        "fp-{}.json",
        cache_name(&root.to_string_lossy().to_lowercase())
    )))
}

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
) -> Result<Vec<ArchiveDigest>, String> {
    let (root, game_dir, mut digests) = {
        let guard = state.project.lock().map_err(|e| e.to_string())?;
        let project = guard.as_ref().ok_or_else(no_project)?;
        let game = project.game_info();
        (
            project.root.clone(),
            project.game_dir.clone(),
            game.archives,
        )
    };
    let path = fingerprint_cache_path(&app, &root)?;
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
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let text = serde_json::to_string(&cached).map_err(|e| e.to_string())?;
        std::fs::write(path, text).map_err(|e| e.to_string())?;
    }
    Ok(digests)
}

/// Layout positions are cached in the app data directory, never in the project.
#[tauri::command(async)]
pub fn layout_cache_get(app: AppHandle, key: String) -> Result<Option<String>, String> {
    let path = cache_file(&app, &key)?;
    Ok(std::fs::read_to_string(path).ok())
}

#[tauri::command(async)]
pub fn layout_cache_put(app: AppHandle, key: String, data: String) -> Result<(), String> {
    let path = cache_file(&app, &key)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, data).map_err(|e| e.to_string())
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
) -> Result<Vec<ArchiveEntryInfo>, String> {
    let guard = state.project.lock().map_err(|e| e.to_string())?;
    let project = guard.as_ref().ok_or_else(no_project)?;
    let loaded = project
        .archives
        .iter()
        .find(|a| a.info.path == path)
        .ok_or_else(|| format!("`{path}` is not an archive of this project."))?;
    if let Some(err) = &loaded.info.error {
        return Err(err.clone());
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
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(out)
}

#[tauri::command(async)]
pub fn archive_read_entry(
    state: State<'_, AppState>,
    archive: String,
    name: String,
) -> Result<tauri::ipc::Response, String> {
    let opened = {
        let guard = state.project.lock().map_err(|e| e.to_string())?;
        let project = guard.as_ref().ok_or_else(no_project)?;
        project
            .archives
            .iter()
            .find(|a| a.info.path == archive)
            .and_then(|a| a.archive.as_ref())
            .map(Arc::clone)
            .ok_or_else(|| format!("`{archive}` is not open."))?
    };
    let bytes = opened
        .read_entry(&name, renpy_core::rpa::PREVIEW_MAX)
        .map_err(|e| e.to_string())?;
    Ok(tauri::ipc::Response::new(bytes))
}

#[tauri::command]
pub fn archive_cancel(state: State<'_, AppState>) -> Result<(), String> {
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
) -> Result<u32, String> {
    let (archive_path, game_dir, root) = {
        let guard = state.project.lock().map_err(|e| e.to_string())?;
        let project = guard.as_ref().ok_or_else(no_project)?;
        let rel = archive.clone();
        let path = project
            .game_dir
            .join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
        if !project.archives.iter().any(|a| a.info.path == archive) {
            return Err(format!("`{archive}` is not an archive of this project."));
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
        let guard = state.project.lock().map_err(|e| e.to_string())?;
        ensure_game_closed(guard.as_ref().ok_or_else(no_project)?)?;
    }
    state
        .cancel
        .store(false, std::sync::atomic::Ordering::Relaxed);
    let cancel = state.cancel.clone();
    let app_progress = app.clone();
    let filter: Option<std::collections::HashSet<String>> = names.map(|n| n.into_iter().collect());
    let report = tauri::async_runtime::spawn_blocking(move || {
        let opened = renpy_core::rpa::Archive::open(&archive_path).map_err(|e| e.to_string())?;
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
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(report.files)
}

#[tauri::command]
pub async fn archive_build(
    state: State<'_, AppState>,
    src_dir: String,
    out_path: String,
) -> Result<u32, String> {
    let out = std::path::PathBuf::from(&out_path);
    let src = std::path::PathBuf::from(&src_dir);
    {
        let guard = state.project.lock().map_err(|e| e.to_string())?;
        let project = guard.as_ref().ok_or_else(no_project)?;
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
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;
    if let Ok(guard) = state.project.lock() {
        if let Some(project) = guard.as_ref() {
            if inside_dir(&project.game_dir, &out) {
                drop(guard);
                let mut guard = state.project.lock().map_err(|e| e.to_string())?;
                if let Some(project) = guard.as_mut() {
                    project.refresh_archives();
                }
            }
        }
    }
    Ok(report.files)
}

fn patch_dirs(
    app: &AppHandle,
    state: &State<'_, AppState>,
) -> Result<(std::path::PathBuf, std::path::PathBuf), String> {
    let guard = state.project.lock().map_err(|e| e.to_string())?;
    let project = guard.as_ref().ok_or_else(no_project)?;
    ensure_game_closed(project)?;
    let data = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let undo = crate::patch::undo_root(&data, &project.root);
    let backup = backup_dir(app, &project.root)?;
    Ok((undo, backup))
}

#[tauri::command]
pub async fn patch_bake(app: AppHandle) -> Result<crate::patch::PatchReport, String> {
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = app2.state::<AppState>();
        let (undo, backup) = patch_dirs(&app2, &state)?;
        let compile = {
            let guard = state.project.lock().map_err(|e| e.to_string())?;
            let project = guard.as_ref().ok_or_else(no_project)?;
            crate::patch::compile_target(project, &backup)?
        };
        if let Some((root, game_dir, launcher)) = compile {
            renpy_core::engine::run_json_dump(&root, &game_dir, &launcher, "bake".into())
                .map_err(|e| format!("Ren'Py could not compile the edit before baking: {e}"))?;
        }
        let data = app2.path().app_data_dir().map_err(|e| e.to_string())?;
        // Toggles are compiled by the engine before the lock is held for the bake.
        let (root, game_dir, launcher) = {
            let guard = state.project.lock().map_err(|e| e.to_string())?;
            let project = guard.as_ref().ok_or_else(no_project)?;
            (
                project.root.clone(),
                project.game_dir.clone(),
                project.launcher.clone(),
            )
        };
        let toggles = crate::patch::load_toggles(&data, &root);
        crate::patch::compile_toggles(&root, &game_dir, launcher.as_ref(), &toggles)?;
        let result = (|| {
            let mut guard = state.project.lock().map_err(|e| e.to_string())?;
            let project = guard.as_mut().ok_or_else(no_project)?;
            let report = crate::patch::bake(project, &undo, &backup, &toggles)?;
            project.refresh_archives();
            Ok::<_, String>(report)
        })();
        crate::patch::remove_toggles_loose(&game_dir);
        result
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn patch_undo(app: AppHandle) -> Result<crate::patch::PatchReport, String> {
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = app2.state::<AppState>();
        let (undo, backup) = patch_dirs(&app2, &state)?;
        let mut guard = state.project.lock().map_err(|e| e.to_string())?;
        let project = guard.as_mut().ok_or_else(no_project)?;
        let report = crate::patch::undo(project, &undo, &backup)?;
        project.refresh_archives();
        Ok(report)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn patch_remove(app: AppHandle) -> Result<crate::patch::PatchReport, String> {
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = app2.state::<AppState>();
        let (undo, _) = patch_dirs(&app2, &state)?;
        let mut guard = state.project.lock().map_err(|e| e.to_string())?;
        let project = guard.as_mut().ok_or_else(no_project)?;
        let report = crate::patch::remove(project, &undo)?;
        project.refresh_archives();
        Ok(report)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn mod_toggles_get(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<crate::patch::ModToggles, String> {
    let data = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let guard = state.project.lock().map_err(|e| e.to_string())?;
    let project = guard.as_ref().ok_or_else(no_project)?;
    Ok(crate::patch::load_toggles(&data, &project.root))
}

#[tauri::command]
pub fn mod_toggles_set(
    app: AppHandle,
    state: State<'_, AppState>,
    toggles: crate::patch::ModToggles,
) -> Result<(), String> {
    let data = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let guard = state.project.lock().map_err(|e| e.to_string())?;
    let project = guard.as_ref().ok_or_else(no_project)?;
    crate::patch::save_toggles(&data, &project.root, &toggles)
}

#[tauri::command]
pub async fn mod_export(
    app: AppHandle,
    dest: String,
    layout: String,
    include_toggles: bool,
    notes: String,
) -> Result<String, String> {
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = app2.state::<AppState>();
        let guard = state.project.lock().map_err(|e| e.to_string())?;
        let project = guard.as_ref().ok_or_else(no_project)?;
        crate::modexport::export(
            project,
            std::path::Path::new(&dest),
            &layout,
            include_toggles,
            &notes,
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn patch_rebase(app: AppHandle, rel: String) -> Result<crate::patch::RebaseResult, String> {
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = app2.state::<AppState>();
        let (_, backup) = patch_dirs(&app2, &state)?;
        let guard = state.project.lock().map_err(|e| e.to_string())?;
        let project = guard.as_ref().ok_or_else(no_project)?;
        crate::patch::rebase(project, &backup, &rel)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn save_list(state: State<'_, AppState>) -> Result<Vec<renpy_core::saves::SaveSlot>, String> {
    let guard = state.project.lock().map_err(|e| e.to_string())?;
    let project = guard.as_ref().ok_or_else(no_project)?;
    let game = project.game_info();
    Ok(renpy_core::saves::list_saves(
        &project.game_dir,
        game.save_directory.as_deref(),
    ))
}

#[tauri::command]
pub async fn save_inspect(app: AppHandle, path: String, deep: bool) -> Result<renpy_core::saves::SaveDetail, String> {
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = app2.state::<AppState>();
        let guard = state.project.lock().map_err(|e| e.to_string())?;
        let project = guard.as_ref().ok_or_else(no_project)?;
        let game = project.game_info();
        let roots = renpy_core::saves::save_roots(&project.game_dir, game.save_directory.as_deref());
        let file = std::path::PathBuf::from(&path);
        if !renpy_core::saves::within_roots(&file, &roots) {
            return Err("That file is not in a save folder for this game.".into());
        }
        drop(guard);
        renpy_core::saves::inspect_save(&file, deep)
    })
    .await
    .map_err(|e| e.to_string())?
}
