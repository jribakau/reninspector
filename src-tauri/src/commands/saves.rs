//! IPC commands exposed to the frontend.

use crate::error::AppError;

use tauri::{AppHandle, Manager, State};

use super::*;

#[tauri::command(async)]
pub fn save_list(state: State<'_, AppState>) -> Result<Vec<renpy_core::saves::SaveSlot>, AppError> {
    crate::util::with_project(&state, |project| {
        let game = project.game_info();
        Ok(renpy_core::saves::list_saves(
            &project.game_dir,
            game.save_directory.as_deref(),
        ))
    })
}

#[tauri::command]
pub async fn save_inspect(
    app: AppHandle,
    path: String,
    deep: bool,
) -> Result<renpy_core::saves::SaveDetail, AppError> {
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = app2.state::<AppState>();
        let guard = crate::util::lock(&state.project);
        let project = guard.as_ref().ok_or_else(crate::util::no_project)?;
        let game = project.game_info();
        let roots =
            renpy_core::saves::save_roots(&project.game_dir, game.save_directory.as_deref());
        let file = std::path::PathBuf::from(&path);
        if !renpy_core::saves::within_roots(&file, &roots) {
            return Err("That file is not in a save folder for this game.".into());
        }
        drop(guard);
        renpy_core::saves::inspect_save(&file, deep).map_err(AppError::from)
    })
    .await?
}
