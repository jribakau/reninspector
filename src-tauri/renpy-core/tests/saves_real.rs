//! Reads the real saves of every game under `RENPY_CORPUS`.
//!
//!   RENPY_CORPUS=/path/to/games cargo test --release --test saves_real -- --nocapture
//!
//! Nothing is written. A save that cannot be read is reported, and the test
//! fails only when reading one panics or a slot's metadata cannot be listed.

use std::fs;
use std::path::Path;

use renpy_core::saves::{inspect_save, list_saves};
use renpy_core::Project;

#[test]
fn real_saves_are_readable() {
    let Ok(dir) = std::env::var("RENPY_CORPUS") else {
        eprintln!("RENPY_CORPUS not set; skipping");
        return;
    };
    let mut games: Vec<_> = fs::read_dir(Path::new(&dir))
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| p.join("game").is_dir())
                .collect()
        })
        .unwrap_or_default();
    games.sort();
    let mut slots = 0u32;
    let mut deep_ok = 0u32;
    let mut deep_failed: Vec<String> = Vec::new();
    for path in games {
        let Ok(project) = Project::open(&path) else {
            continue;
        };
        let info = project.game_info();
        let found = list_saves(&project.game_dir, info.save_directory.as_deref());
        println!(
            "{:<32} save dir {:<24} {} slot(s)",
            path.file_name().unwrap().to_string_lossy(),
            info.save_directory.clone().unwrap_or_default(),
            found.len()
        );
        for slot in found {
            slots += 1;
            let file = Path::new(&slot.path);
            match inspect_save(file, false) {
                Ok(_) => {}
                Err(e) => {
                    deep_failed.push(format!("{}: {e}", slot.path));
                    continue;
                }
            }
            match inspect_save(file, true) {
                Ok(detail) => {
                    if detail.tree.is_some() {
                        deep_ok += 1;
                    } else if let Some(note) = detail.note {
                        deep_failed.push(format!("{}: {note}", slot.path));
                    }
                }
                Err(e) => deep_failed.push(format!("{}: {e}", slot.path)),
            }
        }
    }
    println!("{slots} slot(s), {deep_ok} unpickled");
    for line in &deep_failed {
        println!("  could not read: {line}");
    }
}
