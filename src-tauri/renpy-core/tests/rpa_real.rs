//! Reads archives from a real game without writing to it.
//! Set `RPA_GAME_PATH` to the project folder. Skipped otherwise.

use std::fs;
use std::path::{Path, PathBuf};

use renpy_core::rpa::{utc_now, Archive, ArchiveWriter, PatchEntry, PatchManifest, MANIFEST_NAME};

fn game_dir(root: &Path) -> Option<PathBuf> {
    let game = root.join("game");
    if game.is_dir() {
        Some(game)
    } else if root
        .file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.eq_ignore_ascii_case("game"))
        .unwrap_or(false)
    {
        Some(root.to_path_buf())
    } else {
        None
    }
}

fn walk_rpa(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(ty) = entry.file_type() else { continue };
        if ty.is_dir() {
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            if name != "cache" && name != "saves" {
                walk_rpa(&path, out);
            }
        } else if path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("rpa"))
            .unwrap_or(false)
        {
            out.push(path);
        }
    }
}

#[test]
fn image_archives_list_image_files() {
    let Ok(root) = std::env::var("RPA_GAME_PATH") else {
        eprintln!("RPA_GAME_PATH not set; skipping");
        return;
    };
    let Some(game) = game_dir(Path::new(&root)) else {
        panic!("{root} has no game folder");
    };
    let mut archives = Vec::new();
    walk_rpa(&game, &mut archives);
    let mut images = 0u32;
    let mut sample = Vec::new();
    let mut saw_image_archive = false;
    for path in &archives {
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if !file_name.contains("image") {
            continue;
        }
        saw_image_archive = true;
        let archive = Archive::open(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        for name in archive.entries.keys() {
            let ext = name
                .rsplit_once('.')
                .map(|(_, e)| e.to_ascii_lowercase())
                .unwrap_or_default();
            if matches!(
                ext.as_str(),
                "png" | "jpg" | "jpeg" | "webp" | "gif" | "avif"
            ) {
                images += 1;
                if sample.len() < 3 {
                    sample.push(name.clone());
                }
            }
        }
    }
    if saw_image_archive {
        assert!(images > 0, "image archive opened but contained no images");
    }
    eprintln!("{images} image files, for example {sample:?}");
}

#[test]
fn real_game_archives_round_trip_in_a_temp_dir() {
    let Ok(root) = std::env::var("RPA_GAME_PATH") else {
        eprintln!("RPA_GAME_PATH not set; skipping");
        return;
    };
    let Some(game) = game_dir(Path::new(&root)) else {
        panic!("{root} has no game folder");
    };
    let mut archives = Vec::new();
    walk_rpa(&game, &mut archives);
    assert!(
        !archives.is_empty(),
        "no .rpa files under {}",
        game.display()
    );

    let mut sample: Option<(String, Vec<u8>)> = None;
    for path in &archives {
        let archive = Archive::open(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let mut total = 0u64;
        for name in archive.entries.keys() {
            let stats = archive
                .copy_entry(name, &mut std::io::sink())
                .unwrap_or_else(|e| panic!("{}:{name}: {e}", path.display()));
            let declared = archive.get(name).unwrap().declared_len().unwrap();
            assert_eq!(stats.len, declared, "{name}");
            total += stats.len;
            if sample.is_none() && name.ends_with(".rpy") && stats.len < 256 * 1024 {
                let bytes = archive.read_entry(name, 256 * 1024).unwrap();
                sample = Some((name.clone(), bytes));
            }
        }
        eprintln!(
            "{}: {} entries, {total} bytes",
            path.display(),
            archive.entries.len()
        );
    }

    let Some((name, bytes)) = sample else {
        eprintln!("no small .rpy inside the archives; listing only");
        return;
    };
    let dir = std::env::temp_dir().join(format!("vn-ide-rpa-real-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let mut edited = bytes.clone();
    edited.extend(b"\n# vn-ide patch probe\n");
    let when = utc_now();
    let mut manifest = PatchManifest {
        version: 1,
        baked_at: when.clone(),
        entries: Default::default(),
    };
    manifest.entries.insert(
        name.clone(),
        PatchEntry {
            source: Some("original.rpa".into()),
            base_crc32: Some(crc32fast::hash(&bytes)),
            baked_at: when,
            generated: false,
        },
    );
    let manifest_bytes = manifest.to_bytes().unwrap();
    let patch = dir.join("zz_vnide_patch.rpa");
    let mut writer = ArchiveWriter::create(&patch).unwrap();
    writer.add_bytes(&name, &edited).unwrap();
    writer.add_bytes(MANIFEST_NAME, &manifest_bytes).unwrap();
    writer.finish().unwrap();

    let packed = Archive::open(&patch).unwrap();
    assert_eq!(packed.read_entry(&name, 300_000).unwrap(), edited);
    assert!(packed.get(MANIFEST_NAME).is_some());
    let _ = fs::remove_dir_all(dir);
}
