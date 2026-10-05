//! Zip a mod the player can copy into `game/`. Only changed files go in.

use crate::error::AppError;
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use renpy_core::rpa::{self, Archive, ArchiveWriter, PatchManifest};
use renpy_core::{sha256_file, GameInfo, Project};
use serde::Serialize;
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

use crate::patch::TOGGLES_NAME;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ModFile {
    name: String,
    source: Option<String>,
    base_crc32: Option<u32>,
    generated: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SourceArchive {
    path: String,
    sha256: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ModJson<'a> {
    name: Option<&'a str>,
    version: Option<&'a str>,
    engine_version: Option<&'a str>,
    archives: Vec<SourceArchive>,
    entries: Vec<ModFile>,
}

/// Everything a mod needs, owned, so it can be packed after the project lock is dropped.
///
/// The patch is copied into memory. An `Archive` reopens its file on every read, so a
/// handle alone would see a half-changed file if the patch were baked again mid-export.
/// The base game archives are never rewritten by the IDE, so handles are enough for them.
pub struct ExportJob {
    manifest: PatchManifest,
    patch_files: BTreeMap<String, Vec<u8>>,
    patch_path: String,
    sources: Vec<(String, Arc<Archive>)>,
    game: GameInfo,
    game_dir: PathBuf,
}

pub fn prepare(project: &Project) -> Result<ExportJob, AppError> {
    let patch_item = project
        .archives
        .iter()
        .find(|item| item.info.is_patch)
        .ok_or_else(|| AppError::new("Bake a patch before exporting a mod."))?;
    let patch = patch_item
        .archive
        .as_ref()
        .ok_or_else(|| AppError::new("The patch could not be read."))?;
    let manifest =
        rpa::read_manifest(patch)?.ok_or_else(|| AppError::new("The patch has no manifest."))?;
    let mut patch_files = BTreeMap::new();
    for name in patch.entries.keys() {
        let bytes = patch
            .read_entry(name, rpa::SCRIPT_MAX)
            .map_err(|e| format!("Could not read `{name}` from the patch: {e}"))?;
        patch_files.insert(name.clone(), bytes);
    }
    let sources = project
        .archives
        .iter()
        .filter_map(|item| {
            item.archive
                .as_ref()
                .map(|archive| (item.info.path.clone(), Arc::clone(archive)))
        })
        .collect();
    Ok(ExportJob {
        manifest,
        patch_files,
        patch_path: patch_item.info.path.clone(),
        sources,
        game: project.game_info(),
        game_dir: project.game_dir.clone(),
    })
}

pub fn export(
    job: &ExportJob,
    dest: &Path,
    layout: &str,
    include_toggles: bool,
    notes: &str,
) -> Result<String, AppError> {
    if layout != "rpa" && layout != "loose" {
        return Err("Layout must be rpa or loose.".into());
    }
    if dest.as_os_str().is_empty() {
        return Err("Choose a zip file to write.".into());
    }
    let manifest = &job.manifest;

    let mut kept: Vec<(String, Vec<u8>)> = Vec::new();
    let mut skipped = 0u32;
    for (name, entry) in &manifest.entries {
        if name == rpa::MANIFEST_NAME {
            continue;
        }
        if entry.generated && !include_toggles {
            continue;
        }
        if name == TOGGLES_NAME && !include_toggles {
            continue;
        }
        let bytes = job.patch_files.get(name).ok_or_else(|| {
            format!("Could not read `{name}` from the patch: it is not in the archive")
        })?;
        if same_as_base(&job.sources, entry.source.as_deref(), name, bytes) {
            skipped += 1;
            continue;
        }
        kept.push((name.clone(), bytes.clone()));
    }
    // The game runs the compiled sidecar of an archived script, and the patch
    // stores it beside the script without a manifest entry. Ship it too.
    let scripts: Vec<String> = kept.iter().map(|(name, _)| name.clone()).collect();
    for name in scripts {
        let side = sidecar_name(&name);
        if kept.iter().any(|(have, _)| *have == side) {
            continue;
        }
        if let Some(bytes) = job.patch_files.get(&side) {
            kept.push((side, bytes.clone()));
        }
    }
    if kept.is_empty() {
        return Err(
            "Nothing to export. A mod only contains files that differ from the original game."
                .into(),
        );
    }

    let game = &job.game;
    let archives = game
        .archives
        .iter()
        .map(|item| {
            let path = job.game_dir.join(&item.path);
            SourceArchive {
                path: item.path.clone(),
                sha256: sha256_file(&path).ok(),
            }
        })
        .collect();
    let entries: Vec<ModFile> = kept
        .iter()
        .map(|(name, _)| match manifest.entries.get(name) {
            Some(entry) => ModFile {
                name: name.clone(),
                source: entry.source.clone(),
                base_crc32: entry.base_crc32,
                generated: entry.generated,
            },
            None => ModFile {
                name: name.clone(),
                source: None,
                base_crc32: None,
                generated: false,
            },
        })
        .collect();
    let doc = ModJson {
        name: game.name.as_deref(),
        version: game.version.as_deref(),
        engine_version: game.engine_version.as_deref(),
        archives,
        entries,
    };
    let json = serde_json::to_vec_pretty(&doc)?;
    let readme = readme(game, notes, skipped);

    if let Some(parent) = dest.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    let file = File::create(dest).map_err(|e| format!("Could not create the zip: {e}"))?;
    let mut zip = ZipWriter::new(file);
    let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    if layout == "rpa" {
        let packed = pack_rpa(&job.patch_path, &kept, manifest)?;
        let arc_name = job
            .patch_path
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or("patch.rpa");
        zip.start_file(arc_name, opts)?;
        zip.write_all(&packed)?;
    } else {
        for (name, bytes) in &kept {
            let zip_name = name.replace('\\', "/");
            zip.start_file(zip_name, opts)?;
            zip.write_all(bytes)?;
        }
    }
    zip.start_file("README.txt", opts)?;
    zip.write_all(readme.as_bytes())?;
    zip.start_file("vnide_mod.json", opts)?;
    zip.write_all(&json)?;
    zip.finish()?;
    Ok(format!(
        "Exported {} file{} to {}",
        kept.len(),
        if kept.len() == 1 { "" } else { "s" },
        dest.display()
    ))
}

fn sidecar_name(rel: &str) -> String {
    if let Some(stem) = rel.strip_suffix(".rpym") {
        format!("{stem}.rpymc")
    } else if let Some(stem) = rel.strip_suffix(".rpy") {
        format!("{stem}.rpyc")
    } else {
        format!("{rel}.rpyc")
    }
}

fn same_as_base(
    sources: &[(String, Arc<Archive>)],
    source: Option<&str>,
    name: &str,
    bytes: &[u8],
) -> bool {
    let Some(source) = source else {
        return false;
    };
    let Some((_, archive)) = sources.iter().find(|(path, _)| path == source) else {
        return false;
    };
    match archive.read_entry(name, rpa::SCRIPT_MAX) {
        Ok(original) => original == bytes,
        Err(_) => false,
    }
}

fn pack_rpa(
    rel: &str,
    kept: &[(String, Vec<u8>)],
    old: &PatchManifest,
) -> Result<Vec<u8>, AppError> {
    let temp = std::env::temp_dir().join(format!(
        "vn-ide-export-{}-{}.rpa",
        std::process::id(),
        rel.replace(['/', '\\'], "_")
    ));
    let mut writer = ArchiveWriter::create(&temp)?;
    let mut next = PatchManifest {
        version: old.version,
        baked_at: old.baked_at.clone(),
        entries: Default::default(),
    };
    for (name, bytes) in kept {
        writer.add_bytes(name, bytes)?;
        if let Some(entry) = old.entries.get(name) {
            next.entries.insert(name.clone(), entry.clone());
        }
    }
    let manifest = next.to_bytes()?;
    writer.add_bytes(rpa::MANIFEST_NAME, &manifest)?;
    writer.finish()?;
    let bytes = fs::read(&temp).map_err(crate::error::AppError::from);
    let _ = fs::remove_file(&temp);
    bytes
}

fn readme(game: &renpy_core::GameInfo, notes: &str, skipped: u32) -> String {
    let name = game.name.as_deref().unwrap_or("this game");
    let version = game.version.as_deref().unwrap_or("unknown");
    let engine = game.engine_version.as_deref().unwrap_or("unknown");
    let mut text = format!(
        "{name} {version}\nRen'Py {engine}\n\n{notes}\n\nInstall: copy the files in this archive into the game's game/ folder.\nThis mod contains only changed files.\n"
    );
    if skipped > 0 {
        text.push_str(&format!(
            "{skipped} file{} matched the original and was left out.\n",
            if skipped == 1 { "" } else { "s" }
        ));
    }
    text
}
