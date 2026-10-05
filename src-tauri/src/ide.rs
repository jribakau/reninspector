//! Search, file operations, symbols, assets, translations, logs and project lifecycle.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use renpy_core::scene::{self, LinkKind, Place, SceneOp, StmtSpec};
use renpy_core::{catalog, DialogueStats, LanguageStat, SearchHit, Symbol, Translation, Variable};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

use crate::commands::{self, AppState};
use crate::edit::{self, EditImpact};

fn no_project() -> String {
    "No project is open.".into()
}

fn script_rel(rel: &str) -> Result<String, String> {
    let rel = rel.trim().replace('\\', "/");
    if rel.is_empty()
        || rel.starts_with('/')
        || rel.contains(':')
        || rel
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == "..")
    {
        return Err("That path is not inside game/.".into());
    }
    if !(rel.ends_with(".rpy") || rel.ends_with(".rpym")) {
        return Err("Script files must end in .rpy.".into());
    }
    Ok(rel)
}

fn abs_under(game_dir: &Path, rel: &str) -> PathBuf {
    game_dir.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogView {
    pub symbols: Vec<Symbol>,
    pub variables: Vec<Variable>,
    pub languages: Vec<LanguageStat>,
    pub dialogue: DialogueStats,
    pub endings: Vec<String>,
    pub dead_ends: Vec<String>,
}

#[tauri::command(async)]
pub fn get_catalog(state: State<'_, AppState>) -> Result<CatalogView, String> {
    let analysis = {
        let guard = state.project.lock().map_err(|e| e.to_string())?;
        guard.as_ref().ok_or_else(no_project)?.analysis.clone()
    };
    let (endings, dead_ends) = catalog::endings(&analysis.map.nodes);
    Ok(CatalogView {
        symbols: analysis.catalog.symbols.clone(),
        variables: analysis.catalog.variables.clone(),
        languages: analysis.catalog.languages.clone(),
        dialogue: analysis.catalog.dialogue.clone(),
        endings,
        dead_ends,
    })
}

#[tauri::command(async)]
pub fn get_translations(
    state: State<'_, AppState>,
    lang: String,
) -> Result<Vec<Translation>, String> {
    let guard = state.project.lock().map_err(|e| e.to_string())?;
    let project = guard.as_ref().ok_or_else(no_project)?;
    let lang = lang.trim();
    Ok(project
        .analysis
        .catalog
        .translations
        .iter()
        .filter(|t| lang.is_empty() || t.lang == lang)
        .take(2000)
        .cloned()
        .collect())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Route {
    pub steps: Vec<String>,
}

#[tauri::command(async)]
pub fn label_routes(state: State<'_, AppState>, name: String) -> Result<Vec<Route>, String> {
    let guard = state.project.lock().map_err(|e| e.to_string())?;
    let project = guard.as_ref().ok_or_else(no_project)?;
    Ok(catalog::routes_to(
        &project.analysis.map.nodes,
        &project.analysis.map.edges,
        &name,
        8,
    )
    .into_iter()
    .map(|steps| Route { steps })
    .collect())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchReport {
    pub hits: Vec<SearchHit>,
    pub truncated: bool,
}

#[tauri::command(async)]
pub fn search_project(
    state: State<'_, AppState>,
    query: String,
    dialogue_only: bool,
) -> Result<SearchReport, String> {
    let query = query.trim().to_string();
    if query.len() < 2 {
        return Ok(SearchReport {
            hits: Vec::new(),
            truncated: false,
        });
    }
    const LIMIT: usize = 200;
    if dialogue_only {
        let guard = state.project.lock().map_err(|e| e.to_string())?;
        let project = guard.as_ref().ok_or_else(no_project)?;
        let mut hits = catalog::search_dialogue(&project.files, &query, LIMIT + 1);
        let truncated = hits.len() > LIMIT;
        if truncated {
            hits.truncate(LIMIT);
        }
        return Ok(SearchReport { hits, truncated });
    }
    let holds = {
        let guard = state.project.lock().map_err(|e| e.to_string())?;
        let project = guard.as_ref().ok_or_else(no_project)?;
        script_holds(project)
    };
    let mut hits = Vec::new();
    for (rel, hold) in holds {
        if hits.len() > LIMIT {
            break;
        }
        let Some(text) = read_hold(&rel, hold) else {
            continue;
        };
        catalog::search_lines(&rel, &text, &query, LIMIT + 1, &mut hits);
    }
    let truncated = hits.len() > LIMIT;
    if truncated {
        hits.truncate(LIMIT);
    }
    Ok(SearchReport { hits, truncated })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextReplace {
    pub path: String,
    pub before: String,
    pub text: String,
}

/// True when `current` is still the text the editor read. A leading BOM is ignored.
fn same_text(current: &[u8], before: &str) -> bool {
    let current = current.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(current);
    edit::hash_bytes(current) == edit::hash_bytes(before.as_bytes())
}

#[tauri::command(async)]
pub fn replace_text(
    app: AppHandle,
    state: State<'_, AppState>,
    changes: Vec<TextReplace>,
) -> Result<EditImpact, String> {
    if changes.is_empty() {
        return Ok(EditImpact::default());
    }
    let mut guard = state.project.lock().map_err(|e| e.to_string())?;
    let project = guard.as_mut().ok_or_else(no_project)?;
    commands::ensure_game_closed(project)?;
    let mut writes = Vec::with_capacity(changes.len());
    for change in &changes {
        let rel = script_rel(&change.path)?;
        let bytes = project.read_script_bytes(&rel)?;
        if !same_text(&bytes, &change.before) {
            return Err(format!(
                "`{rel}` changed since the search. Search again before replacing."
            ));
        }
        writes.push((rel, change.text.clone()));
    }
    let mut edit = state.edit.lock().map_err(|e| e.to_string())?;
    let root = commands::backup_dir(&app, &project.root)?;
    let (impact, _) = edit::apply_writes(project, &mut edit, &root, &writes)?;
    Ok(impact)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RefHit {
    pub path: String,
    pub line: u32,
    pub text: String,
}

fn owned_sites(
    catalog: &catalog::Catalog,
    kind: &str,
    name: &str,
) -> HashMap<String, HashSet<u32>> {
    let mut out: HashMap<String, HashSet<u32>> = HashMap::new();
    for site in &catalog.occurrences {
        if site.kind == kind && site.name == name {
            out.entry(site.path.clone()).or_default().insert(site.line);
        }
    }
    out
}

/// A script's text, or the loose path to read after the project lock is dropped.
enum ScriptHold {
    Shared(Arc<str>),
    Archived(Arc<renpy_core::rpa::Archive>, String),
    Path(PathBuf),
}

fn script_holds(project: &renpy_core::Project) -> Vec<(String, ScriptHold)> {
    project
        .files
        .iter()
        .map(|f| {
            let hold = if let Some(text) = &f.source {
                ScriptHold::Shared(Arc::clone(text))
            } else if let renpy_core::Origin::Archived { archive } = &f.origin {
                match project
                    .archives
                    .iter()
                    .find(|a| a.info.path == *archive)
                    .and_then(|a| a.archive.as_ref())
                {
                    Some(opened) => ScriptHold::Archived(Arc::clone(opened), f.rel.clone()),
                    None => ScriptHold::Shared(Arc::from("")),
                }
            } else {
                ScriptHold::Path(f.abs.clone())
            };
            (f.rel.clone(), hold)
        })
        .collect()
}

fn read_hold_bytes(rel: &str, hold: ScriptHold) -> Result<Vec<u8>, String> {
    match hold {
        ScriptHold::Shared(text) => Ok(text.as_bytes().to_vec()),
        ScriptHold::Archived(archive, name) => archive
            .read_entry(&name, renpy_core::rpa::SCRIPT_MAX)
            .map_err(|e| e.to_string()),
        ScriptHold::Path(path) => fs::read(&path).map_err(|e| format!("Could not read {rel}: {e}")),
    }
}

fn read_hold(rel: &str, hold: ScriptHold) -> Option<String> {
    let bytes = read_hold_bytes(rel, hold).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// Text plus the hash of the bytes it came from, so an apply step can tell
/// whether the file changed after it was read.
fn read_hold_utf8(rel: &str, hold: ScriptHold) -> Result<(String, u64), String> {
    let bytes = read_hold_bytes(rel, hold)?;
    let hash = edit::hash_bytes(&bytes);
    let text =
        String::from_utf8(bytes).map_err(|_| format!("`{rel}` is not UTF-8. Edit it in code."))?;
    Ok((text, hash))
}

#[tauri::command(async)]
pub fn find_references(
    state: State<'_, AppState>,
    kind: String,
    name: String,
) -> Result<Vec<RefHit>, String> {
    let (lines, holds) = {
        let guard = state.project.lock().map_err(|e| e.to_string())?;
        let project = guard.as_ref().ok_or_else(no_project)?;
        let lines = owned_sites(&project.analysis.catalog, &kind, &name);
        let holds = script_holds(project)
            .into_iter()
            .filter(|(rel, _)| lines.contains_key(rel))
            .collect::<Vec<_>>();
        (lines, holds)
    };
    let mut hits = Vec::new();
    for (rel, hold) in holds {
        if hits.len() >= 300 {
            break;
        }
        let Some(wanted) = lines.get(&rel) else {
            continue;
        };
        let Some(text) = read_hold(&rel, hold) else {
            continue;
        };
        for (i, line) in text.split('\n').enumerate() {
            if hits.len() >= 300 {
                break;
            }
            let n = (i + 1) as u32;
            if !wanted.contains(&n) {
                continue;
            }
            let shown = line.trim_end().trim_start_matches('\r');
            hits.push(RefHit {
                path: rel.clone(),
                line: n,
                text: shown.trim().chars().take(180).collect(),
            });
        }
    }
    Ok(hits)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveResult {
    pub hit: bool,
    pub symbol: Option<Symbol>,
}

#[tauri::command(async)]
pub fn resolve_symbol(
    state: State<'_, AppState>,
    path: String,
    line: u32,
    name: String,
    prefer: Option<String>,
) -> Result<ResolveResult, String> {
    let guard = state.project.lock().map_err(|e| e.to_string())?;
    let project = guard.as_ref().ok_or_else(no_project)?;
    let resolved = catalog::resolve(
        &project.analysis.catalog,
        &path,
        line,
        &name,
        prefer.as_deref(),
    );
    Ok(ResolveResult {
        hit: resolved.hit,
        symbol: resolved.symbol,
    })
}

const RENAME_LIMIT: usize = 300;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameEdit {
    pub path: String,
    pub line: u32,
    pub before: String,
    pub after: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenamePreview {
    pub hits: Vec<RenameEdit>,
    pub file_count: u32,
    pub truncated: bool,
}

struct RenamePlan {
    changes: Vec<(String, String)>,
    /// Hash of each file's bytes when it was read, to detect a save in between.
    sources: HashMap<String, u64>,
    hits: Vec<RenameEdit>,
    file_count: u32,
    truncated: bool,
}

fn check_new_name(new_name: &str) -> Result<String, String> {
    let new_name = new_name.trim().to_string();
    if new_name.is_empty() || new_name.contains(|c: char| c.is_whitespace()) {
        return Err("The new name has to be a single word.".into());
    }
    Ok(new_name)
}

fn clip_line(line: &str) -> String {
    line.trim().chars().take(180).collect()
}

/// Declaration and use lines from the index. Stops once more than `RENAME_LIMIT` lines change.
fn plan_rename(
    files: &[(String, String, u64)],
    lines: &HashMap<String, HashSet<u32>>,
    old_name: &str,
    new_name: &str,
) -> Result<RenamePlan, String> {
    let mut hits = Vec::new();
    let mut changes = Vec::new();
    let mut file_count = 0u32;
    let mut truncated = false;
    let mut sources = HashMap::new();
    for (rel, text, hash) in files {
        sources.insert(rel.clone(), *hash);
        if truncated {
            break;
        }
        let Some(wanted) = lines.get(rel) else {
            continue;
        };
        let mut next = String::new();
        let mut changed = false;
        let mut counted = false;
        for (i, line) in text.split('\n').enumerate() {
            let raw = line.trim_end_matches('\r');
            if wanted.contains(&((i + 1) as u32)) {
                let rewritten = catalog::rename_in_line(raw, old_name, new_name);
                if rewritten != raw {
                    changed = true;
                    if !counted {
                        file_count = file_count.saturating_add(1);
                        counted = true;
                    }
                    if hits.len() >= RENAME_LIMIT {
                        truncated = true;
                        break;
                    }
                    hits.push(RenameEdit {
                        path: rel.clone(),
                        line: (i + 1) as u32,
                        before: clip_line(raw),
                        after: clip_line(&rewritten),
                    });
                }
                next.push_str(&rewritten);
            } else {
                next.push_str(raw);
            }
            next.push('\n');
        }
        if truncated {
            break;
        }
        if changed {
            let body = if text.ends_with('\n') {
                next
            } else {
                next.trim_end_matches('\n').to_string()
            };
            changes.push((rel.clone(), body));
        }
    }
    Ok(RenamePlan {
        changes,
        sources,
        hits,
        file_count,
        truncated,
    })
}

#[tauri::command(async)]
pub fn preview_rename(
    state: State<'_, AppState>,
    kind: String,
    old_name: String,
    new_name: String,
) -> Result<RenamePreview, String> {
    let new_name = check_new_name(&new_name)?;
    if new_name == old_name {
        return Ok(RenamePreview {
            hits: Vec::new(),
            file_count: 0,
            truncated: false,
        });
    }
    let plan = plan_rename_off_lock(&state, &kind, &old_name, &new_name)?;
    Ok(RenamePreview {
        hits: plan.hits,
        file_count: plan.file_count,
        truncated: plan.truncated,
    })
}

#[tauri::command(async)]
pub fn rename_symbol(
    app: AppHandle,
    state: State<'_, AppState>,
    kind: String,
    old_name: String,
    new_name: String,
) -> Result<EditImpact, String> {
    let new_name = check_new_name(&new_name)?;
    if new_name == old_name {
        return Ok(EditImpact::default());
    }
    let plan = plan_rename_off_lock(&state, &kind, &old_name, &new_name)?;
    if plan.truncated {
        return Err("More than 300 lines match. Rename was not applied.".into());
    }
    let mut guard = state.project.lock().map_err(|e| e.to_string())?;
    let project = guard.as_mut().ok_or_else(no_project)?;
    commands::ensure_game_closed(project)?;
    let mut edit = state.edit.lock().map_err(|e| e.to_string())?;
    let root = commands::backup_dir(&app, &project.root)?;
    // The plan was built without the lock. Refuse if a save changed any of its files since.
    for (rel, _) in &plan.changes {
        let now = project.read_script_bytes(rel).map(|b| edit::hash_bytes(&b));
        if now.ok() != plan.sources.get(rel).copied() {
            return Err(format!(
                "`{rel}` changed while the rename was being prepared. Try the rename again."
            ));
        }
    }
    let (impact, _) = edit::apply_writes(project, &mut edit, &root, &plan.changes)?;
    Ok(impact)
}

fn plan_rename_off_lock(
    state: &State<'_, AppState>,
    kind: &str,
    old_name: &str,
    new_name: &str,
) -> Result<RenamePlan, String> {
    let (lines, holds) = {
        let guard = state.project.lock().map_err(|e| e.to_string())?;
        let project = guard.as_ref().ok_or_else(no_project)?;
        if !project.analysis_is_current() {
            return Err("The project is still being analysed. Try again in a moment.".into());
        }
        let lines = owned_sites(&project.analysis.catalog, kind, old_name);
        let holds = script_holds(project)
            .into_iter()
            .filter(|(rel, _)| lines.contains_key(rel))
            .collect::<Vec<_>>();
        (lines, holds)
    };
    let mut files = Vec::with_capacity(holds.len());
    for (rel, hold) in holds {
        let (text, hash) = read_hold_utf8(&rel, hold)?;
        files.push((rel, text, hash));
    }
    plan_rename(&files, &lines, old_name, new_name)
}

#[tauri::command(async)]
pub fn create_script(
    state: State<'_, AppState>,
    path: String,
    text: Option<String>,
) -> Result<(), String> {
    let rel = script_rel(&path)?;
    let mut guard = state.project.lock().map_err(|e| e.to_string())?;
    let project = guard.as_mut().ok_or_else(no_project)?;
    commands::ensure_game_closed(project)?;
    if project.file_index(&rel).is_some() {
        return Err(format!("`{rel}` already exists."));
    }
    let abs = abs_under(&project.game_dir, &rel);
    if abs.exists() {
        return Err(format!("`{rel}` already exists on disk."));
    }
    if let Some(parent) = abs.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Could not create the folder: {e}"))?;
    }
    let stem = rel
        .rsplit('/')
        .next()
        .unwrap_or("script")
        .trim_end_matches(".rpym")
        .trim_end_matches(".rpy");
    let body = text.unwrap_or_else(|| format!("label {stem}:\n    return\n"));
    fs::write(&abs, body).map_err(|e| format!("Could not write {rel}: {e}"))?;
    project.refresh_paths(std::slice::from_ref(&abs));
    Ok(())
}

#[tauri::command(async)]
pub fn rename_script(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    new_path: String,
) -> Result<(), String> {
    let from = script_rel(&path)?;
    let to = script_rel(&new_path)?;
    let mut guard = state.project.lock().map_err(|e| e.to_string())?;
    let project = guard.as_mut().ok_or_else(no_project)?;
    commands::ensure_game_closed(project)?;
    let idx = project
        .file_index(&from)
        .ok_or_else(|| format!("`{from}` is not a script of this project."))?;
    if !project.files[idx].abs.is_file() {
        return Err(
            "Only a loose script can be renamed. An archived script has to be saved first.".into(),
        );
    }
    if project.file_index(&to).is_some() {
        return Err(format!("`{to}` already exists."));
    }
    let src = project.files[idx].abs.clone();
    let dest = abs_under(&project.game_dir, &to);
    let root = commands::backup_dir(&app, &project.root)?;
    let backup = root.join(from.replace('/', std::path::MAIN_SEPARATOR_STR));
    if !backup.is_file() {
        if let Some(parent) = backup.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        fs::copy(&src, &backup).map_err(|e| format!("Could not back up {from}: {e}"))?;
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::rename(&src, &dest).map_err(|e| format!("Could not rename {from}: {e}"))?;
    project.refresh_paths(&[src, dest]);
    Ok(())
}

#[tauri::command(async)]
pub fn delete_script(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<(), String> {
    let rel = script_rel(&path)?;
    let mut guard = state.project.lock().map_err(|e| e.to_string())?;
    let project = guard.as_mut().ok_or_else(no_project)?;
    commands::ensure_game_closed(project)?;
    let idx = project
        .file_index(&rel)
        .ok_or_else(|| format!("`{rel}` is not a script of this project."))?;
    let abs = project.files[idx].abs.clone();
    if !abs.is_file() {
        return Err("This script is not a loose file, so there is nothing to delete.".into());
    }
    let root = commands::backup_dir(&app, &project.root)?;
    let backup = root.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
    if !backup.is_file() {
        if let Some(parent) = backup.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        fs::copy(&abs, &backup).map_err(|e| format!("Could not back up {rel}: {e}"))?;
    }
    fs::remove_file(&abs).map_err(|e| format!("Could not delete {rel}: {e}"))?;
    project.refresh_paths(std::slice::from_ref(&abs));
    Ok(())
}

#[tauri::command(async)]
pub fn update_translation(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    line: u32,
    text: String,
) -> Result<EditImpact, String> {
    if line == 0 {
        return Err("That translation is not tied to a line.".into());
    }
    let mut guard = state.project.lock().map_err(|e| e.to_string())?;
    let project = guard.as_mut().ok_or_else(no_project)?;
    commands::ensure_game_closed(project)?;
    let mut edit = state.edit.lock().map_err(|e| e.to_string())?;
    let root = commands::backup_dir(&app, &project.root)?;
    let bytes = project.read_script_bytes(&path)?;
    let original =
        String::from_utf8(bytes).map_err(|_| format!("`{path}` is not UTF-8. Edit it in code."))?;
    let mut lines: Vec<String> = original
        .split('\n')
        .map(|l| l.trim_end_matches('\r').to_string())
        .collect();
    let idx = (line as usize)
        .checked_sub(1)
        .ok_or("Line 0 is not a script line.")?;
    let row = lines
        .get_mut(idx)
        .ok_or_else(|| format!("{path} has no line {line}."))?;
    *row = catalog::replace_quoted(row, &text);
    let mut next = lines.join("\n");
    if original.ends_with('\n') && !next.ends_with('\n') {
        next.push('\n');
    }
    edit::apply_write(project, &mut edit, &root, &path, &next)
}

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
fn preview_kind(name: &str) -> Option<&'static str> {
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

fn append_archived_assets(files: &mut Vec<AssetFile>, total: &mut u32, archived: &[(String, u64)]) {
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
pub fn asset_report(state: State<'_, AppState>) -> Result<AssetReport, String> {
    let (game_dir, refs, image_uses, present_names, archive_assets) = {
        let guard = state.project.lock().map_err(|e| e.to_string())?;
        let project = guard.as_ref().ok_or_else(no_project)?;
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
fn asset_rel(path: &str) -> Result<String, String> {
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
) -> Result<tauri::ipc::Response, String> {
    let rel = asset_rel(&path)?;
    let guard = state.project.lock().map_err(|e| e.to_string())?;
    let project = guard.as_ref().ok_or_else(no_project)?;
    let abs = abs_under(&project.game_dir, &rel);
    if abs.is_file() {
        drop(guard);
        let bytes = fs::read(&abs).map_err(|e| e.to_string())?;
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
        let bytes = archive
            .read_entry(&name, 4_000_000)
            .map_err(|e| e.to_string())?;
        return Ok(tauri::ipc::Response::new(bytes));
    }
    Err(format!("`{rel}` is not in this project."))
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
) -> Result<tauri::ipc::Response, String> {
    let rel = dir_rel(&path)?;
    if rel.is_empty() {
        return Err("That path is not a file.".into());
    }
    let kind = preview_kind(&rel).ok_or_else(|| "That file cannot be previewed.".to_string())?;
    let limit = preview_limit(kind);
    let guard = state.project.lock().map_err(|e| e.to_string())?;
    let project = guard.as_ref().ok_or_else(no_project)?;
    let abs = abs_under(&project.root, &rel);
    if abs.is_file() {
        drop(guard);
        let bytes = fs::read(&abs).map_err(|e| e.to_string())?;
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
        let bytes = archive
            .read_entry(&name, limit)
            .map_err(|e| e.to_string())?;
        return Ok(tauri::ipc::Response::new(bytes));
    }
    Err(format!("`{rel}` is not in this project."))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogLine {
    pub source: String,
    pub text: String,
    pub path: Option<String>,
    pub line: Option<u32>,
}

#[tauri::command(async)]
pub fn read_logs(state: State<'_, AppState>) -> Result<Vec<LogLine>, String> {
    let guard = state.project.lock().map_err(|e| e.to_string())?;
    let project = guard.as_ref().ok_or_else(no_project)?;
    let mut out = Vec::new();
    for name in ["traceback.txt", "errors.txt", "log.txt"] {
        let path = project.root.join(name);
        let Some(text) = tail_file(&path, 80_000) else {
            continue;
        };
        for line in text
            .lines()
            .rev()
            .take(120)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
        {
            let (file, line_no) = link_in(line);
            out.push(LogLine {
                source: name.into(),
                text: line.trim_end().to_string(),
                path: file,
                line: line_no,
            });
        }
    }
    Ok(out)
}

const SAMPLE_SCRIPT: &str = include_str!("../sample/game/script.rpy");

/// Copies the bundled demo into the app data folder the first time, then
/// returns that folder. Later calls keep whatever the user changed.
#[tauri::command(async)]
pub fn open_sample(app: AppHandle) -> Result<String, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("sample");
    let script = dir.join("game").join("script.rpy");
    if !script.is_file() {
        if let Some(parent) = script.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Could not create the demo: {e}"))?;
        }
        fs::write(&script, SAMPLE_SCRIPT).map_err(|e| format!("Could not write the demo: {e}"))?;
    }
    Ok(dir.to_string_lossy().into_owned())
}

/// Swaps the user's home folder for `~`, in both slash styles, so a pasted
/// bundle does not carry the account name.
fn hide_home(text: &str, home: Option<&str>) -> String {
    let Some(home) = home.map(|h| h.trim_end_matches(['/', '\\'])).filter(|h| h.len() > 3) else {
        return text.to_string();
    };
    let mut out = text.to_string();
    for variant in [
        home.to_string(),
        home.replace('\\', "/"),
        home.replace('/', "\\"),
    ] {
        out = out.replace(&variant, "~");
    }
    out
}

fn home_dir() -> Option<String> {
    std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .ok()
}

/// Text the user asked to copy. Nothing is sent anywhere. The home folder is
/// replaced with `~`, but log lines can still name other private folders.
#[tauri::command(async)]
pub fn diagnostic_bundle(state: State<'_, AppState>) -> String {
    let mut out = format!(
        "Ren'Inspector {}\n{} {}\n",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    // Only the facts are read under the lock. The log files are read after it is released.
    let facts = state.project.lock().ok().and_then(|guard| {
        guard.as_ref().map(|project| {
            (
                project.root.clone(),
                project.engine_version.clone(),
                project.script_version.clone(),
            )
        })
    });
    if let Some((root, engine_version, script_version)) = facts {
        out.push_str(&format!("Project: {}\n", root.display()));
        if let Some(version) = engine_version {
            out.push_str(&format!("Ren'Py: {version}\n"));
        }
        if let Some(version) = script_version {
            out.push_str(&format!("Script version: {version}\n"));
        }
        out.push('\n');
        for name in ["traceback.txt", "errors.txt", "log.txt"] {
            let Some(text) = tail_file(&root.join(name), 8_000) else {
                continue;
            };
            let lines: Vec<&str> = text.lines().rev().take(40).collect();
            if lines.is_empty() {
                continue;
            }
            out.push_str(&format!("--- {name}\n"));
            for line in lines.into_iter().rev() {
                out.push_str(line);
                out.push('\n');
            }
        }
    } else {
        out.push_str("No project is open.\n");
    }
    hide_home(&out, home_dir().as_deref())
}

fn tail_file(path: &Path, max: usize) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    // Only the end of a log is wanted, and a log can be large.
    let mut file = fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let start = len.saturating_sub(max as u64);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut bytes = Vec::with_capacity((len - start) as usize);
    file.take(max as u64).read_to_end(&mut bytes).ok()?;
    let start = start as usize;
    let text = String::from_utf8_lossy(&bytes);
    let text = if start > 0 {
        text.split_once('\n')
            .map(|(_, rest)| rest)
            .unwrap_or(&text)
            .to_string()
    } else {
        text.into_owned()
    };
    Some(text)
}

fn link_in(line: &str) -> (Option<String>, Option<u32>) {
    if let Some(rest) = line.split("File \"").nth(1) {
        if let Some((file, after)) = rest.split_once('"') {
            let line_no = after.split("line ").nth(1).and_then(|s| {
                s.split(|c: char| !c.is_ascii_digit())
                    .next()
                    .and_then(|n| n.parse().ok())
            });
            return (Some(clean_game_path(file)), line_no);
        }
    }
    for token in line.split_whitespace() {
        let token = token.trim_matches(|c: char| c == ',' || c == ':' || c == '(' || c == ')');
        if let Some((file, n)) = token.rsplit_once(':') {
            if (file.ends_with(".rpy") || file.ends_with(".rpym"))
                && n.chars().all(|c| c.is_ascii_digit())
            {
                if let Ok(num) = n.parse() {
                    return (Some(clean_game_path(file)), Some(num));
                }
            }
        }
    }
    (None, None)
}

fn clean_game_path(file: &str) -> String {
    let file = file.replace('\\', "/");
    file.strip_prefix("./")
        .or_else(|| file.strip_prefix("game/"))
        .unwrap_or(&file)
        .to_string()
}

const AUTORELOAD: &str = "\
# Written by Ren'Inspector. While this file is here, a launch reloads scripts after they change.\n\
init 999 python:\n    \
    config.autoreload = True\n";

const AUTORELOAD_FILE: &str = "vnide_autoreload.rpy";

#[tauri::command]
pub fn autoreload_enabled(state: State<'_, AppState>) -> Result<bool, String> {
    let guard = state.project.lock().map_err(|e| e.to_string())?;
    let project = guard.as_ref().ok_or_else(no_project)?;
    Ok(project.game_dir.join(AUTORELOAD_FILE).is_file())
}

#[tauri::command]
pub fn set_autoreload(state: State<'_, AppState>, enabled: bool) -> Result<bool, String> {
    let guard = state.project.lock().map_err(|e| e.to_string())?;
    let project = guard.as_ref().ok_or_else(no_project)?;
    let path = project.game_dir.join(AUTORELOAD_FILE);
    if enabled {
        fs::write(&path, AUTORELOAD).map_err(|e| format!("Could not turn on auto-reload: {e}"))?;
    } else {
        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(path.with_extension("rpyc"));
    }
    Ok(enabled)
}

/// Keeps `vnide_autoreload.rpy` out of `game/` while a build runs, so a
/// distribution never ships it, and writes it back when dropped.
pub struct AutoreloadAside {
    restore: Option<PathBuf>,
}

impl AutoreloadAside {
    pub fn take(game_dir: &Path) -> Result<Self, String> {
        let path = game_dir.join(AUTORELOAD_FILE);
        // An orphaned .rpyc still loads, so it goes even when the .rpy is already gone.
        let _ = fs::remove_file(path.with_extension("rpyc"));
        if !path.is_file() {
            return Ok(Self { restore: None });
        }
        fs::remove_file(&path)
            .map_err(|e| format!("Could not take {AUTORELOAD_FILE} out of game/ for the build: {e}"))?;
        Ok(Self { restore: Some(path) })
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

fn scene_step(app: AppHandle, state: State<'_, AppState>, undo: bool) -> Result<SceneReport, String> {
    let mut guard = state.project.lock().map_err(|e| e.to_string())?;
    let project = guard.as_mut().ok_or_else(no_project)?;
    commands::ensure_game_closed(project)?;
    let mut edit = state.edit.lock().map_err(|e| e.to_string())?;
    let step = {
        let mut h = state.scene_history.lock().map_err(|e| e.to_string())?;
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
        return Err(stale(&step));
    };
    let (expect, target) = if undo {
        (&step.after, &step.before)
    } else {
        (&step.before, &step.after)
    };
    if &current != expect {
        return Err(stale(&step));
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
}

#[tauri::command(async)]
pub fn scene_undo(app: AppHandle, state: State<'_, AppState>) -> Result<SceneReport, String> {
    scene_step(app, state, true)
}

#[tauri::command(async)]
pub fn scene_redo(app: AppHandle, state: State<'_, AppState>) -> Result<SceneReport, String> {
    scene_step(app, state, false)
}

fn script_text(project: &renpy_core::Project, rel: &str) -> Result<String, String> {
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

fn import_image(game_dir: &Path, source: &str) -> Result<String, String> {
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
) -> Result<Vec<SceneOp>, String> {
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
            ));
        }
    }
    if change.op == "add-label" {
        scene::check_label(change.name.trim())?;
        if label_defined(project, change.name.trim()) {
            return Err(format!("`{}` is already a scene.", change.name.trim()));
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
                return Err(format!("`{var}` is already a character."));
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
                return Err(format!("`{name}` is already an image."));
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
) -> Result<renpy_core::stage::StageEstimate, String> {
    let guard = state.project.lock().map_err(|e| e.to_string())?;
    let project = guard.as_ref().ok_or_else(no_project)?;
    let pins: Vec<(String, String)> = vars.unwrap_or_default().into_iter().collect();
    renpy_core::stage::estimate(project, &file, line, &pins)
}

#[tauri::command(async)]
pub fn scene_edit(
    app: AppHandle,
    state: State<'_, AppState>,
    change: SceneChange,
) -> Result<SceneReport, String> {
    let mut guard = state.project.lock().map_err(|e| e.to_string())?;
    let project = guard.as_mut().ok_or_else(no_project)?;
    commands::ensure_game_closed(project)?;
    let mut edit = state.edit.lock().map_err(|e| e.to_string())?;
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
            return Err(format!("`{name}` is already an image."));
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
            return Err(format!("`{rel}` is not a script of this project."));
        }
        let body = scene::apply_all("", &ops)?;
        let before = edit::snapshot(&project.analysis);
        drop(edit);
        let abs = abs_under(&project.game_dir, &rel);
        if let Some(parent) = abs.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("Could not create the folder: {e}"))?;
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
) -> Result<String, String> {
    let name = name.trim().to_string();
    if name.is_empty() || name.contains(['/', '\\', ':', '.']) || name.contains('\n') {
        return Err("Use a game name without slashes, colons, or dots.".into());
    }
    let scene_name = scene.unwrap_or_else(|| "start".into()).trim().to_string();
    scene::check_label(&scene_name)?;
    let root = PathBuf::from(&parent).join(&name);
    if root.exists() {
        return Err(format!("{} already exists.", root.display()));
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
        None => {
            fs::write(game.join("options.rpy"), minimal_options(&name)).map_err(|e| e.to_string())
        }
    };
    let written = made.and_then(|_| {
        fs::write(game.join("script.rpy"), first_script(&scene_name)).map_err(|e| e.to_string())
    });
    if let Err(e) = written {
        let _ = fs::remove_dir_all(&root);
        return Err(e);
    }
    Ok(root.to_string_lossy().into_owned())
}

/// Ren'Py always begins at `label start`.
fn first_script(scene_name: &str) -> String {
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirEntry {
    pub name: String,
    pub path: String,
    pub dir: bool,
}

fn dir_rel(rel: &str) -> Result<String, String> {
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

fn merge_listing(prefix: &str, mut disk: Vec<DirEntry>, archived: &[String]) -> Vec<DirEntry> {
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

fn with_game_prefix(game_prefix: &str, name: &str) -> String {
    let name = norm_path(name);
    if game_prefix.is_empty() || name.is_empty() {
        name
    } else {
        format!("{game_prefix}/{name}")
    }
}

fn read_disk_children(root: &Path, rel: &str) -> Result<Vec<DirEntry>, String> {
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
    for entry in fs::read_dir(&dir).map_err(|e| e.to_string())?.flatten() {
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
pub fn list_dir(state: State<'_, AppState>, path: String) -> Result<Vec<DirEntry>, String> {
    let rel = dir_rel(&path)?;
    let (root, names) = {
        let guard = state.project.lock().map_err(|e| e.to_string())?;
        let project = guard.as_ref().ok_or_else(no_project)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn autoreload_is_out_of_game_during_a_build_and_back_after() {
        let game = std::env::temp_dir().join(format!("vnide-autoreload-{}", std::process::id()));
        let _ = fs::remove_dir_all(&game);
        fs::create_dir_all(&game).unwrap();
        let rpy = game.join(AUTORELOAD_FILE);
        fs::write(&rpy, AUTORELOAD).unwrap();
        fs::write(rpy.with_extension("rpyc"), b"compiled").unwrap();
        {
            let _aside = AutoreloadAside::take(&game).unwrap();
            assert!(!rpy.exists());
            assert!(!rpy.with_extension("rpyc").exists());
        }
        assert_eq!(fs::read_to_string(&rpy).unwrap(), AUTORELOAD);

        fs::remove_file(&rpy).unwrap();
        fs::write(rpy.with_extension("rpyc"), b"orphan").unwrap();
        drop(AutoreloadAside::take(&game).unwrap());
        assert!(!rpy.exists());
        assert!(!rpy.with_extension("rpyc").exists());
        let _ = fs::remove_dir_all(&game);
    }

    #[test]
    fn diagnostics_hide_the_home_folder() {
        let text = "C:\\Users\\ann\\game\\x.rpy and C:/Users/ann/game/y.rpy";
        assert_eq!(
            hide_home(text, Some("C:\\Users\\ann")),
            "~\\game\\x.rpy and ~/game/y.rpy"
        );
        assert_eq!(hide_home(text, None), text);
        assert_eq!(hide_home(text, Some("/")), text);
    }

    #[test]
    fn tail_keeps_only_the_end_of_a_file() {
        let path = std::env::temp_dir().join(format!("vnide-tail-{}.txt", std::process::id()));
        fs::write(&path, "one\ntwo\nthree\nfour\n").unwrap();
        assert_eq!(tail_file(&path, 1000).unwrap(), "one\ntwo\nthree\nfour\n");
        // Starts mid-line, so the partial first line is dropped.
        assert_eq!(tail_file(&path, 10).unwrap(), "four\n");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn replace_rejects_a_file_that_changed() {
        let before = "hello world\n";
        assert!(same_text(before.as_bytes(), before));
        assert!(!same_text(b"hello there\n", before));
        assert!(same_text(b"\xEF\xBB\xBFhello world\n", before));
    }

    #[test]
    fn first_scene_is_reached_from_start() {
        assert_eq!(
            first_script("start"),
            "label start:\n    \"…\"\n    return\n"
        );
        let named = first_script("intro");
        assert!(
            named.starts_with("label start:\n    jump intro\n\nlabel intro:\n"),
            "{named}"
        );
        let parsed = renpy_core::parser::check_syntax(&named);
        assert!(parsed.is_empty(), "{parsed:?}");
    }

    #[test]
    fn sdk_root_is_found_from_each_launcher() {
        let sdk = std::env::temp_dir().join(format!("vnide-sdk-root-{}", std::process::id()));
        let _ = fs::remove_dir_all(&sdk);
        fs::create_dir_all(sdk.join("launcher")).unwrap();
        fs::create_dir_all(sdk.join("gui").join("game")).unwrap();
        assert_eq!(
            crate::sdk::sdk_root(&sdk.join("renpy.exe")),
            Some(sdk.clone())
        );
        assert_eq!(
            crate::sdk::sdk_root(&sdk.join("renpy.sh")),
            Some(sdk.clone())
        );
        let mac = sdk
            .join("renpy.app")
            .join("Contents")
            .join("MacOS")
            .join("renpy");
        assert_eq!(crate::sdk::sdk_root(&mac), Some(sdk.clone()));
        assert_eq!(
            crate::sdk::sdk_root(
                &sdk.join("launcher")
                    .join("x")
                    .join("y")
                    .join("z")
                    .join("w")
                    .join("renpy")
            ),
            None
        );
        let _ = fs::remove_dir_all(&sdk);
    }

    /// Needs an installed SDK:
    ///   VNIDE_SDK=C:\renpy-8.5.2-sdk\renpy.exe cargo test --lib -- --ignored sdk_generates
    #[test]
    #[ignore]
    fn sdk_generates_the_standard_gui() {
        let exe = PathBuf::from(std::env::var("VNIDE_SDK").expect("VNIDE_SDK"));
        let root = std::env::temp_dir()
            .join(format!("vnide-sdk-gen-{}", std::process::id()))
            .join("My Game");
        let _ = fs::remove_dir_all(root.parent().unwrap());
        fs::create_dir_all(root.join("game")).unwrap();
        crate::sdk::generate_from_sdk(None, None, None, &exe, &root).unwrap();
        let game = root.join("game");
        for file in [
            "gui.rpy",
            "screens.rpy",
            "options.rpy",
            "gui/textbox.png",
            "gui/button/idle_background.png",
        ] {
            assert!(game.join(file).is_file(), "missing {file}");
        }
        let options = fs::read_to_string(game.join("options.rpy")).unwrap();
        assert!(
            options.contains("define config.name = _(\"My Game\")"),
            "{options}"
        );
        let _ = fs::remove_dir_all(root.parent().unwrap());
    }

    #[test]
    fn archived_images_join_the_asset_list() {
        let mut files = vec![AssetFile {
            path: "images/loose.png".into(),
            kind: "image".into(),
            bytes: 1,
            used: false,
        }];
        let mut total = 1u32;
        append_archived_assets(
            &mut files,
            &mut total,
            &[
                ("images/loose.png".into(), 9),
                ("images\\bg/sayori.png".into(), 20),
                ("audio/music.ogg".into(), 30),
                ("scripts/script.rpy".into(), 4),
                ("images/bg/sayori.png".into(), 99),
            ],
        );
        assert_eq!(total, 3);
        assert_eq!(files.len(), 3);
        assert_eq!(
            files
                .iter()
                .filter(|f| f.path == "images/loose.png")
                .count(),
            1
        );
        assert!(files
            .iter()
            .any(|f| f.path == "images/bg/sayori.png" && f.bytes == 20));
        assert!(files
            .iter()
            .any(|f| f.path == "audio/music.ogg" && f.kind == "audio"));
    }

    #[test]
    fn archive_path_lists_as_directory_then_file() {
        let names = vec!["scripts/story/a.rpy".into()];
        let parent = merge_listing("scripts", vec![], &names);
        assert_eq!(
            parent,
            vec![DirEntry {
                name: "story".into(),
                path: "scripts/story".into(),
                dir: true,
            }]
        );
        let child = merge_listing("scripts/story", vec![], &names);
        assert_eq!(
            child,
            vec![DirEntry {
                name: "a.rpy".into(),
                path: "scripts/story/a.rpy".into(),
                dir: false,
            }]
        );
    }

    #[test]
    fn disk_entry_hides_the_same_archive_child() {
        let disk = vec![DirEntry {
            name: "Scripts".into(),
            path: "Scripts".into(),
            dir: true,
        }];
        let merged = merge_listing("", disk, &["scripts/story/a.rpy".into()]);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].name, "Scripts");
        assert!(merged[0].dir);
    }

    #[test]
    fn listing_skips_cache_saves_and_shims() {
        let names = vec![
            "cache/x.png".into(),
            "saves/1.save".into(),
            "vnide_live.rpy".into(),
            "script.rpy".into(),
        ];
        let root = merge_listing("", vec![], &names);
        assert_eq!(root.len(), 1);
        assert_eq!(root[0].name, "script.rpy");
        assert!(!root[0].dir);
    }

    #[test]
    fn archive_entries_appear_under_the_game_folder() {
        let names = vec![with_game_prefix("game", "scripts/story/a.rpy")];
        let root = merge_listing(
            "",
            vec![DirEntry {
                name: "game".into(),
                path: "game".into(),
                dir: true,
            }],
            &names,
        );
        assert_eq!(root.len(), 1);
        assert_eq!(root[0].path, "game");
        let inside = merge_listing("game", vec![], &names);
        assert_eq!(inside[0].name, "scripts");
        assert_eq!(inside[0].path, "game/scripts");
        assert!(inside[0].dir);
    }

    #[test]
    fn asset_paths_stay_inside_game() {
        assert_eq!(asset_rel("images/bg.png").as_deref(), Ok("images/bg.png"));
        assert!(asset_rel(r"C:/Users/x.png").is_err());
        assert!(asset_rel(r"C:\Users\x.png").is_err());
        assert!(asset_rel("images/../../secret.png").is_err());
        assert!(asset_rel("images/../../../windows/notepad.exe").is_err());
        assert!(asset_rel("notes.txt").is_err());
    }

    #[test]
    fn preview_kinds_cover_text_image_sound_and_video() {
        assert_eq!(preview_kind("notes.md"), Some("text"));
        assert_eq!(preview_kind("shot.SVG"), Some("image"));
        assert_eq!(preview_kind("voice.flac"), Some("audio"));
        assert_eq!(preview_kind("clip.webm"), Some("video"));
        assert_eq!(preview_kind("archive.rpa"), None);
    }
}
