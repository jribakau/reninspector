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

/// Rebuild the patch from every loose override plus the entries already in it.
pub fn bake(project: &Project, undo_dir: &Path, backup_root: &Path) -> Result<PatchReport, String> {
    let overrides: Vec<&renpy_core::project::SourceFile> = project
        .files
        .iter()
        .filter(|f| {
            matches!(f.origin, Origin::Override { .. })
                || edit::decompiled_marker_path(backup_root, &f.rel).is_file()
        })
        .collect();
    if overrides.is_empty() {
        return Err("Nothing to bake. Saving an archived script writes a loose file; bake folds those into the patch.".into());
    }
    // The game never parses a .rpy that lives only inside an archive. It runs
    // the compiled .rpyc. Compile while the loose file is still on disk.
    compile_overrides(project, &overrides)?;
    let rel = patch_rel(project);
    let patch_abs = safe_join(&project.game_dir, &rel)?;
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

        let source = file.origin.archive().map(|s| s.to_string());
        let base_crc32 = source
            .as_deref()
            .and_then(|archive| project.archived_crc(archive, &file.rel));
        manifest.entries.insert(
            file.rel.clone(),
            PatchEntry {
                source,
                base_crc32,
                baked_at: manifest.baked_at.clone(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::{self, EditState};
    use renpy_core::engine::run_json_dump;
    use renpy_core::Launcher;
    use std::process::Command;

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
        let report = bake(&project, &undo_dir, &backups).unwrap();
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
        let report = bake(&project, &undo_dir, &backups).unwrap();
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
