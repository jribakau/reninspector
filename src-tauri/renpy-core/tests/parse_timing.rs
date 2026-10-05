//! Times compiled-script parsing over real games.
//!
//!   RPYC_GAME_PATH=/path/to/game cargo test --release --test parse_timing -- --nocapture
//!
//! Several roots may be separated by `;`. Skips when the variable is unset.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use renpy_core::rpyc::{decompile, node_sequence};
use sha2::{Digest, Sha256};

const RUNS: usize = 3;

#[test]
fn times_compiled_scripts_at_rpyc_game_path() {
    let Ok(raw) = std::env::var("RPYC_GAME_PATH") else {
        eprintln!("RPYC_GAME_PATH is unset; skipping");
        return;
    };
    let mut files = Vec::new();
    let mut bytes_total = 0usize;
    for path in raw.split(';').filter(|s| !s.trim().is_empty()) {
        let root = Path::new(path.trim());
        let game = if root.join("game").is_dir() {
            root.join("game")
        } else {
            root.to_path_buf()
        };
        assert!(game.is_dir(), "RPYC_GAME_PATH is not a game: {path}");
        collect_rpyc(&game, &mut files);
    }
    files.sort();
    assert!(!files.is_empty(), "no .rpyc under RPYC_GAME_PATH");
    let loaded: Vec<Vec<u8>> = files
        .iter()
        .map(|path| {
            let bytes = fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            bytes_total += bytes.len();
            bytes
        })
        .collect();

    let mut best = Duration::MAX;
    let mut parsed = 0usize;
    for run in 0..RUNS {
        let start = Instant::now();
        let mut ok = 0usize;
        for bytes in &loaded {
            if node_sequence(bytes).is_ok() {
                ok += 1;
            }
        }
        let elapsed = start.elapsed();
        eprintln!("run {run}: {elapsed:?} files={} parsed={ok}", loaded.len());
        if elapsed < best {
            best = elapsed;
            parsed = ok;
        }
    }
    eprintln!(
        "best of {RUNS}: {best:?} files={} parsed={parsed} bytes={bytes_total}",
        loaded.len()
    );
    eprintln!("output digest: {}", output_digest(&files, &loaded));
}

/// One hash over the node sequence and the full decompiled text of every file.
/// Equal digests before and after a parser change mean the output did not move.
fn output_digest(files: &[PathBuf], loaded: &[Vec<u8>]) -> String {
    let mut hasher = Sha256::new();
    for (path, bytes) in files.iter().zip(loaded) {
        hasher.update(path.to_string_lossy().as_bytes());
        match node_sequence(bytes) {
            Ok(seq) => {
                for node in seq {
                    hasher.update(node.as_bytes());
                    hasher.update([0]);
                }
            }
            Err(e) => hasher.update(format!("err:{e}").as_bytes()),
        }
        match decompile(bytes) {
            Ok(got) => {
                hasher.update(got.text.as_bytes());
                hasher.update([got.editable as u8, got.partial as u8]);
                for reason in &got.reasons {
                    hasher.update(reason.as_bytes());
                }
                for line in &got.engine_lines {
                    hasher.update(line.to_le_bytes());
                }
                hasher.update(got.engine_file.unwrap_or_default().as_bytes());
            }
            Err(e) => hasher.update(format!("err:{e}").as_bytes()),
        }
    }
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn collect_rpyc(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else {
        return;
    };
    for ent in rd.flatten() {
        let path = ent.path();
        if path.is_dir() {
            collect_rpyc(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rpyc") {
            out.push(path);
        }
    }
}
