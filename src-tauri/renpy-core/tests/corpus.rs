//! Runs the reader over a folder of real Ren'Py games.
//!
//!   RENPY_CORPUS=/path/to/games cargo test --release --test corpus -- --nocapture
//!
//! With `RENPY_CORPUS_ENGINE=1` it also asks each game's bundled engine for a
//! `--json-dump` (cached in the temp folder; `=refresh` forces a new run) and
//! requires the reader to agree with the engine about where labels are. There
//! are no per-game exceptions: a mismatch is a reader bug.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use renpy_core::engine::{run_json_dump, EngineRun};
use renpy_core::Project;

fn games(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| p.join("game").is_dir())
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

fn engine_run(p: &Project, name: &str, mode: &str) -> Option<EngineRun> {
    let cache_dir = std::env::temp_dir().join("vn-ide-corpus");
    let _ = fs::create_dir_all(&cache_dir);
    let safe: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect();
    let cache = cache_dir.join(format!("{safe}.json"));
    if mode != "refresh" {
        if let Some(run) = fs::read_to_string(&cache)
            .ok()
            .and_then(|t| serde_json_from(&t))
        {
            return Some(run);
        }
    }
    let launcher = p.launcher.as_ref()?;
    match run_json_dump(&p.root, &p.game_dir, launcher, p.source_key()) {
        Ok(run) => {
            if let Ok(text) = serde_json_to(&run) {
                let _ = fs::write(&cache, text);
            }
            Some(run)
        }
        Err(e) => {
            println!("  engine dump failed: {e}");
            None
        }
    }
}

// serde_json is a dependency of the crate, re-used here through its public types.
fn serde_json_from(text: &str) -> Option<EngineRun> {
    serde_json::from_str(text).ok()
}
fn serde_json_to(run: &EngineRun) -> Result<String, String> {
    serde_json::to_string(run).map_err(|e| e.to_string())
}

#[test]
fn corpus() {
    let Ok(dir) = std::env::var("RENPY_CORPUS") else {
        eprintln!("RENPY_CORPUS not set; skipping");
        return;
    };
    let engine_mode = std::env::var("RENPY_CORPUS_ENGINE").unwrap_or_default();
    let list = games(Path::new(&dir));
    assert!(!list.is_empty(), "no games with a game/ folder under {dir}");

    let mut failures: Vec<String> = Vec::new();
    println!(
        "{:<28} {:>7} {:>9} {:>6} {:>5} {:>6} {:>5} {:>7} {:>5} {:>6} {:>7}",
        "game",
        "engine",
        "lines",
        "labels",
        "menus",
        "edges",
        "dyn",
        "missing",
        "unrch",
        "syntax",
        "ms"
    );
    for path in list {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let started = Instant::now();
        let mut project = match Project::open(&path) {
            Ok(p) => p,
            Err(e) => {
                failures.push(format!("{name}: could not open: {e}"));
                continue;
            }
        };
        let ms = started.elapsed().as_millis();
        let s = project.analysis.stats.clone();
        let syntax: usize = project.files.iter().map(|f| f.issues.len()).sum();
        println!(
            "{:<28} {:>7} {:>9} {:>6} {:>5} {:>6} {:>5} {:>7} {:>5} {:>6} {:>7}",
            name.chars().take(28).collect::<String>(),
            project.engine_version.clone().unwrap_or_else(|| "?".into()),
            s.lines,
            s.labels,
            s.menus,
            s.edges,
            s.dynamic_jumps,
            s.missing_targets,
            s.unreachable,
            syntax,
            ms
        );
        for d in project
            .analysis
            .diagnostics
            .items
            .iter()
            .filter(|d| d.code == "missing-label" || d.code == "syntax")
            .take(6)
        {
            println!("    {} {}:{} {}", d.code, d.path, d.line, d.message);
        }
        if !project.compiled_only.is_empty() || !project.archives.is_empty() {
            println!(
                "    source-less: {} compiled-only .rpyc, {} .rpa archive(s)",
                project.compiled_only.len(),
                project.archives.len()
            );
        }
        if ms > 1500 {
            failures.push(format!(
                "{name}: parse + analysis took {ms} ms (limit 1500)"
            ));
        }
        let logical: u64 = project.files.iter().map(|f| f.total_lines as u64).sum();
        if logical > 0 && (syntax as f64) / (logical as f64) > 0.005 {
            failures.push(format!(
                "{name}: {syntax} syntax issues in {logical} lines (> 0.5%)"
            ));
        }

        if !engine_mode.is_empty() {
            let Some(run) = engine_run(&project, &name, &engine_mode) else {
                println!("    engine: no dump available");
                continue;
            };
            let labels = run.dump.labels.len();
            println!(
                "    engine: {} labels, {} screens, {} defines in {} ms{}",
                labels,
                run.dump.screens.len(),
                run.dump.defines,
                run.duration_ms,
                if run.from_cache { " (cached)" } else { "" }
            );
            for n in &run.notes {
                println!("    engine note: {n}");
            }
            project.set_engine(run);
            if labels == 0 {
                println!("    engine reports no labels; comparison skipped");
                continue;
            }
            let diff = project.analysis.engine_diff.clone().unwrap_or_default();
            println!(
                "    diff: {} parser-only, {} engine-only, {} moved",
                diff.parser_only.len(),
                diff.engine_only.len(),
                diff.moved.len()
            );
            for (n, f, l) in diff.parser_only.iter().take(8) {
                println!("      parser-only {n} at {f}:{l}");
            }
            for (n, f, l) in diff.engine_only.iter().take(8) {
                println!("      engine-only {n} at {f}:{l}");
            }
            for (n, a, b) in diff.moved.iter().take(8) {
                println!(
                    "      moved {n}: parser {}:{} vs engine {}:{}",
                    a.0, a.1, b.0, b.1
                );
            }
            if !diff.is_empty() {
                failures.push(format!(
                    "{name}: parser disagrees with the engine ({} parser-only, {} engine-only, {} moved)",
                    diff.parser_only.len(),
                    diff.engine_only.len(),
                    diff.moved.len()
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "corpus failures:\n  {}",
        failures.join("\n  ")
    );
}
