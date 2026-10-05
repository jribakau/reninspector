//! Read compiled Ren'Py scripts (`.rpyc`) without executing them.
//!
//! The result is `.rpy` text the existing parser can map. A file is editable
//! only when that text describes the same labels, jumps, calls, dialogue and
//! menu choices as the compiled tree. Anything we cannot represent stays
//! visible and read-only, so a save cannot drop it.

mod ast;
mod atl;
mod container;
mod decompile;
mod sl;
mod unpickle;
mod verify;

pub use unpickle::{loads_inert, Value};

#[derive(Debug, Clone)]
pub struct Decompiled {
    pub text: String,
    /// Safe to write back as a `.rpy`. False when the decompile is partial or
    /// does not round-trip through the parser.
    pub editable: bool,
    pub partial: bool,
    /// Why the file is read-only. Empty when it is editable.
    pub reasons: Vec<String>,
    /// Compiled linenumber for each line of `text`. Same length as `text.lines()`.
    pub engine_lines: Vec<u32>,
    /// Script path relative to `game/`, from the compiled filename.
    pub engine_file: Option<String>,
}

#[cfg(test)]
mod tests {
    fn first_compiled_where(pred: impl Fn(&super::Decompiled) -> bool) -> Option<Vec<u8>> {
        let root = std::env::var_os("RPYC_GAME_PATH")?;
        let root = std::path::PathBuf::from(root);
        let game = if root.join("game").is_dir() {
            root.join("game")
        } else {
            root
        };
        if !game.is_dir() {
            return None;
        }
        let mut files = Vec::new();
        super::collect_rpyc(&game, &game, &mut files);
        for rel in files {
            let Ok(bytes) = std::fs::read(game.join(&rel)) else {
                continue;
            };
            if let Ok(got) = super::decompile(&bytes) {
                if pred(&got) {
                    return Some(bytes);
                }
            }
        }
        None
    }

    #[test]
    fn decompiles_a_story_script_when_configured() {
        let Some(bytes) = first_compiled_where(|got| got.text.contains("label ")) else {
            eprintln!("RPYC_GAME_PATH not set; skipping");
            return;
        };
        let got = super::decompile(&bytes).unwrap_or_else(|e| panic!("{e}"));
        assert!(
            got.editable,
            "story file should be editable:\n{}",
            got.text.lines().take(8).collect::<Vec<_>>().join("\n")
        );
        assert!(got.text.contains("label "));
    }

    #[test]
    fn decompiles_a_screen_script_when_configured() {
        let Some(bytes) =
            first_compiled_where(|got| got.text.contains("screen ") || got.text.contains("style "))
        else {
            eprintln!("RPYC_GAME_PATH not set; skipping");
            return;
        };
        let got = super::decompile(&bytes).unwrap_or_else(|e| panic!("{e}"));
        assert!(
            got.text.contains("screen ") || got.text.contains("style "),
            "{}",
            got.text.lines().take(12).collect::<Vec<_>>().join("\n")
        );
    }

    #[test]
    fn fixtures_decompile_and_survive_fuzzing() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/rpyc");
        let mut found = 0;
        for which in ["7", "8"] {
            let path = root.join(which).join("script.rpyc");
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            found += 1;
            let got = super::decompile(&bytes).unwrap_or_else(|e| panic!("{which}: {e}"));
            assert!(got.editable, "{which}: {:?}\n{}", got.reasons, got.text);
            assert!(got.text.contains("label start"), "{which}");
            fuzz_compiled(&bytes);
        }
        if found == 0 {
            eprintln!(
                "compiled fixtures are not in the tree yet; the engine round trip writes them"
            );
        }
    }

    fn fuzz_compiled(bytes: &[u8]) {
        for n in [
            0usize,
            1,
            8,
            32,
            bytes.len() / 2,
            bytes.len().saturating_sub(1),
        ] {
            if n > bytes.len() {
                continue;
            }
            let slice = &bytes[..n];
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _ = super::decompile(slice);
            }));
            assert!(result.is_ok(), "decompile panicked on a truncated script");
        }
        for i in (0..bytes.len()).step_by((bytes.len() / 24).max(1)) {
            let mut flipped = bytes.to_vec();
            flipped[i] ^= 0xff;
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _ = super::decompile(&flipped);
            }));
            assert!(result.is_ok(), "decompile panicked after a bit flip at {i}");
        }
    }

    /// Reports editable, partial and failed counts. Never writes the game.
    /// `RPYC_GAME_PATH` may list several roots separated by `;`.
    #[test]
    fn reports_compiled_scripts_at_rpyc_game_path() {
        let Ok(raw) = std::env::var("RPYC_GAME_PATH") else {
            eprintln!("RPYC_GAME_PATH is unset; skipping");
            return;
        };
        for path in raw.split(';').filter(|s| !s.trim().is_empty()) {
            let root = std::path::Path::new(path.trim());
            let game = if root.join("game").is_dir() {
                root.join("game")
            } else {
                root.to_path_buf()
            };
            assert!(game.is_dir(), "RPYC_GAME_PATH is not a game: {path}");
            let mut files = Vec::new();
            super::collect_rpyc(&game, &game, &mut files);
            let mut editable = 0;
            let mut partial = 0;
            let mut failed = 0;
            for rel in &files {
                let bytes = std::fs::read(game.join(rel)).unwrap();
                match super::decompile(&bytes) {
                    Ok(got) if got.editable => editable += 1,
                    Ok(got) => {
                        partial += 1;
                        eprintln!("partial {rel}: {:?}", got.reasons);
                    }
                    Err(e) => {
                        failed += 1;
                        eprintln!("failed {rel}: {e}");
                    }
                }
            }
            eprintln!(
                "{path} editable={editable} partial={partial} failed={failed} files={}",
                files.len()
            );
            assert!(!files.is_empty(), "no .rpyc under {path}");
            assert_eq!(editable + partial + failed, files.len());
        }
    }
}

#[cfg(test)]
fn collect_rpyc(root: &std::path::Path, dir: &std::path::Path, out: &mut Vec<String>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for ent in rd.flatten() {
        let path = ent.path();
        if path.is_dir() {
            collect_rpyc(root, &path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rpyc") {
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            out.push(rel);
        }
    }
}

/// Node sequence of a compiled script, after the same folds the decompiler uses.
pub fn node_sequence(bytes: &[u8]) -> Result<Vec<String>, String> {
    let nodes = container::read_statements(bytes)?;
    let tree = ast::tree_from(&nodes);
    Ok(ast::sequence(&tree.nodes))
}

fn engine_file_of(nodes: &[Value]) -> Option<String> {
    let raw = nodes.iter().find_map(|n| ast::text_field(n, "filename"))?;
    let rel = game_script_rel(&raw);
    if rel.is_empty() {
        None
    } else {
        Some(rel)
    }
}

/// `game/script.rpy`, `C:/proj/game/script.rpy` and `script.rpy` all become `script.rpy`.
pub fn game_script_rel(path: &str) -> String {
    let s = path.replace('\\', "/");
    if let Some(i) = s.rfind("/game/") {
        return s[i + "/game/".len()..].to_string();
    }
    s.strip_prefix("game/").unwrap_or(&s).to_string()
}

pub fn decompile(bytes: &[u8]) -> Result<Decompiled, String> {
    let nodes = container::read_statements(bytes)?;
    let engine_file = engine_file_of(&nodes);
    let tree = ast::tree_from(&nodes);
    let (text, engine_lines) = decompile::render_mapped(&tree);
    let why = verify::explain(&tree, &text);
    let mut reasons = tree.reasons;
    if let Some(why) = why {
        if why != "partial" {
            reasons.push(why);
        }
    }
    let editable = reasons.is_empty();
    Ok(Decompiled {
        text,
        editable,
        partial: tree.partial || !editable,
        reasons,
        engine_lines,
        engine_file,
    })
}
