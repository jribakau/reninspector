//! Zip a mod the player can copy into `game/`. Only changed files go in.

use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

use renpy_core::rpa::{self, ArchiveWriter, PatchManifest};
use renpy_core::{sha256_file, Project};
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

pub fn export(
    project: &Project,
    dest: &Path,
    layout: &str,
    include_toggles: bool,
    notes: &str,
) -> Result<String, String> {
    if layout != "rpa" && layout != "loose" {
        return Err("Layout must be rpa or loose.".into());
    }
    if dest.as_os_str().is_empty() {
        return Err("Choose a zip file to write.".into());
    }
    let patch = project
        .archives
        .iter()
        .find(|item| item.info.is_patch)
        .ok_or_else(|| "Bake a patch before exporting a mod.".to_string())?;
    let archive = patch
        .archive
        .as_ref()
        .ok_or_else(|| "The patch could not be read.".to_string())?;
    let manifest = rpa::read_manifest(archive)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "The patch has no manifest.".to_string())?;

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
        let bytes = archive
            .read_entry(name, rpa::SCRIPT_MAX)
            .map_err(|e| format!("Could not read `{name}` from the patch: {e}"))?;
        if same_as_base(project, entry.source.as_deref(), name, &bytes) {
            skipped += 1;
            continue;
        }
        kept.push((name.clone(), bytes));
    }
    // The game runs the compiled sidecar of an archived script, and the patch
    // stores it beside the script without a manifest entry. Ship it too.
    let scripts: Vec<String> = kept.iter().map(|(name, _)| name.clone()).collect();
    for name in scripts {
        let side = sidecar_name(&name);
        if kept.iter().any(|(have, _)| *have == side) || !archive.entries.contains_key(&side) {
            continue;
        }
        let bytes = archive
            .read_entry(&side, rpa::SCRIPT_MAX)
            .map_err(|e| format!("Could not read `{side}` from the patch: {e}"))?;
        kept.push((side, bytes));
    }
    if kept.is_empty() {
        return Err(
            "Nothing to export. A mod only contains files that differ from the original game."
                .into(),
        );
    }

    let game = project.game_info();
    let archives = game
        .archives
        .iter()
        .map(|item| {
            let path = project.game_dir.join(&item.path);
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
    let json = serde_json::to_vec_pretty(&doc).map_err(|e| e.to_string())?;
    let readme = readme(&game, notes, skipped);

    if let Some(parent) = dest.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
    }
    let file = File::create(dest).map_err(|e| format!("Could not create the zip: {e}"))?;
    let mut zip = ZipWriter::new(file);
    let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    if layout == "rpa" {
        let packed = pack_rpa(&patch.info.path, &kept, &manifest)?;
        let arc_name = patch
            .info
            .path
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or("patch.rpa");
        zip.start_file(arc_name, opts)
            .map_err(|e| e.to_string())?;
        zip.write_all(&packed).map_err(|e| e.to_string())?;
    } else {
        for (name, bytes) in &kept {
            let zip_name = name.replace('\\', "/");
            zip.start_file(zip_name, opts).map_err(|e| e.to_string())?;
            zip.write_all(bytes).map_err(|e| e.to_string())?;
        }
    }
    zip.start_file("README.txt", opts).map_err(|e| e.to_string())?;
    zip.write_all(readme.as_bytes()).map_err(|e| e.to_string())?;
    zip.start_file("vnide_mod.json", opts)
        .map_err(|e| e.to_string())?;
    zip.write_all(&json).map_err(|e| e.to_string())?;
    zip.finish().map_err(|e| e.to_string())?;
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

fn same_as_base(project: &Project, source: Option<&str>, name: &str, bytes: &[u8]) -> bool {
    let Some(source) = source else {
        return false;
    };
    let Some(archive) = project
        .archives
        .iter()
        .find(|item| item.info.path == source)
        .and_then(|item| item.archive.as_ref())
    else {
        return false;
    };
    match archive.read_entry(name, rpa::SCRIPT_MAX) {
        Ok(original) => original == bytes,
        Err(_) => false,
    }
}

fn pack_rpa(rel: &str, kept: &[(String, Vec<u8>)], old: &PatchManifest) -> Result<Vec<u8>, String> {
    let temp = std::env::temp_dir().join(format!(
        "vn-ide-export-{}-{}.rpa",
        std::process::id(),
        rel.replace(['/', '\\'], "_")
    ));
    let mut writer = ArchiveWriter::create(&temp).map_err(|e| e.to_string())?;
    let mut next = PatchManifest {
        version: old.version,
        baked_at: old.baked_at.clone(),
        entries: Default::default(),
    };
    for (name, bytes) in kept {
        writer.add_bytes(name, bytes).map_err(|e| e.to_string())?;
        if let Some(entry) = old.entries.get(name) {
            next.entries.insert(name.clone(), entry.clone());
        }
    }
    let manifest = next.to_bytes().map_err(|e| e.to_string())?;
    writer
        .add_bytes(rpa::MANIFEST_NAME, &manifest)
        .map_err(|e| e.to_string())?;
    writer.finish().map_err(|e| e.to_string())?;
    let bytes = fs::read(&temp).map_err(|e| e.to_string());
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
