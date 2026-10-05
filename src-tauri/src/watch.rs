//! Watches `game/**/*.rpy`, re-parses changed files and tells the UI.

use crate::error::AppError;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use renpy_core::project::is_project_script;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::commands::AppState;

pub const CHANGED_EVENT: &str = "project:changed";
const COALESCE: Duration = Duration::from_millis(200);

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ChangedPayload {
    /// Paths relative to `game/`.
    paths: Vec<String>,
}

/// Dropping the handle stops the watcher (and, through the closed channel, its worker thread).
pub struct WatchHandle {
    _watcher: RecommendedWatcher,
}

pub fn start(app: AppHandle, game_dir: PathBuf) -> Result<WatchHandle, AppError> {
    let (tx, rx) = mpsc::channel::<PathBuf>();
    let filter_dir = game_dir.clone();
    let mut watcher =
        notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| {
            let Ok(event) = res else { return };
            if !matches!(
                event.kind,
                EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
            ) {
                return;
            }
            for p in event.paths {
                if is_project_script(&filter_dir, &p) || renpy_core::project::is_archive_path(&p) {
                    let _ = tx.send(p);
                }
            }
        })?;
    watcher.watch(&game_dir, RecursiveMode::Recursive)?;

    std::thread::spawn(move || {
        while let Ok(first) = rx.recv() {
            let mut batch: HashSet<PathBuf> = HashSet::new();
            batch.insert(first);
            while let Ok(next) = rx.recv_timeout(COALESCE) {
                batch.insert(next);
            }
            let paths: Vec<PathBuf> = batch.into_iter().collect();
            let sniffed: Vec<(PathBuf, Vec<u8>)> = paths
                .iter()
                .map(|p| {
                    // Archives can be gigabytes and are never written by a script save.
                    let bytes = if renpy_core::project::is_archive_path(p) {
                        Vec::new()
                    } else {
                        std::fs::read(p).unwrap_or_default()
                    };
                    (p.clone(), bytes)
                })
                .collect();
            let state = app.state::<AppState>();
            let ready = {
                let Ok(mut guard) = state.project.lock() else {
                    continue;
                };
                let Ok(mut edit) = state.edit.lock() else {
                    continue;
                };
                let Some(project) = guard.as_mut() else {
                    continue;
                };
                // Drop events caused by our own saves so the editor keeps its buffer.
                let external: Vec<PathBuf> = sniffed
                    .iter()
                    .filter(|(p, bytes)| !crate::edit::is_our_write(&mut edit, p, bytes))
                    .map(|(p, _)| p.clone())
                    .collect();
                if external.is_empty() || !project.ingest_paths(&external) {
                    continue;
                }
                let rels = external
                    .iter()
                    .filter_map(|p| p.strip_prefix(&project.game_dir).ok())
                    .map(|p| p.to_string_lossy().replace('\\', "/"))
                    .collect::<Vec<_>>();
                Some((rels, project.fork_analysis()))
            };
            let Some((rels, job)) = ready else { continue };
            let analysis = job.run();
            if let Ok(mut guard) = state.project.lock() {
                if let Some(project) = guard.as_mut() {
                    project.publish_analysis(job.epoch, analysis);
                }
            }
            let _ = app.emit(CHANGED_EVENT, ChangedPayload { paths: rels });
        }
    });

    Ok(WatchHandle { _watcher: watcher })
}
