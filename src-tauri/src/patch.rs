//! Fold loose overrides into a patch archive the IDE owns.
//!
//! The game's original archives are never written. The patch is a normal
//! RPA-3.0 file whose name sorts last, so Ren'Py searches it first, and a
//! loose file still beats it. Bake copies the previous patch and the loose
//! files aside first, so undo can put them back.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use renpy_core::rpa::{self, Archive, ArchiveWriter, PatchEntry, PatchManifest};
use renpy_core::{Origin, Project};
use serde::{Deserialize, Serialize};

use crate::edit;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchReport {
    pub patch: String,
    pub files: u32,
    pub action: String,
}

#[derive(Serialize, Deserialize)]
struct UndoOp {
    kind: String,
    patch_rel: String,
    had_previous: bool,
    /// Loose files to put back, relative to `game/`.
    files: Vec<String>,
}

pub fn undo_root(app_data: &Path, project_root: &Path) -> PathBuf {
    edit::backup_root(app_data, project_root)
        .parent()
        .unwrap_or(app_data)
        .join("rpa-undo")
        .join(
            edit::backup_root(app_data, project_root)
                .file_name()
                .unwrap_or_default(),
        )
}

fn safe_join(root: &Path, rel: &str) -> Result<PathBuf, String> {
    if rel.is_empty()
        || rel.contains('\0')
        || rel
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == "..")
    {
        return Err(format!("unsafe path `{rel}`"));
    }
    let mut path = root.to_path_buf();
    for part in rel.split('/') {
        path.push(part);
    }
    Ok(path)
}

fn patch_rel(project: &Project) -> String {
    if let Some(existing) = project.archives.iter().find(|a| a.info.is_patch) {
        return existing.info.path.clone();
    }
    let stems: Vec<String> = project
        .archives
        .iter()
        .map(|a| {
            a.info
                .path
                .rsplit_once('.')
                .map(|(stem, _)| stem.to_string())
                .unwrap_or_else(|| a.info.path.clone())
        })
        .collect();
    format!("{}.rpa", rpa::patch_stem(stems))
}

fn sidecar_rel(rel: &str) -> String {
    if rel.ends_with(".rpym") {
        format!("{}.rpymc", &rel[..rel.len() - 5])
    } else if let Some(stem) = rel.strip_suffix(".rpy") {
        format!("{stem}.rpyc")
    } else {
        format!("{rel}.rpyc")
    }
}

fn sidecar_is_stale(project: &Project, file: &renpy_core::project::SourceFile) -> bool {
    let Ok(side) = safe_join(&project.game_dir, &sidecar_rel(&file.rel)) else {
        return true;
    };
    match (modified_at(&side), modified_at(&file.abs)) {
        (Some(compiled), Some(source)) => compiled < source,
        _ => true,
    }
}

fn compile_overrides(
    project: &Project,
    overrides: &[&renpy_core::project::SourceFile],
) -> Result<(), String> {
    if !overrides.iter().any(|file| sidecar_is_stale(project, file)) {
        return Ok(());
    }
    let Some(launcher) = &project.launcher else {
        return Ok(());
    };
    renpy_core::engine::run_json_dump(&project.root, &project.game_dir, launcher, "bake".into())
        .map_err(|e| format!("Ren'Py could not compile the edit before baking: {e}"))?;
    if overrides.iter().any(|file| sidecar_is_stale(project, file)) {
        return Err(
            "Ren'Py ran, but it did not compile the edited script, so the patch was not written. \
             The game ignores a .rpy that exists only inside an archive and runs the compiled .rpyc."
                .into(),
        );
    }
    Ok(())
}

fn modified_at(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// Launcher to run when edited scripts still need a fresh `.rpyc` before baking.
/// `None` when there is nothing to compile (or no launcher); `bake` then no-ops that step.
pub fn compile_target(
    project: &Project,
    backup_root: &Path,
) -> Result<Option<(std::path::PathBuf, std::path::PathBuf, renpy_core::Launcher)>, String> {
    let overrides: Vec<&renpy_core::project::SourceFile> = project
        .files
        .iter()
        .filter(|f| {
            matches!(f.origin, Origin::Override { .. })
                || edit::decompiled_marker_path(backup_root, &f.rel).is_file()
        })
        .collect();
    if overrides.is_empty() || !overrides.iter().any(|file| sidecar_is_stale(project, file)) {
        return Ok(None);
    }
    let Some(launcher) = &project.launcher else {
        return Ok(None);
    };
    Ok(Some((
        project.root.clone(),
        project.game_dir.clone(),
        launcher.clone(),
    )))
}

pub const TOGGLES_NAME: &str = "vnide_toggles.rpy";
pub const TOGGLES_COMPILED: &str = "vnide_toggles.rpyc";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModToggles {
    #[serde(default)]
    pub console: bool,
    #[serde(default)]
    pub developer: bool,
    #[serde(default)]
    pub quick_save_keys: bool,
    #[serde(default)]
    pub skip_unseen: bool,
    #[serde(default)]
    pub rollback: bool,
}

impl Default for ModToggles {
    fn default() -> Self {
        Self {
            console: false,
            developer: false,
            quick_save_keys: false,
            skip_unseen: false,
            rollback: false,
        }
    }
}

impl ModToggles {
    pub fn any(&self) -> bool {
        self.console || self.developer || self.quick_save_keys || self.skip_unseen || self.rollback
    }

    /// `init 999` so it runs after the game's own options.
    pub fn script(&self) -> String {
        let mut lines = vec!["init 999 python:".to_string()];
        if self.console {
            lines.push("    config.console = True".into());
        }
        if self.developer {
            lines.push("    config.developer = True".into());
        }
        if self.quick_save_keys {
            lines.push("    config.keymap['quick_save'] = ['K_F5']".into());
            lines.push("    config.keymap['quick_load'] = ['K_F9']".into());
        }
        if self.skip_unseen {
            lines.push("    config.allow_skipping = True".into());
            lines.push("    _preferences.skip_unseen = True".into());
        }
        if self.rollback {
            lines.push("    config.rollback_enabled = True".into());
        }
        if lines.len() == 1 {
            return String::new();
        }
        lines.push(String::new());
        lines.join("\n")
    }
}

pub fn toggles_path(app_data: &Path, project_root: &Path) -> PathBuf {
    let hash = edit::backup_root(app_data, project_root)
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    app_data.join("toggles").join(hash).with_extension("json")
}

pub fn load_toggles(app_data: &Path, project_root: &Path) -> ModToggles {
    let Ok(text) = fs::read_to_string(toggles_path(app_data, project_root)) else {
        return ModToggles::default();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

pub fn save_toggles(
    app_data: &Path,
    project_root: &Path,
    toggles: &ModToggles,
) -> Result<(), String> {
    let path = toggles_path(app_data, project_root);
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(toggles).map_err(|e| e.to_string())?;
    fs::write(path, text).map_err(|e| e.to_string())
}

/// Rebuild the patch from every loose override plus the entries already in it.
pub fn bake(
    project: &Project,
    undo_dir: &Path,
    backup_root: &Path,
    toggles: &ModToggles,
) -> Result<PatchReport, String> {
    let overrides: Vec<&renpy_core::project::SourceFile> = project
        .files
        .iter()
        .filter(|f| {
            matches!(f.origin, Origin::Override { .. })
                || edit::decompiled_marker_path(backup_root, &f.rel).is_file()
        })
        .collect();
    let rel = patch_rel(project);
    let patch_abs = safe_join(&project.game_dir, &rel)?;
    let had_toggles = patch_abs.is_file()
        && Archive::open(&patch_abs)
            .ok()
            .and_then(|archive| rpa::read_manifest(&archive).ok().flatten())
            .is_some_and(|m| m.entries.contains_key(TOGGLES_NAME));
    if overrides.is_empty() && !toggles.any() && !had_toggles {
        return Err("Nothing to bake. Saving an archived script writes a loose file; bake folds those into the patch.".into());
    }
    // The game never parses a .rpy that lives only inside an archive. It runs
    // the compiled .rpyc. Compile while the loose file is still on disk.
    let bases = capture_bases(project, &overrides);
    compile_overrides(project, &overrides)?;
    let _ = fs::remove_dir_all(undo_dir);
    fs::create_dir_all(undo_dir).map_err(|e| format!("Could not create the undo folder: {e}"))?;

    let had_previous = patch_abs.is_file();
    if had_previous {
        fs::copy(&patch_abs, undo_dir.join("previous.rpa"))
            .map_err(|e| format!("Could not back up the current patch: {e}"))?;
    }

    let mut carried: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let mut old_manifest: Option<PatchManifest> = None;
    if had_previous {
        let archive = Archive::open(&patch_abs).map_err(|e| e.to_string())?;
        old_manifest = rpa::read_manifest(&archive).map_err(|e| e.to_string())?;
        for name in archive.entries.keys() {
            if name == rpa::MANIFEST_NAME {
                continue;
            }
            let bytes = archive
                .read_entry(name, rpa::SCRIPT_MAX)
                .map_err(|e| format!("Could not read `{name}` from the patch: {e}"))?;
            carried.insert(name.clone(), bytes);
        }
    }

    let mut manifest = PatchManifest {
        version: 1,
        baked_at: rpa::utc_now(),
        entries: BTreeMap::new(),
    };
    if let Some(old) = &old_manifest {
        manifest.entries = old.entries.clone();
    }

    let mut undo_files = Vec::new();
    let files_dir = undo_dir.join("files");
    for file in &overrides {
        let bytes = fs::read(&file.abs)
            .map_err(|e| format!("Could not read {}: {e}", file.abs.display()))?;
        copy_aside(&files_dir, &file.rel, &bytes)?;
        undo_files.push(file.rel.clone());
        for marker in [
            edit::marker_path(backup_root, &file.rel),
            edit::decompiled_marker_path(backup_root, &file.rel),
        ] {
            if !marker.is_file() {
                continue;
            }
            let saved = undo_dir
                .join("markers")
                .join(marker.strip_prefix(backup_root).unwrap_or(marker.as_path()));
            if let Some(parent) = saved.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            fs::copy(&marker, &saved)
                .map_err(|e| format!("Could not back up the edit record for {}: {e}", file.rel))?;
        }
        carried.insert(file.rel.clone(), bytes);

        let side_rel = sidecar_rel(&file.rel);
        let side_abs = safe_join(&project.game_dir, &side_rel)?;
        if side_abs.is_file() {
            let keep = match (modified_at(&side_abs), modified_at(&file.abs)) {
                (Some(c), Some(r)) => c >= r,
                _ => false,
            };
            if keep {
                let side_bytes = fs::read(&side_abs)
                    .map_err(|e| format!("Could not read {}: {e}", side_abs.display()))?;
                copy_aside(&files_dir, &side_rel, &side_bytes)?;
                undo_files.push(side_rel.clone());
                carried.insert(side_rel, side_bytes);
            }
        }

        // The patch sorts first, so a file baked before reports the patch as its
        // archive. Keep what the first bake recorded: that is the real original.
        let previous = old_manifest
            .as_ref()
            .and_then(|m| m.entries.get(&file.rel))
            .filter(|e| !e.generated);
        let from_patch = file.origin.archive() == Some(rel.as_str());
        let reuse = previous.filter(|old| from_patch || old.source.is_none());
        let (source, base_crc32) = match reuse {
            Some(old) => (old.source.clone(), old.base_crc32),
            None => {
                let source = file
                    .origin
                    .archive()
                    .filter(|name| *name != rel)
                    .map(|s| s.to_string());
                let mut crc = source
                    .as_deref()
                    .and_then(|archive| project.archived_crc(archive, &file.rel));
                if crc.is_none() {
                    crc = bases.get(&file.rel).and_then(|snap| snap.crc);
                }
                (source, crc)
            }
        };
        if reuse.is_none() {
            if let (Some(crc), Some(text)) = (
                base_crc32,
                bases.get(&file.rel).and_then(|snap| snap.text.as_deref()),
            ) {
                write_base(backup_root, crc, &file.rel, text)?;
            }
        }
        manifest.entries.insert(
            file.rel.clone(),
            PatchEntry {
                source,
                base_crc32,
                baked_at: manifest.baked_at.clone(),
                generated: false,
            },
        );
    }

    // A loose .rpyc that was not fresh must not stay behind and shadow the patch.
    let mut drop_stale_rpyc = Vec::new();
    for file in &overrides {
        let side_rel = sidecar_rel(&file.rel);
        if carried.contains_key(&side_rel) {
            continue;
        }
        let side_abs = safe_join(&project.game_dir, &side_rel)?;
        if side_abs.is_file() {
            let bytes = fs::read(&side_abs).unwrap_or_default();
            copy_aside(&files_dir, &side_rel, &bytes)?;
            undo_files.push(side_rel.clone());
            drop_stale_rpyc.push(side_abs);
            carried.remove(&side_rel);
        }
    }

    apply_toggles(&mut carried, &mut manifest, toggles, &project.game_dir)?;

    let manifest_bytes = manifest.to_bytes().map_err(|e| e.to_string())?;
    let op = UndoOp {
        kind: "bake".into(),
        patch_rel: rel.clone(),
        had_previous,
        files: undo_files,
    };
    fs::write(
        undo_dir.join("op.json"),
        serde_json::to_vec(&op).map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("Could not write the undo record: {e}"))?;

    let count = carried.len() as u32;
    let mut writer = ArchiveWriter::create(&patch_abs).map_err(|e| e.to_string())?;
    for (name, bytes) in &carried {
        writer.add_bytes(name, bytes).map_err(|e| e.to_string())?;
    }
    writer
        .add_bytes(rpa::MANIFEST_NAME, &manifest_bytes)
        .map_err(|e| e.to_string())?;
    if let Err(e) = writer.finish() {
        let _ = fs::remove_dir_all(undo_dir);
        return Err(e.to_string());
    }

    for file in &overrides {
        let _ = fs::remove_file(&file.abs);
        let _ = fs::remove_file(edit::marker_path(backup_root, &file.rel));
        let _ = fs::remove_file(edit::decompiled_marker_path(backup_root, &file.rel));
    }
    for path in &drop_stale_rpyc {
        let _ = fs::remove_file(path);
    }
    // Fresh sidecars were copied into the patch; remove the loose copies too.
    for name in carried.keys() {
        if name.ends_with(".rpyc") || name.ends_with(".rpymc") {
            if let Ok(path) = safe_join(&project.game_dir, name) {
                let _ = fs::remove_file(path);
            }
        }
    }
    Ok(PatchReport {
        patch: rel,
        files: count,
        action: "bake".into(),
    })
}

fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    for entry in fs::read_dir(from).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let dest = to.join(entry.file_name());
        if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
            copy_tree(&entry.path(), &dest)?;
        } else {
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            fs::copy(entry.path(), &dest).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn copy_aside(files_dir: &Path, rel: &str, bytes: &[u8]) -> Result<(), String> {
    let dest = safe_join(files_dir, rel)?;
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Could not create {}: {e}", parent.display()))?;
    }
    fs::write(&dest, bytes).map_err(|e| format!("Could not back up {rel}: {e}"))
}

pub fn undo(project: &Project, undo_dir: &Path, backup_root: &Path) -> Result<PatchReport, String> {
    let text = fs::read_to_string(undo_dir.join("op.json"))
        .map_err(|_| "Nothing to undo. Bake or remove a patch first.".to_string())?;
    let op: UndoOp =
        serde_json::from_str(&text).map_err(|e| format!("The undo record is damaged: {e}"))?;
    let patch_abs = safe_join(&project.game_dir, &op.patch_rel)?;
    if op.had_previous {
        let previous = undo_dir.join("previous.rpa");
        if !previous.is_file() {
            return Err("The backed-up patch is missing, so it cannot be restored.".into());
        }
        fs::copy(&previous, &patch_abs).map_err(|e| format!("Could not restore the patch: {e}"))?;
    } else if patch_abs.is_file() {
        fs::remove_file(&patch_abs).map_err(|e| format!("Could not remove the patch: {e}"))?;
    }
    for rel in &op.files {
        let src = safe_join(&undo_dir.join("files"), rel)?;
        let dest = safe_join(&project.game_dir, rel)?;
        if !src.is_file() {
            continue;
        }
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        fs::copy(&src, &dest).map_err(|e| format!("Could not restore {rel}: {e}"))?;
    }
    let markers = undo_dir.join("markers");
    if markers.is_dir() {
        copy_tree(&markers, backup_root)?;
    }
    let _ = fs::remove_dir_all(undo_dir);
    Ok(PatchReport {
        patch: op.patch_rel,
        files: op.files.len() as u32,
        action: "undo".into(),
    })
}

pub fn remove(project: &Project, undo_dir: &Path) -> Result<PatchReport, String> {
    let rel = project
        .archives
        .iter()
        .find(|a| a.info.is_patch)
        .map(|a| a.info.path.clone())
        .ok_or_else(|| "There is no patch archive to remove.".to_string())?;
    let patch_abs = safe_join(&project.game_dir, &rel)?;
    if !patch_abs.is_file() {
        return Err(format!("`{rel}` is not in the game folder."));
    }
    let _ = fs::remove_dir_all(undo_dir);
    fs::create_dir_all(undo_dir).map_err(|e| e.to_string())?;
    fs::copy(&patch_abs, undo_dir.join("previous.rpa"))
        .map_err(|e| format!("Could not back up the patch: {e}"))?;
    let op = UndoOp {
        kind: "remove".into(),
        patch_rel: rel.clone(),
        had_previous: true,
        files: Vec::new(),
    };
    fs::write(
        undo_dir.join("op.json"),
        serde_json::to_vec(&op).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    fs::remove_file(&patch_abs).map_err(|e| format!("Could not remove {rel}: {e}"))?;
    Ok(PatchReport {
        patch: rel,
        files: 0,
        action: "remove".into(),
    })
}

struct CapturedBase {
    crc: Option<u32>,
    text: Option<String>,
}

fn capture_bases(
    project: &Project,
    overrides: &[&renpy_core::project::SourceFile],
) -> BTreeMap<String, CapturedBase> {
    let mut out = BTreeMap::new();
    for file in overrides {
        let captured = match file.origin.archive() {
            Some(archive) => capture_archived(project, archive, &file.rel),
            None => capture_loose(project, &file.rel),
        };
        out.insert(file.rel.clone(), captured);
    }
    out
}

fn capture_archived(project: &Project, archive: &str, rel: &str) -> CapturedBase {
    let crc = project.archived_crc(archive, rel);
    let text = archived_text(project, archive, rel);
    CapturedBase { crc, text }
}

fn capture_loose(project: &Project, rel: &str) -> CapturedBase {
    let side = sidecar_rel(rel);
    let Ok(path) = safe_join(&project.game_dir, &side) else {
        return CapturedBase { crc: None, text: None };
    };
    let Ok(bytes) = fs::read(path) else {
        return CapturedBase { crc: None, text: None };
    };
    let text = renpy_core::rpyc::decompile(&bytes).ok().map(|got| got.text);
    CapturedBase {
        crc: Some(crc32fast::hash(&bytes)),
        text,
    }
}

fn archived_text(project: &Project, archive: &str, rel: &str) -> Option<String> {
    let loaded = project
        .archives
        .iter()
        .find(|item| item.info.path == archive)?
        .archive
        .as_ref()?;
    if rel.ends_with(".rpy") || rel.ends_with(".rpym") {
        if let Ok(bytes) = loaded.read_entry(rel, rpa::SCRIPT_MAX) {
            if let Ok(text) = String::from_utf8(bytes) {
                return Some(text);
            }
        }
    }
    let side = sidecar_rel(rel);
    let bytes = loaded.read_entry(&side, rpa::SCRIPT_MAX).ok()?;
    renpy_core::rpyc::decompile(&bytes).ok().map(|got| got.text)
}

pub fn base_snapshot_path(backup_root: &Path, crc: u32, rel: &str) -> PathBuf {
    backup_root
        .join("bases")
        .join(format!("{crc:08x}"))
        .join(rel)
}

fn write_base(backup_root: &Path, crc: u32, rel: &str, text: &str) -> Result<(), String> {
    if rel.is_empty() || rel.contains("..") || rel.contains('\0') {
        return Err(format!("Refusing to store a base snapshot for `{rel}`."));
    }
    let path = base_snapshot_path(backup_root, crc, rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(path, text).map_err(|e| e.to_string())
}

fn read_base(backup_root: &Path, crc: u32, rel: &str) -> Option<String> {
    fs::read_to_string(base_snapshot_path(backup_root, crc, rel)).ok()
}

/// Where the toggles script is staged, loose, while the engine compiles it.
pub fn toggles_loose_paths(game_dir: &Path) -> (PathBuf, PathBuf) {
    (
        game_dir.join(TOGGLES_NAME),
        game_dir.join(TOGGLES_COMPILED),
    )
}

/// The game ignores a `.rpy` that lives only in an archive and runs the `.rpyc`.
/// So the toggles script is written loose, compiled by the engine, and the
/// compiled file is what `bake` folds into the patch.
pub fn compile_toggles(
    project_root: &Path,
    game_dir: &Path,
    launcher: Option<&renpy_core::Launcher>,
    toggles: &ModToggles,
) -> Result<(), String> {
    let script = toggles.script();
    if script.is_empty() {
        return Ok(());
    }
    let Some(launcher) = launcher else {
        return Err(
            "The mod toggles need Ren'Py to compile them. Pick a launcher or SDK first.".into(),
        );
    };
    let (rpy, rpyc) = toggles_loose_paths(game_dir);
    if rpy.exists() || rpyc.exists() {
        return Err(format!(
            "`{TOGGLES_NAME}` or `{TOGGLES_COMPILED}` already exists in game/. Move it before baking the toggles."
        ));
    }
    let outcome = (|| {
        fs::write(&rpy, script).map_err(|e| format!("Could not stage the toggles script: {e}"))?;
        renpy_core::engine::run_json_dump(project_root, game_dir, launcher, "bake".into())
            .map_err(|e| format!("Ren'Py could not compile the toggles: {e}"))?;
        if !rpyc.is_file() {
            return Err("Ren'Py ran, but it did not compile the toggles script.".to_string());
        }
        Ok(())
    })();
    if outcome.is_err() {
        remove_toggles_loose(game_dir);
    }
    outcome
}

/// Remove the staged toggles script and its compiled file from `game/`.
pub fn remove_toggles_loose(game_dir: &Path) {
    let (rpy, rpyc) = toggles_loose_paths(game_dir);
    let _ = fs::remove_file(rpy);
    let _ = fs::remove_file(rpyc);
}

fn apply_toggles(
    carried: &mut BTreeMap<String, Vec<u8>>,
    manifest: &mut PatchManifest,
    toggles: &ModToggles,
    game_dir: &Path,
) -> Result<(), String> {
    carried.remove(TOGGLES_NAME);
    carried.remove(TOGGLES_COMPILED);
    manifest.entries.remove(TOGGLES_NAME);
    let script = toggles.script();
    if script.is_empty() {
        return Ok(());
    }
    let (_, rpyc) = toggles_loose_paths(game_dir);
    let compiled = fs::read(&rpyc).map_err(|_| {
        format!("`{TOGGLES_COMPILED}` was not compiled, so the toggles cannot be baked.")
    })?;
    carried.insert(TOGGLES_COMPILED.to_string(), compiled);
    carried.insert(TOGGLES_NAME.to_string(), script.into_bytes());
    manifest.entries.insert(
        TOGGLES_NAME.to_string(),
        PatchEntry {
            source: None,
            base_crc32: None,
            baked_at: manifest.baked_at.clone(),
            generated: true,
        },
    );
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RebaseResult {
    pub base: String,
    pub upstream: String,
    pub mine: String,
    pub merged: String,
    pub conflicts: u32,
    pub base_missing: bool,
}

pub fn rebase(project: &Project, backup_root: &Path, rel: &str) -> Result<RebaseResult, String> {
    if rel.is_empty() || rel.contains("..") || rel.contains('\0') {
        return Err("That path is not a script in this game.".into());
    }
    let loaded = project
        .archives
        .iter()
        .find(|item| item.info.is_patch)
        .and_then(|item| item.archive.as_ref())
        .ok_or_else(|| "There is no patch to rebase.".to_string())?;
    let manifest = rpa::read_manifest(loaded)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "The patch has no manifest.".to_string())?;
    let entry = manifest
        .entries
        .get(rel)
        .ok_or_else(|| format!("`{rel}` is not in the patch."))?;
    let mine = mine_text(project, loaded, rel)?;
    let upstream = upstream_text(project, entry, rel);
    let base = entry
        .base_crc32
        .and_then(|crc| read_base(backup_root, crc, rel));
    let base_missing = base.is_none();
    let (merged, conflicts) = match &base {
        Some(text) => merge3(text, &mine, &upstream),
        None => (String::new(), 0),
    };
    Ok(RebaseResult {
        base: base.unwrap_or_default(),
        upstream,
        mine,
        merged,
        conflicts,
        base_missing,
    })
}

fn mine_text(project: &Project, patch: &Archive, rel: &str) -> Result<String, String> {
    if let Ok(path) = safe_join(&project.game_dir, rel) {
        if path.is_file() {
            return fs::read_to_string(path)
                .map_err(|e| format!("Could not read the loose copy of {rel}: {e}"));
        }
    }
    let bytes = patch
        .read_entry(rel, rpa::SCRIPT_MAX)
        .map_err(|e| format!("Could not read `{rel}` from the patch: {e}"))?;
    String::from_utf8(bytes).map_err(|_| format!("`{rel}` in the patch is not text."))
}

fn upstream_text(project: &Project, entry: &PatchEntry, rel: &str) -> String {
    if let Some(archive) = entry.source.as_deref() {
        if let Some(text) = archived_text(project, archive, rel) {
            return text;
        }
    }
    let side = sidecar_rel(rel);
    let Ok(path) = safe_join(&project.game_dir, &side) else {
        return String::new();
    };
    let Ok(bytes) = fs::read(path) else {
        return String::new();
    };
    renpy_core::rpyc::decompile(&bytes)
        .map(|got| got.text)
        .unwrap_or_default()
}

fn merge3(base: &str, mine: &str, upstream: &str) -> (String, u32) {
    match diffy::merge(base, mine, upstream) {
        Ok(text) => (text, 0),
        Err(conflicted) => {
            let n = conflicted.matches("<<<<<<<").count() as u32;
            (conflicted, n.max(1))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::{self, EditState};
    use renpy_core::engine::run_json_dump;
    use renpy_core::Launcher;
    use std::process::Command;

    #[test]
    fn toggles_script_lists_only_what_is_on() {
        let mut toggles = ModToggles::default();
        assert!(toggles.script().is_empty());
        toggles.console = true;
        toggles.rollback = true;
        let script = toggles.script();
        assert!(script.starts_with("init 999 python:"));
        assert!(script.contains("config.console = True"));
        assert!(script.contains("config.rollback_enabled = True"));
        assert!(!script.contains("config.developer"));
    }

    #[test]
    fn toggles_bake_their_compiled_file_and_turning_them_off_removes_it() {
        let root = std::env::temp_dir().join(format!("vn-ide-toggles-{}", std::process::id()));
        let game = root.join("game");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&game).unwrap();
        fs::write(game.join("script.rpy"), "label start:\n    return\n").unwrap();
        let project = Project::open(&root).unwrap();
        let backups = root.join("backups");
        let undo = root.join("undo");
        let mut on = ModToggles::default();
        on.console = true;

        // The engine has not compiled the script, so there is nothing to fold in.
        let err = bake(&project, &undo, &backups, &on).unwrap_err();
        assert!(err.contains("was not compiled"), "{err}");

        fs::write(game.join(TOGGLES_COMPILED), b"compiled").unwrap();
        let report = bake(&project, &undo, &backups, &on).unwrap();
        assert!(!game.join(TOGGLES_COMPILED).exists(), "the loose rpyc is folded in");
        let patch = Archive::open(&game.join(&report.patch)).unwrap();
        assert_eq!(patch.read_entry(TOGGLES_COMPILED, 1024).unwrap(), b"compiled");
        let text = String::from_utf8(patch.read_entry(TOGGLES_NAME, 1024).unwrap()).unwrap();
        assert!(text.contains("config.console = True"));
        let manifest = rpa::read_manifest(&patch).unwrap().unwrap();
        assert!(manifest.entries[TOGGLES_NAME].generated);

        let mut project = Project::open(&root).unwrap();
        project.refresh_archives();
        bake(&project, &undo, &backups, &ModToggles::default()).unwrap();
        let patch = Archive::open(&game.join(&report.patch)).unwrap();
        assert!(patch.get(TOGGLES_NAME).is_none());
        assert!(patch.get(TOGGLES_COMPILED).is_none());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn three_way_merge_marks_a_conflict_and_keeps_a_clean_edit() {
        let (clean, n) = merge3("line\n", "line\nmine\n", "line\n");
        assert_eq!(n, 0);
        assert!(clean.contains("mine"));
        let (conflicted, n) = merge3("line\n", "line\nmine\n", "line\ntheirs\n");
        assert!(n >= 1);
        assert!(conflicted.contains("<<<<<<<"));
    }

    #[test]
    fn bake_puts_the_edit_in_a_patch_and_undo_restores_the_loose_file() {
        let root = std::env::temp_dir().join(format!("vn-ide-patch-{}", std::process::id()));
        let game = root.join("game");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&game).unwrap();
        let mut writer = ArchiveWriter::create(&game.join("scripts.rpa")).unwrap();
        writer
            .add_bytes("chapter.rpy", b"label start:\n    return\n")
            .unwrap();
        writer.finish().unwrap();
        let mut project = Project::open(&root).unwrap();
        let mut edit = EditState::default();
        let backups = root.join("backups");
        edit::apply_write(
            &mut project,
            &mut edit,
            &backups,
            "chapter.rpy",
            "label start:\n    jump shop\n",
        )
        .unwrap();
        let undo_dir = root.join("undo");
        let report = bake(&project, &undo_dir, &backups, &ModToggles::default()).unwrap();
        assert!(report.patch.contains("vnide_patch"));
        assert!(!game.join("chapter.rpy").exists());
        project.refresh_archives();
        let text = String::from_utf8(project.read_script_bytes("chapter.rpy").unwrap()).unwrap();
        assert!(text.contains("jump shop"), "{text}");
        assert_eq!(
            project
                .file_index("chapter.rpy")
                .map(|i| project.files[i].origin.kind()),
            Some("archived")
        );

        undo(&project, &undo_dir, &backups).unwrap();
        assert!(game.join("chapter.rpy").is_file());
        assert!(!undo_dir.exists());
        let _ = fs::remove_dir_all(root);
    }

    /// Bakes one override and asks that engine what it actually loaded.
    ///
    /// Skipped unless `RENPY7_SDK` and/or `RENPY8_SDK` point at a Ren'Py
    /// runtime (the folder with `renpy/`, `lib/` and the game launcher). The
    /// runtime is copied into a temp folder. The real game is never written.
    /// Ren'Py's own `--json-dump` drops labels whose `.rpy` is not a loose
    /// file, so the patched script records the bytes `renpy.loader.load`
    /// returned. That is the engine's view of the patch.
    #[test]
    fn engine_loads_the_baked_patch() {
        let mut ran = 0u32;
        for (key, which) in [("RENPY7_SDK", "renpy7"), ("RENPY8_SDK", "renpy8")] {
            let Ok(sdk) = std::env::var(key) else {
                continue;
            };
            let sdk = PathBuf::from(sdk);
            if !sdk.join("renpy").is_dir() || !sdk.join("lib").is_dir() {
                panic!("{key} is not a Ren'Py runtime: {}", sdk.display());
            }
            ran += 1;
            engine_check(&sdk, which);
        }
        if ran == 0 {
            eprintln!("RENPY7_SDK and RENPY8_SDK are unset; skipping the engine check");
        }
    }

    /// Compiles the toggles with the engine, bakes them, and has the game read
    /// its own settings after the patch's `init 999` block ran.
    #[test]
    fn engine_applies_baked_toggles() {
        let mut ran = 0u32;
        for (key, which) in [("RENPY7_SDK", "renpy7"), ("RENPY8_SDK", "renpy8")] {
            let Ok(sdk) = std::env::var(key) else {
                continue;
            };
            let sdk = PathBuf::from(sdk);
            ran += 1;
            toggles_engine_check(&sdk, which);
        }
        if ran == 0 {
            eprintln!("RENPY7_SDK and RENPY8_SDK are unset; skipping the toggles engine check");
        }
    }

    fn toggles_engine_check(sdk: &Path, which: &str) {
        let work = std::env::temp_dir()
            .join(format!("vn-ide-toggles-engine-{}-{}", which, std::process::id()));
        let _guard = Stage::create(&work);
        let sdk_copy = work.join("sdk");
        stage_runtime(sdk, &sdk_copy).unwrap_or_else(|e| panic!("{which}: {e}"));
        let (exe, _) = launcher_pair(&sdk_copy).unwrap_or_else(|e| panic!("{which}: {e}"));

        let proj = work.join("proj");
        let game = proj.join("game");
        fs::create_dir_all(&game).unwrap();
        let mark = work.join("settings.txt");
        let mark_py = mark.display().to_string().replace('\\', "/");
        // Runs after the patch's `init 999` block.
        let probe = format!(
            "label start:\n    return\n\ninit 1000 python:\n    f = open(\"{mark_py}\", \"w\")\n    f.write(repr((bool(config.console), bool(config.developer), bool(config.rollback_enabled), list(config.keymap.get(\"quick_save\", [])), list(config.keymap.get(\"quick_load\", [])))))\n    f.close()\n"
        );
        fs::write(game.join("script.rpy"), probe).unwrap();

        let launcher = Launcher {
            exe: exe.clone(),
            prefix_args: vec![proj.to_string_lossy().into_owned()],
        };
        let off = ModToggles::default();
        let mut on = ModToggles::default();
        on.console = true;
        on.developer = true;
        on.quick_save_keys = true;
        on.rollback = true;

        run_json_dump(&proj, &game, &launcher, "before".into()).unwrap_or_else(|e| {
            panic!("{which}: engine dump failed: {e}\n{}", engine_log(&exe, &proj))
        });
        let before = fs::read_to_string(&mark).unwrap_or_else(|e| panic!("{which}: no probe output: {e}"));
        let _ = fs::remove_file(&mark);

        compile_toggles(&proj, &game, Some(&launcher), &on).unwrap_or_else(|e| {
            panic!("{which}: {e}\n{}", engine_log(&exe, &proj))
        });
        let project = Project::open(&proj).unwrap();
        let backups = work.join("backups");
        let undo = work.join("undo");
        bake(&project, &undo, &backups, &on).unwrap_or_else(|e| panic!("{which}: {e}"));
        remove_toggles_loose(&game);
        assert!(!game.join(TOGGLES_NAME).exists() && !game.join(TOGGLES_COMPILED).exists());

        run_json_dump(&proj, &game, &launcher, "after".into()).unwrap_or_else(|e| {
            panic!("{which}: engine dump of the patch failed: {e}\n{}", engine_log(&exe, &proj))
        });
        let after = fs::read_to_string(&mark)
            .unwrap_or_else(|e| panic!("{which}: no probe output after baking: {e}"));
        let _ = fs::remove_file(&mark);
        eprintln!("{which}: settings before {before}\n{which}: settings after  {after}");
        assert!(after.starts_with("(True, True, True, ['K_F5']"), "{which}: {after}");
        assert!(after.contains("['K_F9']"), "{which}: {after}");
        assert_ne!(before, after, "{which}: the toggles changed nothing");

        // Turn them off and bake again: the game goes back to its own settings.
        bake(&Project::open(&proj).unwrap(), &undo, &backups, &off)
            .unwrap_or_else(|e| panic!("{which}: {e}"));
        run_json_dump(&proj, &game, &launcher, "off".into()).unwrap();
        let reverted = fs::read_to_string(&mark).unwrap();
        assert_eq!(reverted, before, "{which}: toggles stayed on after being turned off");
    }

    fn engine_check(sdk: &Path, which: &str) {
        let work =
            std::env::temp_dir().join(format!("vn-ide-engine-{}-{}", which, std::process::id()));
        let _guard = Stage::create(&work);
        let sdk_copy = work.join("sdk");
        stage_runtime(sdk, &sdk_copy).unwrap_or_else(|e| panic!("{which}: {e}"));
        let (exe, _) = launcher_pair(&sdk_copy).unwrap_or_else(|e| panic!("{which}: {e}"));

        let proj = work.join("proj");
        let game = proj.join("game");
        fs::create_dir_all(&game).unwrap();
        fs::write(game.join("script.rpy"), "label start:\n    return\n").unwrap();
        let mut writer = ArchiveWriter::create(&game.join("scripts.rpa")).unwrap();
        writer
            .add_bytes(
                "chapter.rpy",
                b"label vnide_check:\n    \"VNIDE_ORIGINAL\"\n",
            )
            .unwrap();
        writer.finish().unwrap();

        let mark = work.join("loaded.bin");
        let mark_py = mark.display().to_string().replace('\\', "/");
        let patched = format!(
            "label vnide_check:\n    \"VNIDE_PATCHED_TOKEN\"\n\ninit python:\n    p = \"{mark_py}\"\n    try:\n        raw = renpy.loader.load(\"chapter.rpy\").read()\n        if not isinstance(raw, bytes):\n            raw = raw.encode(\"utf-8\")\n        f = open(p, \"wb\")\n        f.write(raw)\n        f.close()\n    except Exception as e:\n        f = open(p + \".err\", \"w\")\n        f.write(repr(e))\n        f.close()\n"
        );

        let mut project = Project::open(&proj).unwrap();
        let mut edit = EditState::default();
        let backups = work.join("backups");
        edit::apply_write(&mut project, &mut edit, &backups, "chapter.rpy", &patched).unwrap();

        let launcher = Launcher {
            exe: exe.clone(),
            prefix_args: vec![proj.to_string_lossy().into_owned()],
        };
        // The loose override is on disk, so the dump can see the edited label,
        // and the engine compiles chapter.rpyc beside it.
        let loose = run_json_dump(&proj, &game, &launcher, "loose".into()).unwrap_or_else(|e| {
            panic!(
                "{which}: engine dump of the override failed: {e}\n{}",
                engine_log(&exe, &proj)
            );
        });
        if !loose.dump.labels.is_empty() {
            let (file, _) = loose.dump.labels.get("vnide_check").unwrap_or_else(|| {
                panic!(
                    "{which}: dump did not report vnide_check: {:?}",
                    loose.dump.labels
                )
            });
            assert!(
                file.replace('\\', "/").ends_with("chapter.rpy"),
                "{which}: edited label came from {file}"
            );
        }
        assert!(
            game.join("chapter.rpyc").is_file(),
            "{which}: the engine did not compile the override"
        );
        let _ = fs::remove_file(&mark);
        let _ = fs::remove_file(mark.with_extension("bin.err"));

        let undo_dir = work.join("undo");
        let report = bake(&project, &undo_dir, &backups, &ModToggles::default()).unwrap();
        assert!(
            game.join(&report.patch).is_file(),
            "{which}: patch was not written"
        );
        assert!(
            !game.join("chapter.rpy").exists(),
            "{which}: bake left the loose override in place"
        );
        assert!(
            !game.join("chapter.rpyc").exists(),
            "{which}: bake left the loose rpyc in place"
        );

        let run = run_json_dump(&proj, &game, &launcher, "patch".into()).unwrap_or_else(|e| {
            panic!(
                "{which}: engine dump of the patch failed: {e}\n{}",
                engine_log(&exe, &proj)
            );
        });
        let err = fs::read_to_string(mark.with_extension("bin.err")).unwrap_or_default();
        let loaded = fs::read(&mark).unwrap_or_else(|_| {
            let detail = engine_log(&launcher.exe, &proj);
            panic!(
                "{which}: the engine ran ({}) but did not load chapter.rpy from the patch.\nlabels: {:?}\n{err}\n{detail}",
                run.dump.version, run.dump.labels
            )
        });
        let loaded = String::from_utf8_lossy(&loaded);
        assert!(
            loaded.contains("VNIDE_PATCHED_TOKEN"),
            "{which}: engine loaded something else:\n{loaded}\n{err}"
        );
        assert!(
            !loaded.contains("VNIDE_ORIGINAL"),
            "{which}: engine loaded the original archive, not the patch:\n{loaded}"
        );
        if let Some((file, _)) = run.dump.labels.get("vnide_check") {
            assert!(
                file.replace('\\', "/").ends_with("chapter.rpy"),
                "{which}: {file}"
            );
        }
        eprintln!("{which}: {} loaded the baked patch", run.dump.version);
    }

    /// Compiles a fixture with the game's engine, decompiles it, compiles that
    /// text again, and requires the same statement sequence. The real game is
    /// never written. Also stores the first `.rpyc` as a fixture when that
    /// file is not already in the tree.
    #[test]
    fn decompiled_source_round_trips_through_the_engine() {
        let mut ran = 0u32;
        for (key, which) in [("RENPY7_SDK", "7"), ("RENPY8_SDK", "8")] {
            let Ok(sdk) = std::env::var(key) else {
                continue;
            };
            let sdk = PathBuf::from(sdk);
            if !sdk.join("renpy").is_dir() || !sdk.join("lib").is_dir() {
                panic!("{key} is not a Ren'Py runtime: {}", sdk.display());
            }
            ran += 1;
            round_trip_decompile(&sdk, which);
        }
        if ran == 0 {
            eprintln!("RENPY7_SDK and RENPY8_SDK are unset; skipping the decompile round trip");
        }
    }

    fn round_trip_decompile(sdk: &Path, which: &str) {
        let work =
            std::env::temp_dir().join(format!("vn-ide-rpyc-{}-{}", which, std::process::id()));
        let _guard = Stage::create(&work);
        let sdk_copy = work.join("sdk");
        stage_runtime(sdk, &sdk_copy).unwrap_or_else(|e| panic!("{which}: {e}"));
        let (exe, _) = launcher_pair(&sdk_copy).unwrap_or_else(|e| panic!("{which}: {e}"));

        let src = include_str!("../renpy-core/tests/fixtures/rpyc/src/script.rpy");
        let proj = work.join("proj");
        let game = proj.join("game");
        fs::create_dir_all(&game).unwrap();
        fs::write(game.join("script.rpy"), src).unwrap();
        let launcher = Launcher {
            exe: exe.clone(),
            prefix_args: vec![proj.to_string_lossy().into_owned()],
        };
        run_json_dump(&proj, &game, &launcher, "first".into()).unwrap_or_else(|e| {
            panic!(
                "{which}: the engine did not compile the fixture: {e}\n{}",
                engine_log(&exe, &proj)
            );
        });
        let rpyc_path = game.join("script.rpyc");
        assert!(rpyc_path.is_file(), "{which}: script.rpyc was not written");
        let original = fs::read(&rpyc_path).unwrap();
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("renpy-core/tests/fixtures/rpyc")
            .join(which)
            .join("script.rpyc");
        if !fixture.is_file() {
            fs::create_dir_all(fixture.parent().unwrap()).unwrap();
            fs::write(&fixture, &original).unwrap();
        }

        let got = renpy_core::rpyc::decompile(&original).unwrap_or_else(|e| panic!("{which}: {e}"));
        assert!(
            got.editable,
            "{which}: decompiled fixture is not editable: {:?}\n{}",
            got.reasons, got.text
        );
        let seq1 = renpy_core::rpyc::node_sequence(&original).unwrap();

        let proj2 = work.join("proj2");
        let game2 = proj2.join("game");
        fs::create_dir_all(&game2).unwrap();
        fs::write(game2.join("script.rpy"), &got.text).unwrap();
        let launcher2 = Launcher {
            exe: exe.clone(),
            prefix_args: vec![proj2.to_string_lossy().into_owned()],
        };
        run_json_dump(&proj2, &game2, &launcher2, "second".into()).unwrap_or_else(|e| {
            panic!(
                "{which}: the engine rejected the decompiled source: {e}\n{}\n----\n{}",
                engine_log(&exe, &proj2),
                got.text
            );
        });
        let again = fs::read(game2.join("script.rpyc")).unwrap_or_else(|_| {
            let _ = fs::write(
                std::env::temp_dir().join("vn-ide-decompiled.rpy"),
                &got.text,
            );
            let listing = fs::read_dir(&game2)
                .map(|rd| {
                    rd.flatten()
                        .map(|e| e.file_name().to_string_lossy().into_owned())
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            panic!("{which}: second compile wrote no rpyc ({listing})");
        });
        let seq2 = renpy_core::rpyc::node_sequence(&again).unwrap();
        assert_eq!(
            seq1, seq2,
            "{which}: node sequence changed after recompiling\n{}",
            got.text
        );
        eprintln!(
            "{which}: decompiled source round-tripped ({} nodes)",
            seq1.len()
        );
    }

    fn engine_log(exe: &Path, proj: &Path) -> String {
        use std::process::Stdio;
        let out_path = proj.with_file_name("engine-out.txt");
        let err_path = proj.with_file_name("engine-err.txt");
        let dump = proj.with_file_name("engine-dump.json");
        let stdout = fs::File::create(&out_path).ok();
        let stderr = fs::File::create(&err_path).ok();
        let mut cmd = Command::new(exe);
        cmd.arg(proj)
            .arg("--json-dump")
            .arg(&dump)
            .arg("quit")
            .current_dir(proj)
            .env("PYTHONDONTWRITEBYTECODE", "1");
        if let Some(file) = stdout {
            cmd.stdout(Stdio::from(file));
        }
        if let Some(file) = stderr {
            cmd.stderr(Stdio::from(file));
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000);
        }
        let status = cmd
            .status()
            .map(|s| s.to_string())
            .unwrap_or_else(|e| e.to_string());
        let stdout = fs::read_to_string(&out_path).unwrap_or_default();
        let stderr = fs::read_to_string(&err_path).unwrap_or_default();
        let log = fs::read_to_string(proj.join("log.txt")).unwrap_or_default();
        let trace = fs::read_to_string(proj.join("traceback.txt")).unwrap_or_default();
        format!("status {status}\nstdout:\n{stdout}\nstderr:\n{stderr}\nlog:\n{log}\ntraceback:\n{trace}")
    }

    /// Removes a `lib` junction before deleting the tree, so a cleanup cannot
    /// follow the junction into the real game.
    struct Stage {
        work: PathBuf,
    }

    impl Stage {
        fn create(work: &Path) -> Stage {
            remove_stage(work);
            fs::create_dir_all(work).unwrap();
            Stage {
                work: work.to_path_buf(),
            }
        }
    }

    impl Drop for Stage {
        fn drop(&mut self) {
            remove_stage(&self.work);
        }
    }

    /// Drops the `lib` junction first. A recursive delete must not run while
    /// that junction still exists, or it would delete the real runtime.
    fn remove_stage(work: &Path) {
        if !work.exists() {
            return;
        }
        let lib = work.join("sdk").join("lib");
        let _ = fs::remove_dir(&lib);
        if lib.exists() {
            return;
        }
        let path = work.to_string_lossy().to_string();
        for _ in 0..15 {
            let _ = Command::new("cmd")
                .args(["/c", "rmdir", "/s", "/q", &path])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
            if !work.exists() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
    }

    fn stage_runtime(src: &Path, dest: &Path) -> Result<(), String> {
        fs::create_dir_all(dest).map_err(|e| e.to_string())?;
        let (exe, py) = launcher_pair(src)?;
        fs::copy(&exe, dest.join(exe.file_name().unwrap())).map_err(|e| e.to_string())?;
        fs::copy(&py, dest.join(py.file_name().unwrap())).map_err(|e| e.to_string())?;
        let from = src.join("renpy");
        let to = dest.join("renpy");
        let status = Command::new("robocopy")
            .arg(&from)
            .arg(&to)
            .args(["/E", "/NFL", "/NDL", "/NJH", "/NJS", "/NC", "/NS"])
            .status()
            .map_err(|e| format!("robocopy: {e}"))?;
        if status.code().unwrap_or(16) >= 8 {
            return Err(format!("could not copy the engine ({})", status));
        }
        let link_at = dest.join("lib");
        let link_to = src.join("lib");
        let link = Command::new("cmd")
            .args([
                "/c",
                "mklink",
                "/J",
                link_at.to_str().unwrap_or(""),
                link_to.to_str().unwrap_or(""),
            ])
            .status()
            .map_err(|e| format!("mklink: {e}"))?;
        if !link.success() {
            return Err("could not link the engine runtime".into());
        }
        Ok(())
    }

    fn launcher_pair(sdk: &Path) -> Result<(PathBuf, PathBuf), String> {
        let mut found = None;
        for entry in fs::read_dir(sdk).map_err(|e| e.to_string())? {
            let path = entry.map_err(|e| e.to_string())?.path();
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if !ext.eq_ignore_ascii_case("exe") {
                continue;
            }
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_lowercase();
            if name.starts_with("python") || name.starts_with("unins") {
                continue;
            }
            let py = path.with_extension("py");
            if py.is_file() {
                found = Some((path, py));
                break;
            }
        }
        found.ok_or_else(|| format!("no launcher next to {}", sdk.display()))
    }
}
