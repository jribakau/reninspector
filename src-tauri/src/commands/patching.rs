//! IPC commands exposed to the frontend.

use crate::error::AppError;

use tauri::{AppHandle, Manager, State};

use super::*;

fn patch_dirs(
    app: &AppHandle,
    state: &State<'_, AppState>,
) -> Result<(std::path::PathBuf, std::path::PathBuf), AppError> {
    crate::util::with_project(state, |project| {
        ensure_game_closed(project)?;
        let data = crate::util::data_dir(app)?;
        let undo = crate::patch::undo_root(&data, &project.root);
        let backup = backup_dir(app, &project.root)?;
        Ok((undo, backup))
    })
}

#[tauri::command]
pub async fn patch_bake(app: AppHandle) -> Result<crate::patch::PatchReport, AppError> {
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = app2.state::<AppState>();
        let (undo, backup) = patch_dirs(&app2, &state)?;
        let compile = {
            let guard = crate::util::lock(&state.project);
            let project = guard.as_ref().ok_or_else(crate::util::no_project)?;
            crate::patch::compile_target(project, &backup)?
        };
        if let Some((root, game_dir, launcher)) = compile {
            renpy_core::engine::run_json_dump(&root, &game_dir, &launcher, "bake".into())
                .map_err(|e| format!("Ren'Py could not compile the edit before baking: {e}"))?;
        }
        let data = crate::util::data_dir(&app2)?;
        // Toggles are compiled by the engine before the lock is held for the bake.
        let (root, game_dir, launcher) = {
            let guard = crate::util::lock(&state.project);
            let project = guard.as_ref().ok_or_else(crate::util::no_project)?;
            (
                project.root.clone(),
                project.game_dir.clone(),
                project.launcher.clone(),
            )
        };
        let toggles = crate::patch::load_toggles(&data, &root);
        crate::patch::compile_toggles(&root, &game_dir, launcher.as_ref(), &toggles)?;
        let result = (|| {
            let mut guard = crate::util::lock(&state.project);
            let project = guard.as_mut().ok_or_else(crate::util::no_project)?;
            let report = crate::patch::bake(project, &undo, &backup, &toggles)?;
            project.refresh_archives();
            Ok::<_, AppError>(report)
        })();
        crate::patch::remove_toggles_loose(&game_dir);
        result
    })
    .await?
}

#[tauri::command]
pub async fn patch_undo(app: AppHandle) -> Result<crate::patch::PatchReport, AppError> {
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = app2.state::<AppState>();
        let (undo, backup) = patch_dirs(&app2, &state)?;
        let mut guard = crate::util::lock(&state.project);
        let project = guard.as_mut().ok_or_else(crate::util::no_project)?;
        let report = crate::patch::undo(project, &undo, &backup)?;
        project.refresh_archives();
        Ok(report)
    })
    .await?
}

#[tauri::command]
pub async fn patch_remove(app: AppHandle) -> Result<crate::patch::PatchReport, AppError> {
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = app2.state::<AppState>();
        let (undo, _) = patch_dirs(&app2, &state)?;
        let mut guard = crate::util::lock(&state.project);
        let project = guard.as_mut().ok_or_else(crate::util::no_project)?;
        let report = crate::patch::remove(project, &undo)?;
        project.refresh_archives();
        Ok(report)
    })
    .await?
}

#[tauri::command(async)]
pub fn mod_toggles_get(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<crate::patch::ModToggles, AppError> {
    let data = crate::util::data_dir(&app)?;
    crate::util::with_project(&state, |project| {
        Ok(crate::patch::load_toggles(&data, &project.root))
    })
}

#[tauri::command(async)]
pub fn mod_toggles_set(
    app: AppHandle,
    state: State<'_, AppState>,
    toggles: crate::patch::ModToggles,
) -> Result<(), AppError> {
    let data = crate::util::data_dir(&app)?;
    crate::util::with_project(&state, |project| {
        crate::patch::save_toggles(&data, &project.root, &toggles)
    })
}

#[tauri::command]
pub async fn mod_export(
    app: AppHandle,
    dest: String,
    layout: String,
    include_toggles: bool,
    notes: String,
) -> Result<String, AppError> {
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = app2.state::<AppState>();
        let job = {
            let guard = crate::util::lock(&state.project);
            let project = guard.as_ref().ok_or_else(crate::util::no_project)?;
            crate::modexport::prepare(project)?
        };
        crate::modexport::export(
            &job,
            std::path::Path::new(&dest),
            &layout,
            include_toggles,
            &notes,
        )
    })
    .await?
}

#[tauri::command]
pub async fn patch_rebase(
    app: AppHandle,
    rel: String,
) -> Result<crate::patch::RebaseResult, AppError> {
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = app2.state::<AppState>();
        let (sources, backup, exe_name) = {
            let guard = crate::util::lock(&state.project);
            let project = guard.as_ref().ok_or_else(crate::util::no_project)?;
            let exe_name = project
                .launcher
                .as_ref()
                .and_then(|launcher| launcher.exe.file_name())
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            let backup = backup_dir(&app2, &project.root)?;
            let sources = crate::patch::rebase_sources(project, &rel)?;
            (sources, backup, exe_name)
        };
        if crate::launch::game_running(&exe_name) {
            return Err(format!(
                "{exe_name} is running. Close the game before changing files in game/."
            )
            .into());
        }
        crate::patch::rebase_with(&sources, &backup, &rel)
    })
    .await?
}
