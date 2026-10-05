//! Search, file operations, symbols, assets, translations, logs and project lifecycle.

use crate::error::AppError;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use renpy_core::scene::{self, LinkKind, Place, SceneOp, StmtSpec};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

use crate::commands::{self, AppState};
use crate::edit::{self, EditImpact};

use super::*;

pub(crate) const AUTORELOAD: &str = "\
# Written by Ren'Inspector. While this file is here, a launch reloads scripts after they change.\n\
init 999 python:\n    \
    config.autoreload = True\n";

pub(crate) const AUTORELOAD_FILE: &str = "vnide_autoreload.rpy";

#[tauri::command]
pub fn autoreload_enabled(state: State<'_, AppState>) -> Result<bool, AppError> {
    crate::util::with_project(&state, |project| {
        Ok(project.game_dir.join(AUTORELOAD_FILE).is_file())
    })
}

#[tauri::command]
pub fn set_autoreload(state: State<'_, AppState>, enabled: bool) -> Result<bool, AppError> {
    crate::util::with_project(&state, |project| {
        let path = project.game_dir.join(AUTORELOAD_FILE);
        if enabled {
            fs::write(&path, AUTORELOAD)
                .map_err(|e| format!("Could not turn on auto-reload: {e}"))?;
        } else {
            let _ = fs::remove_file(&path);
            let _ = fs::remove_file(path.with_extension("rpyc"));
        }
        Ok(enabled)
    })
}

/// Keeps `vnide_autoreload.rpy` out of `game/` while a build runs, so a
/// distribution never ships it, and writes it back when dropped.
pub struct AutoreloadAside {
    restore: Option<PathBuf>,
}

impl AutoreloadAside {
    pub fn take(game_dir: &Path) -> Result<Self, AppError> {
        let path = game_dir.join(AUTORELOAD_FILE);
        // An orphaned .rpyc still loads, so it goes even when the .rpy is already gone.
        let _ = fs::remove_file(path.with_extension("rpyc"));
        if !path.is_file() {
            return Ok(Self { restore: None });
        }
        fs::remove_file(&path).map_err(|e| {
            format!("Could not take {AUTORELOAD_FILE} out of game/ for the build: {e}")
        })?;
        Ok(Self {
            restore: Some(path),
        })
    }
}

impl Drop for AutoreloadAside {
    fn drop(&mut self) {
        if let Some(path) = &self.restore {
            let _ = fs::write(path, AUTORELOAD);
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneChange {
    pub op: String,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub line: u32,
    #[serde(default)]
    pub speaker: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub target: String,
    #[serde(default)]
    pub target_line: u32,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub color: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub new_label: bool,
    /// `before`, `after`, or `into`, for `add-stmt`.
    #[serde(default)]
    pub place: String,
    /// -1 or 1, for `move-stmt`.
    #[serde(default)]
    pub dir: i32,
    /// The `if` condition for `set-choice-cond`.
    #[serde(default)]
    pub cond: String,
    /// `jump`, `call`, `choice`, or `return`, for `link-scene`.
    #[serde(default)]
    pub how: String,
    #[serde(default)]
    pub spec: Option<StmtSpec>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneReport {
    pub path: String,
    pub impact: EditImpact,
    /// 1-based line that was inserted, moved, or edited. 0 when there is none.
    pub focus_line: u32,
    pub can_undo: bool,
    pub can_redo: bool,
}

/// One scene edit that can be taken back: the file's text before and after.
struct SceneStep {
    rel: String,
    before: String,
    after: String,
}

/// Undo and redo for flow edits. Each step holds whole-file text, so a step is
/// refused when the file has changed since.
#[derive(Default)]
pub struct SceneHistory {
    undo: Vec<SceneStep>,
    redo: Vec<SceneStep>,
}

const SCENE_HISTORY_LIMIT: usize = 100;

impl SceneHistory {
    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }

    fn record(&mut self, step: SceneStep) {
        self.redo.clear();
        self.undo.push(step);
        if self.undo.len() > SCENE_HISTORY_LIMIT {
            self.undo.remove(0);
        }
    }

    fn forget(&mut self, rel: &str) {
        self.undo.retain(|s| s.rel != rel);
        self.redo.retain(|s| s.rel != rel);
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneHistoryState {
    pub can_undo: bool,
    pub can_redo: bool,
}

fn history_state(state: &State<'_, AppState>) -> SceneHistoryState {
    match state.scene_history.lock() {
        Ok(h) => SceneHistoryState {
            can_undo: !h.undo.is_empty(),
            can_redo: !h.redo.is_empty(),
        },
        Err(_) => SceneHistoryState {
            can_undo: false,
            can_redo: false,
        },
    }
}

/// Read one staging, jump, call, or dialogue line back into the fields the flow editor shows.
#[tauri::command]
pub fn scene_parse(code: String) -> Option<StmtSpec> {
    scene::parse_stmt(&code)
}

#[tauri::command]
pub fn scene_history(state: State<'_, AppState>) -> SceneHistoryState {
    history_state(&state)
}

/// 1-based first line where two texts differ.
fn first_difference(a: &str, b: &str) -> u32 {
    let mut n = 0u32;
    for (x, y) in a.split('\n').zip(b.split('\n')) {
        if x != y {
            break;
        }
        n += 1;
    }
    n + 1
}

fn scene_step(
    app: AppHandle,
    state: State<'_, AppState>,
    undo: bool,
) -> Result<SceneReport, AppError> {
    crate::util::with_project_mut(&state, |project| {
        commands::ensure_game_closed(project)?;
        let mut edit = crate::util::lock(&state.edit);
        let step = {
            let mut h = crate::util::lock(&state.scene_history);
            let stack = if undo { &mut h.undo } else { &mut h.redo };
            stack.pop().ok_or_else(|| {
                if undo {
                    "Nothing to undo.".to_string()
                } else {
                    "Nothing to redo.".to_string()
                }
            })?
        };
        let stale = |step: &SceneStep| {
            if let Ok(mut h) = state.scene_history.lock() {
                h.forget(&step.rel);
            }
            format!(
                "{} changed since that edit. Use the code view to change it.",
                step.rel
            )
        };
        let Ok(current) = script_text(project, &step.rel) else {
            return Err(stale(&step).into());
        };
        let (expect, target) = if undo {
            (&step.after, &step.before)
        } else {
            (&step.before, &step.after)
        };
        if &current != expect {
            return Err(stale(&step).into());
        }
        let root = commands::backup_dir(&app, &project.root)?;
        let impact = match edit::apply_write(project, &mut edit, &root, &step.rel, target) {
            Ok(impact) => impact,
            Err(e) => {
                if let Ok(mut h) = state.scene_history.lock() {
                    if undo {
                        h.undo.push(step);
                    } else {
                        h.redo.push(step);
                    }
                }
                return Err(e);
            }
        };
        let focus_line = first_difference(&current, target);
        let path = step.rel.clone();
        if let Ok(mut h) = state.scene_history.lock() {
            if undo {
                h.redo.push(step);
            } else {
                h.undo.push(step);
            }
        }
        let flags = history_state(&state);
        Ok(SceneReport {
            path,
            impact,
            focus_line,
            can_undo: flags.can_undo,
            can_redo: flags.can_redo,
        })
    })
}

#[tauri::command(async)]
pub fn scene_undo(app: AppHandle, state: State<'_, AppState>) -> Result<SceneReport, AppError> {
    scene_step(app, state, true)
}

#[tauri::command(async)]
pub fn scene_redo(app: AppHandle, state: State<'_, AppState>) -> Result<SceneReport, AppError> {
    scene_step(app, state, false)
}

fn script_text(project: &renpy_core::Project, rel: &str) -> Result<String, AppError> {
    let bytes = project.read_script_bytes(rel)?;
    let text =
        String::from_utf8(bytes).map_err(|_| format!("`{rel}` is not UTF-8. Edit it in code."))?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    Ok(text.replace("\r\n", "\n"))
}

fn label_defined(project: &renpy_core::Project, name: &str) -> bool {
    project
        .analysis
        .map
        .nodes
        .iter()
        .any(|n| n.id == name && n.kind != "missing")
}

fn prefer_script(project: &renpy_core::Project, preferred: &str) -> String {
    if project.file_index(preferred).is_some() {
        return preferred.to_string();
    }
    if project.file_index("script.rpy").is_some() {
        return "script.rpy".into();
    }
    project
        .files
        .iter()
        .find(|f| f.rel.ends_with(".rpy") && f.origin.editable())
        .map(|f| f.rel.clone())
        .unwrap_or_else(|| preferred.to_string())
}

fn import_image(game_dir: &Path, source: &str) -> Result<String, AppError> {
    let source = PathBuf::from(source.trim());
    if source.as_os_str().is_empty() {
        return Err("Choose an image file.".into());
    }
    let source = source
        .canonicalize()
        .map_err(|_| "That image file could not be read.".to_string())?;
    if !source.is_file() {
        return Err("Choose an image file.".into());
    }
    let ext = source
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if !matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "webp" | "gif") {
        return Err("Use a png, jpeg, webp, or gif.".into());
    }
    if let Ok(game) = game_dir.canonicalize() {
        if let Ok(rel) = source.strip_prefix(&game) {
            return Ok(rel.to_string_lossy().replace('\\', "/"));
        }
    }
    let images = game_dir.join("images");
    fs::create_dir_all(&images).map_err(|e| format!("Could not create images/: {e}"))?;
    let stem = source
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("image")
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>();
    let stem = if stem.is_empty() {
        "image".into()
    } else {
        stem
    };
    let mut dest = images.join(format!("{stem}.{ext}"));
    let mut n = 2u32;
    while dest.exists() {
        dest = images.join(format!("{stem}-{n}.{ext}"));
        n += 1;
        if n > 500 {
            return Err("Too many images with that name.".into());
        }
    }
    fs::copy(&source, &dest).map_err(|e| format!("Could not copy the image: {e}"))?;
    Ok(format!(
        "images/{}",
        dest.file_name().unwrap_or_default().to_string_lossy()
    ))
}

fn scene_ops(
    project: &renpy_core::Project,
    change: &SceneChange,
    image_file: &str,
) -> Result<Vec<SceneOp>, AppError> {
    let new_name = if change.new_label {
        change.target.trim()
    } else {
        ""
    };
    if !new_name.is_empty() {
        scene::check_label(new_name)?;
        if label_defined(project, new_name) {
            return Err(format!(
                "`{new_name}` is already a scene. Jump to it instead of creating it."
            )
            .into());
        }
    }
    if change.op == "add-label" {
        scene::check_label(change.name.trim())?;
        if label_defined(project, change.name.trim()) {
            return Err(format!("`{}` is already a scene.", change.name.trim()).into());
        }
    }
    let mut ops = match change.op.as_str() {
        "set-say" => vec![SceneOp::SetSay {
            line: change.line,
            speaker: change.speaker.trim().to_string(),
            text: change.text.clone(),
        }],
        "set-choice" => vec![SceneOp::SetChoice {
            line: change.line,
            text: change.text.clone(),
            target: change.target.trim().to_string(),
            target_line: change.target_line,
        }],
        "set-jump" => vec![SceneOp::SetJump {
            line: change.line,
            target: change.target.trim().to_string(),
        }],
        "add-say" => vec![SceneOp::AddSay {
            after: change.line,
            speaker: change.speaker.trim().to_string(),
            text: change.text.clone(),
        }],
        "add-choice" => vec![SceneOp::AddChoice {
            anchor: change.line,
            text: change.text.clone(),
            target: change.target.trim().to_string(),
        }],
        "add-stmt" => vec![SceneOp::AddStmt {
            anchor: change.line,
            place: match change.place.as_str() {
                "before" => Place::Before,
                "into" => Place::Into,
                _ => Place::After,
            },
            spec: change.spec.clone().ok_or("Choose what to add.")?,
        }],
        "set-stmt" => vec![SceneOp::SetStmt {
            line: change.line,
            spec: change.spec.clone().ok_or("Choose what to write.")?,
        }],
        "delete-stmt" => vec![SceneOp::DeleteStmt { line: change.line }],
        "move-stmt" => vec![SceneOp::MoveStmt {
            line: change.line,
            dir: change.dir,
        }],
        "duplicate-stmt" => vec![SceneOp::DuplicateStmt { line: change.line }],
        "set-choice-cond" => vec![SceneOp::SetChoiceCond {
            line: change.line,
            cond: change.cond.clone(),
        }],
        "link-scene" => vec![SceneOp::LinkScene {
            label_line: change.line,
            how: match change.how.as_str() {
                "jump" => LinkKind::Jump,
                "call" => LinkKind::Call,
                "choice" => LinkKind::Choice,
                "return" => LinkKind::Return,
                _ => return Err("Choose how the scenes connect.".into()),
            },
            target: change.target.trim().to_string(),
            caption: change.text.clone(),
        }],
        "add-label" => vec![SceneOp::AddLabel {
            name: change.name.trim().to_string(),
        }],
        "add-character" => {
            let var = change.name.trim();
            if project
                .analysis
                .catalog
                .symbols
                .iter()
                .any(|s| s.kind == "character" && s.name == var)
            {
                return Err(format!("`{var}` is already a character.").into());
            }
            vec![SceneOp::AddCharacter {
                var: var.to_string(),
                who: change.text.clone(),
                color: change.color.trim().to_string(),
            }]
        }
        "add-image" => {
            let name = change.name.trim();
            if project
                .analysis
                .catalog
                .symbols
                .iter()
                .any(|s| s.kind == "image" && s.name == name)
            {
                return Err(format!("`{name}` is already an image.").into());
            }
            vec![SceneOp::AddImage {
                name: name.to_string(),
                file: image_file.to_string(),
            }]
        }
        _ => return Err("That scene edit is not supported.".into()),
    };
    if !new_name.is_empty() {
        ops.push(SceneOp::AddLabel {
            name: new_name.to_string(),
        });
    }
    Ok(ops)
}

#[tauri::command(async)]
pub fn stage_at(
    state: State<'_, AppState>,
    file: String,
    line: u32,
    vars: Option<HashMap<String, String>>,
) -> Result<renpy_core::stage::StageEstimate, AppError> {
    crate::util::with_project(&state, |project| {
        let pins: Vec<(String, String)> = vars.unwrap_or_default().into_iter().collect();
        renpy_core::stage::estimate(project, &file, line, &pins).map_err(AppError::from)
    })
}

#[tauri::command(async)]
pub fn scene_edit(
    app: AppHandle,
    state: State<'_, AppState>,
    change: SceneChange,
) -> Result<SceneReport, AppError> {
    crate::util::with_project_mut(&state, |project| {
        commands::ensure_game_closed(project)?;
        let mut edit = crate::util::lock(&state.edit);
        if change.op == "add-image" {
            let name = change.name.trim();
            scene::check_image_name(name)?;
            if project
                .analysis
                .catalog
                .symbols
                .iter()
                .any(|s| s.kind == "image" && s.name == name)
            {
                return Err(format!("`{name}` is already an image.").into());
            }
        }
        let image_file = if change.op == "add-image" {
            import_image(&project.game_dir, &change.source)?
        } else {
            String::new()
        };
        let rel = if change.path.trim().is_empty() {
            match change.op.as_str() {
                "add-character" => prefer_script(project, "characters.rpy"),
                "add-image" => prefer_script(project, "images.rpy"),
                _ => return Err("Open a scene before editing it.".into()),
            }
        } else {
            script_rel(&change.path)?
        };
        let ops = scene_ops(project, &change, &image_file)?;
        if project.file_index(&rel).is_none() {
            if !matches!(change.op.as_str(), "add-character" | "add-image") {
                return Err(format!("`{rel}` is not a script of this project.").into());
            }
            let body = scene::apply_all("", &ops)?;
            let before = edit::snapshot(&project.analysis);
            drop(edit);
            let abs = abs_under(&project.game_dir, &rel);
            if let Some(parent) = abs.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("Could not create the folder: {e}"))?;
            }
            fs::write(&abs, &body).map_err(|e| format!("Could not write {rel}: {e}"))?;
            project.refresh_paths(std::slice::from_ref(&abs));
            let flags = history_state(&state);
            return Ok(SceneReport {
                path: rel,
                impact: edit::diff_impact(&before, &project.analysis),
                focus_line: 0,
                can_undo: flags.can_undo,
                can_redo: flags.can_redo,
            });
        }
        let original = script_text(project, &rel)?;
        let (text, focus_line) = scene::apply_all_focus(&original, &ops)?;
        let root = commands::backup_dir(&app, &project.root)?;
        let impact = edit::apply_write(project, &mut edit, &root, &rel, &text)?;
        if text != original {
            if let Ok(mut h) = state.scene_history.lock() {
                h.record(SceneStep {
                    rel: rel.clone(),
                    before: original,
                    after: text,
                });
            }
        }
        let flags = history_state(&state);
        Ok(SceneReport {
            path: rel,
            impact,
            focus_line,
            can_undo: flags.can_undo,
            can_redo: flags.can_redo,
        })
    })
}
