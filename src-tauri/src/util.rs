//! Shared lock, project and app-data helpers for command handlers.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager};

use crate::commands::AppState;
use crate::error::AppError;
use renpy_core::Project;

pub fn no_project() -> AppError {
    AppError::new("No project is open.")
}

/// Recover from a poisoned lock. One panicked command should not freeze the app.
pub fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub fn with_project<T>(
    state: &AppState,
    f: impl FnOnce(&Project) -> Result<T, AppError>,
) -> Result<T, AppError> {
    let guard = lock(&state.project);
    let project = guard.as_ref().ok_or_else(crate::util::no_project)?;
    f(project)
}

pub fn with_project_mut<T>(
    state: &AppState,
    f: impl FnOnce(&mut Project) -> Result<T, AppError>,
) -> Result<T, AppError> {
    let mut guard = lock(&state.project);
    let project = guard.as_mut().ok_or_else(crate::util::no_project)?;
    f(project)
}

pub fn data_dir(app: &AppHandle) -> Result<PathBuf, AppError> {
    Ok(app.path().app_data_dir()?)
}

pub fn backup_dir(app: &AppHandle, root: &Path) -> Result<PathBuf, AppError> {
    Ok(crate::edit::backup_root(&data_dir(app)?, root))
}

pub fn cache_name(key: &str) -> String {
    let dig = Sha256::digest(key.as_bytes());
    dig.iter().take(8).map(|b| format!("{b:02x}")).collect()
}

fn cache_file(app: &AppHandle, folder: &str, file_name: &str) -> Result<PathBuf, AppError> {
    Ok(data_dir(app)?.join(folder).join(file_name))
}

pub fn engine_cache_path(app: &AppHandle, root: &Path) -> Result<PathBuf, AppError> {
    cache_file(
        app,
        "engine",
        &format!(
            "{}.json",
            cache_name(&root.to_string_lossy().to_lowercase())
        ),
    )
}

pub fn image_cache_path(app: &AppHandle, root: &Path) -> Result<PathBuf, AppError> {
    cache_file(
        app,
        "stage-images",
        &format!(
            "{}.json",
            cache_name(&root.to_string_lossy().to_lowercase())
        ),
    )
}

pub fn layout_cache_path(app: &AppHandle, key: &str) -> Result<PathBuf, AppError> {
    cache_file(app, "layouts", &format!("{}.json", cache_name(key)))
}

pub fn fingerprint_cache_path(app: &AppHandle, root: &Path) -> Result<PathBuf, AppError> {
    cache_file(
        app,
        "layouts",
        &format!(
            "fp-{}.json",
            cache_name(&root.to_string_lossy().to_lowercase())
        ),
    )
}
