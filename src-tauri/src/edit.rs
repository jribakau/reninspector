//! Writing project scripts.
//!
//! The first write of a file copies the pristine bytes to the IDE's app-data
//! folder (never into the game). A later revert puts those bytes back. Writes
//! are recorded by content hash so the file watcher can ignore them instead of
//! reloading the editor.

use crate::error::AppError;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use renpy_core::{Analysis, Diagnostic, Origin, Project};
use serde::{Deserialize, Serialize};

pub fn hash_bytes(bytes: &[u8]) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut h);
    h.finish()
}

pub(crate) struct Snapshot {
    diags: Vec<Diagnostic>,
    labels: HashSet<String>,
    reach: HashMap<String, bool>,
}

pub(crate) fn snapshot(analysis: &Analysis) -> Snapshot {
    Snapshot {
        diags: analysis.diagnostics.items.clone(),
        labels: primary_labels(analysis),
        reach: analysis
            .map
            .nodes
            .iter()
            .map(|n| (n.id.clone(), n.reachable))
            .collect(),
    }
}

/// How long a recorded write suppresses watcher events for that file when the
/// bytes on disk do not yet match (the rename is still landing).
const PENDING_TTL: Duration = Duration::from_secs(3);

#[derive(Default)]
pub struct EditState {
    /// `path_key` -> hash of the bytes we are writing or just wrote.
    pub pending: HashMap<String, PendingWrite>,
}

pub struct PendingWrite {
    pub hash: u64,
    pub at: Instant,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct EditImpact {
    /// Problems that exist now and are not the same problem moved to a new line.
    pub added: Vec<Diagnostic>,
    pub removed: Vec<Diagnostic>,
    pub labels_added: Vec<String>,
    pub labels_removed: Vec<String>,
    pub became_unreachable: Vec<String>,
    pub became_reachable: Vec<String>,
}

impl EditImpact {
    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.added.is_empty()
            && self.removed.is_empty()
            && self.labels_added.is_empty()
            && self.labels_removed.is_empty()
            && self.became_unreachable.is_empty()
            && self.became_reachable.is_empty()
    }
}

/// `backups/<hash of the project root>/<path relative to game/>`.
pub fn backup_root(app_data: &Path, project_root: &Path) -> PathBuf {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    project_root.to_string_lossy().to_lowercase().hash(&mut h);
    app_data
        .join("backups")
        .join(format!("{:016x}", h.finish()))
}

pub fn path_key(path: &Path) -> String {
    let mut s = path.to_string_lossy().replace('\\', "/");
    if let Some(rest) = s.strip_prefix("//?/") {
        s = rest.to_string();
    }
    s.to_lowercase()
}

/// True when `bytes` are a write we just made (or are making). A match consumes
/// the record. A mismatch inside the TTL still counts as ours, so a notify
/// event that races the rename does not reload the editor.
pub fn is_our_write(edit: &mut EditState, path: &Path, bytes: &[u8]) -> bool {
    let key = path_key(path);
    let Some(pending) = edit.pending.get(&key) else {
        return false;
    };
    if hash_bytes(bytes) == pending.hash {
        edit.pending.remove(&key);
        return true;
    }
    if pending.at.elapsed() > PENDING_TTL {
        edit.pending.remove(&key);
        return false;
    }
    true
}

fn uses_crlf(original: &[u8]) -> bool {
    let crlf = original.windows(2).filter(|w| *w == b"\r\n").count();
    let lf = original.iter().filter(|b| **b == b'\n').count();
    crlf > 0 && crlf * 2 >= lf
}

/// The editor works in UTF-8 with `\n` and no BOM. Put back whatever the file had.
pub fn encode_like(original: &[u8], text: &str) -> Vec<u8> {
    let normalized = text.replace("\r\n", "\n");
    let body = if uses_crlf(original) {
        normalized.replace('\n', "\r\n")
    } else {
        normalized
    };
    let mut out = Vec::with_capacity(body.len() + 3);
    if original.starts_with(&[0xEF, 0xBB, 0xBF]) {
        out.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
    }
    out.extend(body.as_bytes());
    out
}

fn backup_path(backup_root: &Path, rel: &str) -> PathBuf {
    backup_root.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR))
}

/// Written instead of a byte backup when the script we first saved lives in an archive.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ArchivedMarker {
    archive: String,
    crc32: u32,
    loose_rpyc_existed: bool,
}

pub fn marker_path(backup_root: &Path, rel: &str) -> PathBuf {
    backup_path(backup_root, &format!("{rel}.vnide-archived"))
}

/// Written on the first save of a script that was decompiled from a `.rpyc`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DecompiledMarker {
    archive: Option<String>,
    /// The `.rpyc` was a loose file, so revert puts those bytes back.
    loose_rpyc: bool,
}

pub fn decompiled_marker_path(backup_root: &Path, rel: &str) -> PathBuf {
    backup_path(backup_root, &format!("{rel}.vnide-decompiled"))
}

fn read_decompiled_marker(backup_root: &Path, rel: &str) -> Option<DecompiledMarker> {
    let text = fs::read_to_string(decompiled_marker_path(backup_root, rel)).ok()?;
    serde_json::from_str(&text).ok()
}

fn write_decompiled_marker(
    backup_root: &Path,
    rel: &str,
    marker: &DecompiledMarker,
) -> Result<(), AppError> {
    let dest = decompiled_marker_path(backup_root, rel);
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Could not create backup folder: {e}"))?;
    }
    let text = serde_json::to_string(marker)?;
    fs::write(dest, text).map_err(|e| {
        crate::error::AppError::new(format!("Could not record the compiled original: {e}"))
    })
}

fn rpyc_rel(rel: &str) -> Option<String> {
    if let Some(stem) = rel.strip_suffix(".rpy") {
        Some(format!("{stem}.rpyc"))
    } else {
        rel.strip_suffix(".rpym")
            .map(|stem| format!("{stem}.rpymc"))
    }
}

fn read_marker(backup_root: &Path, rel: &str) -> Option<ArchivedMarker> {
    let text = fs::read_to_string(marker_path(backup_root, rel)).ok()?;
    serde_json::from_str(&text).ok()
}

fn write_marker(backup_root: &Path, rel: &str, marker: &ArchivedMarker) -> Result<(), AppError> {
    let dest = marker_path(backup_root, rel);
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Could not create backup folder: {e}"))?;
    }
    let text = serde_json::to_string(marker)?;
    fs::write(dest, text).map_err(|e| {
        crate::error::AppError::new(format!("Could not record the archived original: {e}"))
    })
}

fn compiled_sidecar(abs: &Path) -> PathBuf {
    let rpym = abs
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("rpym"))
        .unwrap_or(false);
    abs.with_extension(if rpym { "rpymc" } else { "rpyc" })
}

/// Copy the pristine file the first time we are about to change it.
fn ensure_backup(backup_root: &Path, rel: &str, abs: &Path) -> Result<PathBuf, AppError> {
    let dest = backup_path(backup_root, rel);
    if dest.is_file() {
        return Ok(dest);
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Could not create backup folder: {e}"))?;
    }
    fs::copy(abs, &dest).map_err(|e| format!("Could not back up {}: {e}", abs.display()))?;
    Ok(dest)
}

fn replace_file(dest: &Path, bytes: &[u8]) -> Result<(), AppError> {
    let name = dest
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "script.rpy".into());
    let tmp = dest.with_file_name(format!(".{name}.vnide-tmp"));
    let bak = dest.with_file_name(format!(".{name}.vnide-bak"));
    fs::write(&tmp, bytes).map_err(|e| format!("Could not write {}: {e}", tmp.display()))?;
    if !dest.exists() {
        if let Err(e) = fs::rename(&tmp, dest) {
            let _ = fs::remove_file(&tmp);
            return Err(format!("Could not move the new file into place: {e}").into());
        }
        return Ok(());
    }
    // Move the current file aside first so a crash never leaves the script missing.
    let _ = fs::remove_file(&bak);
    if let Err(e) = fs::rename(dest, &bak) {
        let _ = fs::remove_file(&tmp);
        return Err(format!(
            "Could not move {} aside ({e}). If the file is read-only, clear that and try again.",
            dest.display()
        )
        .into());
    }
    if let Err(e) = fs::rename(&tmp, dest) {
        let restored = fs::rename(&bak, dest);
        let _ = fs::remove_file(&tmp);
        if restored.is_err() {
            return Err(format!(
                "Could not move the new file into place ({e}). The previous copy is at {}.",
                bak.display()
            )
            .into());
        }
        return Err(format!("Could not move the new file into place: {e}").into());
    }
    let _ = fs::remove_file(&bak);
    Ok(())
}

fn fingerprint(d: &Diagnostic) -> String {
    format!("{}\u{1}{}\u{1}{}", d.code, d.path, d.message)
}

fn primary_labels(analysis: &Analysis) -> HashSet<String> {
    analysis
        .defs
        .iter()
        .filter(|d| d.primary && d.kind == "label")
        .map(|d| d.name.clone())
        .collect()
}

/// Diagnostics that only moved to a different line are the same problem, not a
/// new one. Everything else is reported.
pub(crate) fn diff_impact(before: &Snapshot, after: &Analysis) -> EditImpact {
    let mut unmatched: HashMap<String, Vec<Diagnostic>> = HashMap::new();
    for d in &before.diags {
        unmatched.entry(fingerprint(d)).or_default().push(d.clone());
    }
    let mut added = Vec::new();
    for d in &after.diagnostics.items {
        let key = fingerprint(d);
        if let Some(list) = unmatched.get_mut(&key) {
            if !list.is_empty() {
                list.pop();
                continue;
            }
        }
        added.push(d.clone());
    }
    let removed: Vec<Diagnostic> = unmatched.into_values().flatten().collect();

    let after_labels = primary_labels(after);
    let mut labels_added: Vec<String> = after_labels.difference(&before.labels).cloned().collect();
    let mut labels_removed: Vec<String> =
        before.labels.difference(&after_labels).cloned().collect();
    labels_added.sort();
    labels_removed.sort();

    let mut became_unreachable = Vec::new();
    let mut became_reachable = Vec::new();
    for n in &after.map.nodes {
        if let Some(was) = before.reach.get(&n.id) {
            if *was && !n.reachable {
                became_unreachable.push(n.id.clone());
            } else if !*was && n.reachable {
                became_reachable.push(n.id.clone());
            }
        }
    }
    became_unreachable.sort();
    became_reachable.sort();

    const CAP: usize = 40;
    EditImpact {
        added: added.into_iter().take(CAP).collect(),
        removed: removed.into_iter().take(CAP).collect(),
        labels_added,
        labels_removed,
        became_unreachable,
        became_reachable,
    }
}

/// Write one script without re-running analysis. `Ok(None)` when the bytes would not change.
fn commit_one(
    project: &Project,
    edit: &mut EditState,
    backup_root: &Path,
    rel: &str,
    text: &str,
) -> Result<Option<PathBuf>, AppError> {
    let idx = project
        .file_index(rel)
        .ok_or_else(|| format!("`{rel}` is not a script of this project."))?;
    let abs = project.files[idx].abs.clone();
    if !project.files[idx].origin.editable() {
        return Err(
            "This compiled script could not be fully recovered, so it stays read-only.".into(),
        );
    }
    let from_archive = matches!(project.files[idx].origin, Origin::Archived { .. });
    let from_compiled = matches!(project.files[idx].origin, Origin::Compiled { .. });
    let original = project
        .read_script_bytes(rel)
        .map_err(|e| format!("Could not read `{rel}`: {e}"))?;
    let bytes = encode_like(&original, text);
    if bytes == original {
        return Ok(None);
    }
    if let Some(parent) = abs.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Could not create {}: {e}", parent.display()))?;
    }
    let fresh_marker = from_archive && read_marker(backup_root, rel).is_none();
    let fresh_compiled = from_compiled && read_decompiled_marker(backup_root, rel).is_none();
    if fresh_compiled {
        let loose_rpyc = compiled_sidecar(&abs).is_file();
        if loose_rpyc {
            let side_rel = rpyc_rel(rel).unwrap_or_else(|| rel.to_string());
            ensure_backup(backup_root, &side_rel, &compiled_sidecar(&abs))?;
        }
        write_decompiled_marker(
            backup_root,
            rel,
            &DecompiledMarker {
                archive: project.files[idx].origin.archive().map(|s| s.to_string()),
                loose_rpyc,
            },
        )?;
    } else if fresh_marker {
        let archive = project.files[idx]
            .origin
            .archive()
            .unwrap_or("")
            .to_string();
        let crc32 = project.archived_crc(&archive, rel).unwrap_or(0);
        let sidecar = compiled_sidecar(&abs);
        write_marker(
            backup_root,
            rel,
            &ArchivedMarker {
                archive,
                crc32,
                loose_rpyc_existed: sidecar.is_file(),
            },
        )?;
    } else if !from_archive && !from_compiled && read_marker(backup_root, rel).is_none() {
        ensure_backup(backup_root, rel, &abs)?;
    }
    edit.pending.insert(
        path_key(&abs),
        PendingWrite {
            hash: hash_bytes(&bytes),
            at: Instant::now(),
        },
    );
    if let Err(e) = replace_file(&abs, &bytes) {
        edit.pending.remove(&path_key(&abs));
        if fresh_marker {
            let _ = fs::remove_file(marker_path(backup_root, rel));
        }
        if fresh_compiled {
            let _ = fs::remove_file(decompiled_marker_path(backup_root, rel));
        }
        return Err(e);
    }
    Ok(Some(abs))
}

/// A crash between moving a script aside and moving its replacement in leaves
/// `.{name}.vnide-bak` next to a missing script. Put those back; drop any
/// backup whose script is present. Returns the restored script paths.
pub fn recover_interrupted_saves(game_dir: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, depth: u32, restored: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if depth < 8 {
                    walk(&path, depth + 1, restored);
                }
                continue;
            }
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let Some(script) = name
                .strip_prefix('.')
                .and_then(|n| n.strip_suffix(".vnide-bak"))
            else {
                continue;
            };
            let dest = path.with_file_name(script);
            if dest.exists() {
                let _ = fs::remove_file(&path);
            } else if fs::rename(&path, &dest).is_ok() {
                restored.push(dest);
            }
        }
    }
    let mut restored = Vec::new();
    walk(game_dir, 0, &mut restored);
    restored
}

/// Write `text` and re-read the file, without rebuilding analysis.
/// `None` when the bytes would not change.
pub(crate) fn prepare_write(
    project: &mut Project,
    edit: &mut EditState,
    backup_root: &Path,
    rel: &str,
    text: &str,
) -> Result<Option<Snapshot>, AppError> {
    let before = snapshot(&project.analysis);
    let Some(abs) = commit_one(project, edit, backup_root, rel, text)? else {
        return Ok(None);
    };
    project.ingest_paths(std::slice::from_ref(&abs));
    Ok(Some(before))
}

/// Write `text` over an existing project script and re-parse.
/// Does nothing when the bytes would not change.
pub fn apply_write(
    project: &mut Project,
    edit: &mut EditState,
    backup_root: &Path,
    rel: &str,
    text: &str,
) -> Result<EditImpact, AppError> {
    let Some(before) = prepare_write(project, edit, backup_root, rel, text)? else {
        return Ok(EditImpact::default());
    };
    project.reanalyze();
    Ok(diff_impact(&before, &project.analysis))
}

struct WrittenFile {
    abs: PathBuf,
    rel: String,
    previous: Option<Vec<u8>>,
    existed: bool,
    /// Markers that were already there. Ones this write created go away on rollback,
    /// so an undone file is not listed as modified.
    had_marker: bool,
    had_compiled_marker: bool,
}

fn rollback_writes(backup_root: &Path, written: &[WrittenFile]) {
    for file in written.iter().rev() {
        if let Some(bytes) = &file.previous {
            let _ = replace_file(&file.abs, bytes);
        } else if !file.existed {
            let _ = fs::remove_file(&file.abs);
        }
        if !file.had_marker {
            let _ = fs::remove_file(marker_path(backup_root, &file.rel));
        }
        if !file.had_compiled_marker {
            let _ = fs::remove_file(decompiled_marker_path(backup_root, &file.rel));
        }
    }
}

/// Write several scripts, then re-parse once.
pub fn apply_writes(
    project: &mut Project,
    edit: &mut EditState,
    backup_root: &Path,
    changes: &[(String, String)],
) -> Result<(EditImpact, Vec<String>), AppError> {
    let before = snapshot(&project.analysis);
    let mut paths = Vec::new();
    let mut rels = Vec::new();
    let mut written: Vec<WrittenFile> = Vec::new();
    for (rel, text) in changes {
        let (existed, previous) = match project.file_index(rel) {
            Some(idx) if project.files[idx].abs.is_file() => {
                match fs::read(&project.files[idx].abs) {
                    Ok(bytes) => (true, Some(bytes)),
                    Err(e) => {
                        rollback_writes(backup_root, &written);
                        return Err(format!("Could not read `{rel}` before writing: {e}").into());
                    }
                }
            }
            _ => (false, None),
        };
        let had_marker = marker_path(backup_root, rel).is_file();
        let had_compiled_marker = decompiled_marker_path(backup_root, rel).is_file();
        match commit_one(project, edit, backup_root, rel, text) {
            Ok(Some(abs)) => {
                written.push(WrittenFile {
                    abs: abs.clone(),
                    rel: rel.clone(),
                    previous,
                    existed,
                    had_marker,
                    had_compiled_marker,
                });
                paths.push(abs);
                rels.push(rel.clone());
            }
            Ok(None) => {}
            Err(e) => {
                rollback_writes(backup_root, &written);
                if !written.is_empty() {
                    let undone: Vec<PathBuf> = written.iter().map(|w| w.abs.clone()).collect();
                    project.refresh_paths(&undone);
                }
                return Err(format!("{e} Earlier files in this change were put back.").into());
            }
        }
    }
    if paths.is_empty() {
        return Ok((EditImpact::default(), Vec::new()));
    }
    project.refresh_paths(&paths);
    Ok((diff_impact(&before, &project.analysis), rels))
}

/// Put the pristine backup back and re-parse.
pub fn apply_revert(
    project: &mut Project,
    edit: &mut EditState,
    backup_root: &Path,
    rel: &str,
) -> Result<EditImpact, AppError> {
    let idx = project
        .file_index(rel)
        .ok_or_else(|| format!("`{rel}` is not a script of this project."))?;
    let abs = project.files[idx].abs.clone();
    if let Some(marker) = read_decompiled_marker(backup_root, rel) {
        if abs.is_file() {
            fs::remove_file(&abs)
                .map_err(|e| format!("Could not remove {}: {e}", abs.display()))?;
        }
        let side = compiled_sidecar(&abs);
        if marker.loose_rpyc {
            let side_rel = rpyc_rel(rel).unwrap_or_else(|| rel.to_string());
            let backup = backup_path(backup_root, &side_rel);
            let bytes = fs::read(&backup)
                .map_err(|e| format!("Could not read the compiled backup: {e}"))?;
            replace_file(&side, &bytes)?;
        } else if side.is_file() {
            let _ = fs::remove_file(&side);
        }
        let _ = fs::remove_file(decompiled_marker_path(backup_root, rel));
        let before = snapshot(&project.analysis);
        project.refresh_paths(std::slice::from_ref(&abs));
        return Ok(diff_impact(&before, &project.analysis));
    }
    if let Some(marker) = read_marker(backup_root, rel) {
        if abs.is_file() {
            fs::remove_file(&abs)
                .map_err(|e| format!("Could not remove {}: {e}", abs.display()))?;
        }
        if !marker.loose_rpyc_existed {
            let _ = fs::remove_file(compiled_sidecar(&abs));
        }
        let _ = fs::remove_file(marker_path(backup_root, rel));
        let before = snapshot(&project.analysis);
        project.refresh_paths(std::slice::from_ref(&abs));
        return Ok(diff_impact(&before, &project.analysis));
    }
    let backup = backup_path(backup_root, rel);
    if !backup.is_file() {
        return Err(format!("`{rel}` has no backup. It has not been saved from the IDE.").into());
    }
    let bytes = fs::read(&backup).map_err(|e| format!("Could not read the backup: {e}"))?;
    edit.pending.insert(
        path_key(&abs),
        PendingWrite {
            hash: hash_bytes(&bytes),
            at: Instant::now(),
        },
    );
    if let Err(e) = replace_file(&abs, &bytes) {
        edit.pending.remove(&path_key(&abs));
        return Err(e);
    }
    let before = snapshot(&project.analysis);
    project.refresh_paths(std::slice::from_ref(&abs));
    Ok(diff_impact(&before, &project.analysis))
}

/// Scripts saved from a decompiled `.rpyc`. Bake packages these like overrides.
pub fn decompiled_edits(project: &Project, backup_root: &Path) -> Vec<String> {
    project
        .files
        .iter()
        .filter(|f| decompiled_marker_path(backup_root, &f.rel).is_file())
        .map(|f| f.rel.clone())
        .collect()
}

/// Scripts whose bytes differ from the pristine backup, relative to `game/`.
pub fn modified_files(project: &Project, backup_root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for f in &project.files {
        if marker_path(backup_root, &f.rel).is_file()
            || decompiled_marker_path(backup_root, &f.rel).is_file()
        {
            out.push(f.rel.clone());
            continue;
        }
        let backup = backup_path(backup_root, &f.rel);
        if !backup.is_file() {
            continue;
        }
        let current = fs::read(&f.abs).unwrap_or_default();
        let saved = fs::read(&backup).unwrap_or_default();
        if current != saved {
            out.push(f.rel.clone());
        }
    }
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_project(name: &str, files: &[(&str, &str)]) -> (PathBuf, Project) {
        let root = std::env::temp_dir().join(format!("vn-ide-edit-{name}-{}", std::process::id()));
        let game = root.join("game");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&game).unwrap();
        for (rel, text) in files {
            let p = game.join(rel);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, text).unwrap();
        }
        let project = Project::open(&root).unwrap();
        (root, project)
    }

    #[test]
    fn encode_preserves_bom_and_crlf() {
        let original = b"\xEF\xBB\xBFlabel start:\r\n    return\r\n";
        let encoded = encode_like(original, "label start:\n    jump shop\n");
        assert!(encoded.starts_with(&[0xEF, 0xBB, 0xBF]));
        assert!(encoded.windows(2).any(|w| w == b"\r\n"));
        assert!(!encoded.windows(2).any(|w| w == b"\n\n") || encoded.contains(&b'\r'));
        let lf = encode_like(
            b"label start:\n    return\n",
            "label start:\n    jump shop\n",
        );
        assert!(!lf.windows(2).any(|w| w == b"\r\n"));
    }

    #[test]
    fn backup_is_pristine_and_revert_restores_it() {
        let script = "label start:\n    jump shop\nlabel shop:\n    return\n";
        let (root, mut project) = temp_project("backup", &[("script.rpy", script)]);
        let mut edit = EditState::default();
        let backups = root.join("backups");
        let impact = apply_write(
            &mut project,
            &mut edit,
            &backups,
            "script.rpy",
            "label start:\n    jump nowhere\nlabel shop:\n    return\n",
        )
        .unwrap();
        assert!(impact
            .added
            .iter()
            .any(|d| d.code == "missing-label" && d.message.contains("nowhere")));
        assert!(impact.became_unreachable.iter().any(|n| n == "shop"));
        let backup = fs::read(backups.join("script.rpy")).unwrap();
        assert_eq!(backup, script.as_bytes());
        // A second write must not overwrite the pristine copy.
        apply_write(
            &mut project,
            &mut edit,
            &backups,
            "script.rpy",
            "label start:\n    jump other\nlabel shop:\n    return\n",
        )
        .unwrap();
        assert_eq!(
            fs::read(backups.join("script.rpy")).unwrap(),
            script.as_bytes()
        );
        assert_eq!(modified_files(&project, &backups), ["script.rpy"]);

        let back = apply_revert(&mut project, &mut edit, &backups, "script.rpy").unwrap();
        assert!(back.removed.iter().any(|d| d.code == "missing-label"));
        assert!(modified_files(&project, &backups).is_empty());
        assert_eq!(
            fs::read(root.join("game/script.rpy")).unwrap(),
            script.as_bytes()
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn self_write_is_suppressed_and_then_forgotten() {
        let (root, project) =
            temp_project("watch", &[("script.rpy", "label start:\n    return\n")]);
        let abs = project.files[0].abs.clone();
        let mut edit = EditState::default();
        let bytes = b"label start:\n    jump shop\n";
        edit.pending.insert(
            path_key(&abs),
            PendingWrite {
                hash: hash_bytes(bytes),
                at: Instant::now(),
            },
        );
        assert!(is_our_write(&mut edit, &abs, bytes));
        assert!(edit.pending.is_empty(), "a matching write is consumed");
        edit.pending.insert(
            path_key(&abs),
            PendingWrite {
                hash: hash_bytes(bytes),
                at: Instant::now(),
            },
        );
        assert!(is_our_write(&mut edit, &abs, b"still the old bytes"));
        assert!(!edit.pending.is_empty(), "a raced event stays pending");
        edit.pending.get_mut(&path_key(&abs)).unwrap().at =
            Instant::now() - Duration::from_secs(10);
        assert!(!is_our_write(&mut edit, &abs, b"an external edit"));
        assert!(edit.pending.is_empty());
        let _ = fs::remove_dir_all(root);
    }

    /// Rewriting a large script must come back in a few seconds.
    /// Skipped unless `RENPY_GAME_PATH` points at the project. The original is never modified.
    #[test]
    fn large_script_save_stays_responsive() {
        let Ok(game) = std::env::var("RENPY_GAME_PATH") else {
            return;
        };
        let src = PathBuf::from(game).join("game").join("script.rpy");
        if !src.is_file() {
            return;
        }
        let root = std::env::temp_dir().join(format!("vn-ide-edit-large-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("game")).unwrap();
        let dest = root.join("game").join("script.rpy");
        fs::copy(&src, &dest).unwrap();
        let mut project = Project::open(&root).unwrap();
        let mut edit = EditState::default();
        let text = fs::read_to_string(&dest).unwrap() + "\n# vn-ide timing\n";
        let started = Instant::now();
        let impact = apply_write(
            &mut project,
            &mut edit,
            &root.join("backups"),
            "script.rpy",
            &text,
        )
        .unwrap();
        let ms = started.elapsed().as_millis();
        eprintln!(
            "large save took {ms} ms ({} new problems)",
            impact.added.len()
        );
        assert!(ms < 8_000, "saving a large script took {ms} ms");
        assert!(root.join("backups").join("script.rpy").is_file());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn saving_an_archived_script_writes_an_override_and_revert_removes_it() {
        let root = std::env::temp_dir().join(format!("vn-ide-edit-rpa-{}", std::process::id()));
        let game = root.join("game");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&game).unwrap();
        let script = b"label start:\n    return\n";
        let mut writer = renpy_core::rpa::ArchiveWriter::create(&game.join("scripts.rpa")).unwrap();
        writer.add_bytes("chapter.rpy", script).unwrap();
        writer.finish().unwrap();
        let mut project = Project::open(&root).unwrap();
        let idx = project.file_index("chapter.rpy").expect("archived script");
        assert_eq!(project.files[idx].origin.kind(), "archived");
        let mut edit = EditState::default();
        let backups = root.join("backups");
        apply_write(
            &mut project,
            &mut edit,
            &backups,
            "chapter.rpy",
            "label start:\n    jump shop\n",
        )
        .unwrap();
        assert!(game.join("chapter.rpy").is_file());
        assert_eq!(
            project.files[project.file_index("chapter.rpy").unwrap()]
                .origin
                .kind(),
            "override"
        );
        assert_eq!(modified_files(&project, &backups), ["chapter.rpy"]);
        // A compiled sidecar the game creates after the save is removed on revert.
        fs::write(game.join("chapter.rpyc"), b"compiled").unwrap();
        apply_revert(&mut project, &mut edit, &backups, "chapter.rpy").unwrap();
        assert!(!game.join("chapter.rpy").exists());
        assert!(!game.join("chapter.rpyc").exists());
        assert_eq!(
            project.files[project.file_index("chapter.rpy").unwrap()]
                .origin
                .kind(),
            "archived"
        );
        assert!(modified_files(&project, &backups).is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn saving_a_decompiled_script_backs_up_the_rpyc_and_revert_restores_it() {
        let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("renpy-core/tests/fixtures/rpyc/8/script.rpyc");
        let Ok(rpyc) = fs::read(&fixture) else {
            eprintln!("compiled fixture is missing; skipping");
            return;
        };
        let root = std::env::temp_dir().join(format!("vn-ide-edit-rpyc-{}", std::process::id()));
        let game = root.join("game");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&game).unwrap();
        fs::write(game.join("script.rpyc"), &rpyc).unwrap();
        let mut project = Project::open(&root).unwrap();
        let idx = project.file_index("script.rpy").expect("decompiled script");
        assert_eq!(project.files[idx].origin.kind(), "compiled");
        assert!(project.files[idx].origin.editable());
        let mut edit = EditState::default();
        let backups = root.join("backups");
        apply_write(
            &mut project,
            &mut edit,
            &backups,
            "script.rpy",
            "label start:\n    jump shop\n",
        )
        .unwrap();
        assert!(game.join("script.rpy").is_file());
        assert!(decompiled_marker_path(&backups, "script.rpy").is_file());
        let backed = fs::read(backup_path(&backups, "script.rpyc")).unwrap();
        assert_eq!(backed, rpyc);
        assert!(modified_files(&project, &backups).contains(&"script.rpy".to_string()));
        apply_revert(&mut project, &mut edit, &backups, "script.rpy").unwrap();
        assert!(!game.join("script.rpy").exists());
        assert_eq!(fs::read(game.join("script.rpyc")).unwrap(), rpyc);
        assert!(!decompiled_marker_path(&backups, "script.rpy").exists());
        assert_eq!(
            project.files[project.file_index("script.rpy").unwrap()]
                .origin
                .kind(),
            "compiled"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn identical_text_does_not_backup() {
        let script = "label start:\n    return\n";
        let (root, mut project) = temp_project("same", &[("script.rpy", script)]);
        let mut edit = EditState::default();
        let backups = root.join("backups");
        let impact = apply_write(&mut project, &mut edit, &backups, "script.rpy", script).unwrap();
        assert!(impact.is_empty());
        assert!(!backups.exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn replace_file_swaps_in_place_and_leaves_no_helpers() {
        let dir = std::env::temp_dir().join(format!("vn-ide-replace-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let dest = dir.join("s.rpy");
        replace_file(&dest, b"one").unwrap();
        replace_file(&dest, b"two").unwrap();
        assert_eq!(fs::read(&dest).unwrap(), b"two");
        let names: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert_eq!(names.len(), 1, "{names:?}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_interrupted_save_is_put_back_on_open() {
        let dir = std::env::temp_dir().join(format!("vn-ide-recover-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("sub")).unwrap();
        fs::write(dir.join(".lost.rpy.vnide-bak"), b"kept").unwrap();
        fs::write(dir.join("sub").join("here.rpy"), b"new").unwrap();
        fs::write(dir.join("sub").join(".here.rpy.vnide-bak"), b"old").unwrap();
        let restored = recover_interrupted_saves(&dir);
        assert_eq!(restored, vec![dir.join("lost.rpy")]);
        assert_eq!(fs::read(dir.join("lost.rpy")).unwrap(), b"kept");
        assert_eq!(fs::read(dir.join("sub").join("here.rpy")).unwrap(), b"new");
        assert!(!dir.join("sub").join(".here.rpy.vnide-bak").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_failed_multi_file_write_puts_earlier_files_back() {
        let a = "label a:\n    return\n";
        let (root, mut project) = temp_project("rollback", &[("a.rpy", a)]);
        let mut edit = EditState::default();
        let backups = root.join("backups");
        let changes = vec![
            ("a.rpy".to_string(), "label a:\n    jump b\n".to_string()),
            (
                "missing.rpy".to_string(),
                "label m:\n    return\n".to_string(),
            ),
        ];
        let err = apply_writes(&mut project, &mut edit, &backups, &changes).unwrap_err();
        assert!(err.contains("put back"), "{err}");
        assert_eq!(fs::read(root.join("game/a.rpy")).unwrap(), a.as_bytes());
        assert!(modified_files(&project, &backups).is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn the_file_index_stays_valid_between_ingest_and_publish() {
        let (root, mut project) = temp_project(
            "ingest",
            &[
                ("b.rpy", "label b:\n    return\n"),
                ("z.rpy", "label z:\n    return\n"),
            ],
        );
        let a = root.join("game/a.rpy");
        fs::write(&a, "label a:\n    return\n").unwrap();
        assert!(project.ingest_paths(std::slice::from_ref(&a)));
        let job = project.fork_analysis();
        for rel in ["a.rpy", "b.rpy", "z.rpy"] {
            let idx = project.file_index(rel).unwrap();
            assert_eq!(project.files[idx].rel, rel);
        }
        // Until the run is published, graphs come from the current files and are not cached.
        let graph = project.label_graph("a", false).unwrap();
        assert_eq!(graph.name, "a");
        assert!(project.cached_label_graph("a", false).is_none());
        let analysis = job.run();
        assert!(project.publish_analysis(job.epoch, analysis));
        assert!(project.analysis.by_name.contains_key("a"));
        let _ = fs::remove_dir_all(root);
    }
}
