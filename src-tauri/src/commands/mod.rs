//! IPC commands exposed to the frontend.

use crate::error::AppError;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use renpy_core::engine::ImageRun;
use renpy_core::{DiagReport, EngineRun, LabelGraph, Project, ProjectInfo, ProjectMap};
use tauri::{AppHandle, State};

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
    /// The ty language server, when one is running.
    pub pylsp: Mutex<crate::pylsp::Proc>,
    /// Set while a ty download should stop.
    pub pylsp_cancel: Arc<AtomicBool>,
    pub pylsp_busy: AtomicBool,
    /// The launcher process for the current build, so it can be killed.
    pub build: Arc<Mutex<Option<std::process::Child>>>,
}

pub(crate) fn ensure_game_closed(project: &Project) -> Result<(), AppError> {
    let Some(launcher) = &project.launcher else {
        return Ok(());
    };
    let name = launcher
        .exe
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if crate::launch::game_running(&name) {
        return Err(
            format!("{name} is running. Close the game before changing files in game/.").into(),
        );
    }
    Ok(())
}

fn reset_editing(state: &State<'_, AppState>) -> Result<(), AppError> {
    crate::util::lock(&state.edit).pending.clear();
    crate::util::lock(&state.scene_history).clear();
    Ok(())
}

pub(crate) use crate::util::backup_dir;

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
) -> Result<ProjectInfo, AppError> {
    crate::live::stop_session(&app, std::time::Duration::from_millis(400));
    crate::pylsp::stop(&state);
    let cache = crate::util::data_dir(&app)
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
    *crate::util::lock(&state.watcher) = None;
    reset_editing(&state)?;
    *crate::util::lock(&state.project) = Some(project);
    match watch::start(app, game_dir) {
        Ok(handle) => *crate::util::lock(&state.watcher) = Some(handle),
        Err(e) => log::warn!("file watcher unavailable: {e}"),
    }
    Ok(info)
}

#[tauri::command(async)]
pub fn get_project_info(state: State<'_, AppState>) -> Result<Option<ProjectInfo>, AppError> {
    let guard = crate::util::lock(&state.project);
    Ok(guard.as_ref().map(|p| p.info()))
}

#[tauri::command(async)]
pub fn get_project_map(state: State<'_, AppState>) -> Result<ProjectMap, AppError> {
    let analysis = {
        let guard = crate::util::lock(&state.project);
        guard
            .as_ref()
            .ok_or_else(crate::util::no_project)?
            .analysis
            .clone()
    };
    Ok(analysis.map.clone())
}

#[tauri::command(async)]
pub fn get_diagnostics(state: State<'_, AppState>) -> Result<DiagReport, AppError> {
    let analysis = {
        let guard = crate::util::lock(&state.project);
        guard
            .as_ref()
            .ok_or_else(crate::util::no_project)?
            .analysis
            .clone()
    };
    Ok(analysis.diagnostics.clone())
}

#[tauri::command(async)]
pub fn get_label_graph(
    state: State<'_, AppState>,
    name: String,
    detail: bool,
) -> Result<LabelGraph, AppError> {
    let request = {
        let guard = crate::util::lock(&state.project);
        let project = guard.as_ref().ok_or_else(crate::util::no_project)?;
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
) -> Result<renpy_core::flow::LabelLines, AppError> {
    crate::util::with_project(&state, |project| {
        project
            .label_lines(&name)
            .ok_or_else(|| crate::error::AppError::new(format!("Label `{name}` not found.")))
    })
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
pub fn edit_status(app: AppHandle, state: State<'_, AppState>) -> Result<EditStatus, AppError> {
    crate::util::with_project(&state, |project| {
        let root = backup_dir(&app, &project.root)?;
        Ok(EditStatus {
            modified: edit::modified_files(project, &root),
            decompiled: edit::decompiled_edits(project, &root),
        })
    })
}

#[tauri::command(async)]
pub fn write_file(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    text: String,
) -> Result<EditImpact, AppError> {
    // Project lock first, then the edit lock: the watcher takes them in this order.
    // Analysis runs after both locks drop so a label-graph read is not blocked.
    let (before, job) = {
        let mut guard = crate::util::lock(&state.project);
        let project = guard.as_mut().ok_or_else(crate::util::no_project)?;
        ensure_game_closed(project)?;
        let mut edit = crate::util::lock(&state.edit);
        let root = backup_dir(&app, &project.root)?;
        let Some(before) = edit::prepare_write(project, &mut edit, &root, &path, &text)? else {
            return Ok(edit::EditImpact::default());
        };
        (before, project.fork_analysis())
    };
    let analysis = job.run();
    let mut guard = crate::util::lock(&state.project);
    let project = guard.as_mut().ok_or_else(crate::util::no_project)?;
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
) -> Result<EditImpact, AppError> {
    crate::util::with_project_mut(&state, |project| {
        ensure_game_closed(project)?;
        let mut edit = crate::util::lock(&state.edit);
        let root = backup_dir(&app, &project.root)?;
        edit::apply_revert(project, &mut edit, &root, &path)
    })
}

/// Syntax of an unsaved buffer. Does not re-run project analysis.
#[tauri::command(async)]
pub fn check_syntax(text: String) -> Vec<renpy_core::SyntaxIssue> {
    renpy_core::parser::check_syntax(&text)
}

/// Raw bytes of a project script (path relative to `game/`), sent as an ArrayBuffer.
#[tauri::command(async)]
pub fn read_file(
    state: State<'_, AppState>,
    path: String,
) -> Result<tauri::ipc::Response, AppError> {
    enum Loaded {
        Bytes(Vec<u8>),
        Loose(PathBuf),
    }
    let loaded = {
        let guard = crate::util::lock(&state.project);
        let project = guard.as_ref().ok_or_else(crate::util::no_project)?;
        let Some(idx) = project.file_index(&path) else {
            return Err(format!("`{path}` is not a script of this project.").into());
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
) -> Result<LaunchReport, AppError> {
    crate::live::stop_session(&app, std::time::Duration::from_millis(800));
    let source = {
        let guard = crate::util::lock(&state.project);
        let project = guard.as_ref().ok_or_else(crate::util::no_project)?;
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
) -> Result<LaunchReport, AppError> {
    crate::live::stop_session(&app, std::time::Duration::from_millis(800));
    let (source, warp, label) = {
        let guard = crate::util::lock(&state.project);
        let project = guard.as_ref().ok_or_else(crate::util::no_project)?;
        if project.file_index(&file).is_none() {
            return Err(format!("`{file}` is not a script of this project.").into());
        }
        let label = crate::live::enclosing_label(project, &file, line);
        let (file, line) = project.engine_spec(&file, line);
        (launch::LaunchSource::capture(project), (file, line), label)
    };
    let extra = launch::LaunchExtra {
        warp_label: label,
        ..Default::default()
    };
    launch::launch(&source, launcher, Some(warp), extra)
}

fn load_cached_engine(app: &AppHandle, project: &Project) -> Option<EngineRun> {
    let path = crate::util::engine_cache_path(app, &project.root).ok()?;
    let text = std::fs::read_to_string(path).ok()?;
    let mut run: EngineRun = serde_json::from_str(&text).ok()?;
    if run.key != project.source_key() {
        return None;
    }
    run.from_cache = true;
    Some(run)
}

fn load_cached_images(app: &AppHandle, project: &Project) -> Option<ImageRun> {
    let path = crate::util::image_cache_path(app, &project.root).ok()?;
    let text = std::fs::read_to_string(path).ok()?;
    let mut run: ImageRun = serde_json::from_str(&text).ok()?;
    // Kept even when the scripts have changed since. A newer `image` statement
    // wins for its own name until a fresh dump replaces this one.
    run.from_cache = true;
    Some(run)
}

pub(crate) fn save_cached_images(app: &AppHandle, root: &std::path::Path, run: &ImageRun) {
    if let Ok(path) = crate::util::image_cache_path(app, root) {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(text) = serde_json::to_string(run) {
            let _ = std::fs::write(path, text);
        }
    }
}

fn save_cached_engine(app: &AppHandle, root: &std::path::Path, run: &EngineRun) {
    if let Ok(path) = crate::util::engine_cache_path(app, root) {
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
) -> Result<renpy_core::EngineTarget, AppError> {
    crate::util::with_project(state, |project| {
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
            None => project.engine_target().map_err(AppError::from),
        }
    })
}

/// Run the game's bundled engine with `--json-dump` and merge the result.
#[tauri::command]
pub async fn engine_dump(
    app: AppHandle,
    state: State<'_, AppState>,
    launcher: Option<String>,
) -> Result<ProjectInfo, AppError> {
    let target = engine_target(&state, launcher)?;
    let t = target.clone();
    let run = tauri::async_runtime::spawn_blocking(move || {
        renpy_core::engine::run_json_dump(&t.root, &t.game_dir, &t.launcher, t.key.clone())
    })
    .await??;
    save_cached_engine(&app, &target.root, &run);
    crate::util::with_project_mut(&state, |project| {
        if project.root != target.root {
            return Err("The project was switched while the engine was running.".into());
        }
        project.set_engine(run);
        Ok(project.info())
    })
}

/// Run the game's engine through init and read the images it registered.
#[tauri::command]
pub async fn stage_images(
    app: AppHandle,
    state: State<'_, AppState>,
    launcher: Option<String>,
) -> Result<ProjectInfo, AppError> {
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
        let guard = crate::util::lock(&state.project);
        guard
            .as_ref()
            .ok_or_else(crate::util::no_project)?
            .init_key()
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
    .await??;
    save_cached_images(&app, &target.root, &run);
    let mut guard = crate::util::lock(&state.project);
    let project = guard.as_mut().ok_or_else(crate::util::no_project)?;
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
) -> Result<ProjectInfo, AppError> {
    let target = engine_target(&state, launcher)?;
    let t = target.clone();
    let (items, notes) = tauri::async_runtime::spawn_blocking(move || {
        renpy_core::engine::run_lint(&t.root, &t.game_dir, &t.launcher)
    })
    .await??;
    crate::util::with_project_mut(&state, |project| {
        if project.root != target.root {
            return Err("The project was switched while the engine was running.".into());
        }
        let _ = notes;
        project.set_lint(items);
        Ok(project.info())
    })
}

mod archives;
mod patching;
mod saves;

pub use archives::*;
pub use patching::*;
pub use saves::*;
