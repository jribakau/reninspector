//! Search, file operations, symbols, assets, translations, logs and project lifecycle.

use crate::error::AppError;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use renpy_core::{catalog, DialogueStats, LanguageStat, SearchHit, Symbol, Translation, Variable};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

use crate::commands::{self, AppState};
use crate::edit::{self, EditImpact};

use super::*;

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
pub fn get_catalog(state: State<'_, AppState>) -> Result<CatalogView, AppError> {
    let analysis = {
        let guard = crate::util::lock(&state.project);
        guard
            .as_ref()
            .ok_or_else(crate::util::no_project)?
            .analysis
            .clone()
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
) -> Result<Vec<Translation>, AppError> {
    crate::util::with_project(&state, |project| {
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
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Route {
    pub steps: Vec<String>,
}

#[tauri::command(async)]
pub fn label_routes(state: State<'_, AppState>, name: String) -> Result<Vec<Route>, AppError> {
    crate::util::with_project(&state, |project| {
        Ok(catalog::routes_to(
            &project.analysis.map.nodes,
            &project.analysis.map.edges,
            &name,
            8,
        )
        .into_iter()
        .map(|steps| Route { steps })
        .collect())
    })
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
) -> Result<SearchReport, AppError> {
    let query = query.trim().to_string();
    if query.len() < 2 {
        return Ok(SearchReport {
            hits: Vec::new(),
            truncated: false,
        });
    }
    const LIMIT: usize = 200;
    if dialogue_only {
        let guard = crate::util::lock(&state.project);
        let project = guard.as_ref().ok_or_else(crate::util::no_project)?;
        let mut hits = catalog::search_dialogue(&project.files, &query, LIMIT + 1);
        let truncated = hits.len() > LIMIT;
        if truncated {
            hits.truncate(LIMIT);
        }
        return Ok(SearchReport { hits, truncated });
    }
    let holds = {
        let guard = crate::util::lock(&state.project);
        let project = guard.as_ref().ok_or_else(crate::util::no_project)?;
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
pub(crate) fn same_text(current: &[u8], before: &str) -> bool {
    let current = current.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(current);
    edit::hash_bytes(current) == edit::hash_bytes(before.as_bytes())
}

#[tauri::command(async)]
pub fn replace_text(
    app: AppHandle,
    state: State<'_, AppState>,
    changes: Vec<TextReplace>,
) -> Result<EditImpact, AppError> {
    if changes.is_empty() {
        return Ok(EditImpact::default());
    }
    crate::util::with_project_mut(&state, |project| {
        commands::ensure_game_closed(project)?;
        let mut writes = Vec::with_capacity(changes.len());
        for change in &changes {
            let rel = script_rel(&change.path)?;
            let bytes = project.read_script_bytes(&rel)?;
            if !same_text(&bytes, &change.before) {
                return Err(format!(
                    "`{rel}` changed since the search. Search again before replacing."
                )
                .into());
            }
            writes.push((rel, change.text.clone()));
        }
        let mut edit = crate::util::lock(&state.edit);
        let root = commands::backup_dir(&app, &project.root)?;
        let (impact, _) = edit::apply_writes(project, &mut edit, &root, &writes)?;
        Ok(impact)
    })
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

fn read_hold_bytes(rel: &str, hold: ScriptHold) -> Result<Vec<u8>, AppError> {
    match hold {
        ScriptHold::Shared(text) => Ok(text.as_bytes().to_vec()),
        ScriptHold::Archived(archive, name) => archive
            .read_entry(&name, renpy_core::rpa::SCRIPT_MAX)
            .map_err(crate::error::AppError::from),
        ScriptHold::Path(path) => fs::read(&path)
            .map_err(|e| crate::error::AppError::new(format!("Could not read {rel}: {e}"))),
    }
}

fn read_hold(rel: &str, hold: ScriptHold) -> Option<String> {
    let bytes = read_hold_bytes(rel, hold).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// Text plus the hash of the bytes it came from, so an apply step can tell
/// whether the file changed after it was read.
fn read_hold_utf8(rel: &str, hold: ScriptHold) -> Result<(String, u64), AppError> {
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
) -> Result<Vec<RefHit>, AppError> {
    let (lines, holds) = {
        let guard = crate::util::lock(&state.project);
        let project = guard.as_ref().ok_or_else(crate::util::no_project)?;
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
) -> Result<ResolveResult, AppError> {
    crate::util::with_project(&state, |project| {
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

fn check_new_name(new_name: &str) -> Result<String, AppError> {
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
) -> Result<RenamePlan, AppError> {
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
) -> Result<RenamePreview, AppError> {
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
) -> Result<EditImpact, AppError> {
    let new_name = check_new_name(&new_name)?;
    if new_name == old_name {
        return Ok(EditImpact::default());
    }
    let plan = plan_rename_off_lock(&state, &kind, &old_name, &new_name)?;
    if plan.truncated {
        return Err("More than 300 lines match. Rename was not applied.".into());
    }
    crate::util::with_project_mut(&state, |project| {
        commands::ensure_game_closed(project)?;
        let mut edit = crate::util::lock(&state.edit);
        let root = commands::backup_dir(&app, &project.root)?;
        // The plan was built without the lock. Refuse if a save changed any of its files since.
        for (rel, _) in &plan.changes {
            let now = project.read_script_bytes(rel).map(|b| edit::hash_bytes(&b));
            if now.ok() != plan.sources.get(rel).copied() {
                return Err(format!(
                    "`{rel}` changed while the rename was being prepared. Try the rename again."
                )
                .into());
            }
        }
        let (impact, _) = edit::apply_writes(project, &mut edit, &root, &plan.changes)?;
        Ok(impact)
    })
}

fn plan_rename_off_lock(
    state: &State<'_, AppState>,
    kind: &str,
    old_name: &str,
    new_name: &str,
) -> Result<RenamePlan, AppError> {
    let (lines, holds) = {
        let guard = crate::util::lock(&state.project);
        let project = guard.as_ref().ok_or_else(crate::util::no_project)?;
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
