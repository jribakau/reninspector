//! Search, file operations, symbols, assets, translations, logs and project lifecycle.

use crate::error::AppError;
use std::fs;
use std::path::{Path, PathBuf};

use renpy_core::scene::{self};
use tauri::{AppHandle, State};

use crate::commands::AppState;

const SAMPLE_SCRIPT: &str = include_str!("../../sample/game/script.rpy");

/// Copies the bundled demo into the app data folder the first time, then
/// returns that folder. Later calls keep whatever the user changed.
#[tauri::command(async)]
pub fn open_sample(app: AppHandle) -> Result<String, AppError> {
    let dir = crate::util::data_dir(&app)?.join("sample");
    let script = dir.join("game").join("script.rpy");
    if !script.is_file() {
        if let Some(parent) = script.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("Could not create the demo: {e}"))?;
        }
        fs::write(&script, SAMPLE_SCRIPT).map_err(|e| format!("Could not write the demo: {e}"))?;
    }
    Ok(dir.to_string_lossy().into_owned())
}

/// With `sdk` (its `renpy.exe` or `renpy.sh`) the project is made the way the
/// Ren'Py launcher makes one, GUI included. Without it, only a script and options.
#[tauri::command(async)]
pub fn create_project(
    app: AppHandle,
    state: State<'_, AppState>,
    parent: String,
    name: String,
    scene: Option<String>,
    sdk: Option<String>,
) -> Result<String, AppError> {
    let name = name.trim().to_string();
    if name.is_empty() || name.contains(['/', '\\', ':', '.']) || name.contains('\n') {
        return Err("Use a game name without slashes, colons, or dots.".into());
    }
    let scene_name = scene.unwrap_or_else(|| "start".into()).trim().to_string();
    scene::check_label(&scene_name)?;
    let root = PathBuf::from(&parent).join(&name);
    if root.exists() {
        return Err(format!("{} already exists.", root.display()).into());
    }
    let game = root.join("game");
    fs::create_dir_all(&game).map_err(|e| format!("Could not create the project: {e}"))?;
    let made = match sdk.filter(|s| !s.trim().is_empty()) {
        Some(exe) => crate::sdk::generate_from_sdk(
            Some(&app),
            Some(state.build.clone()),
            Some(state.build_cancel.clone()),
            Path::new(&exe),
            &root,
        ),
        None => fs::write(game.join("options.rpy"), minimal_options(&name))
            .map_err(crate::error::AppError::from),
    };
    let written = made.and_then(|_| {
        fs::write(game.join("script.rpy"), first_script(&scene_name))
            .map_err(crate::error::AppError::from)
    });
    if let Err(e) = written {
        let _ = fs::remove_dir_all(&root);
        return Err(e);
    }
    Ok(root.to_string_lossy().into_owned())
}

/// Ren'Py always begins at `label start`.
pub(crate) fn first_script(scene_name: &str) -> String {
    let scene = format!("label {scene_name}:\n    \"…\"\n    return\n");
    if scene_name == "start" {
        scene
    } else {
        format!("label start:\n    jump {scene_name}\n\n{scene}")
    }
}

fn minimal_options(name: &str) -> String {
    format!(
        "define config.name = _({})\ndefine config.version = \"0.1\"\ndefine config.has_sound = True\ndefine config.has_music = True\n",
        scene::renpy_string(name)
    )
}
