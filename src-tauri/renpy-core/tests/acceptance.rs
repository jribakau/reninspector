//! Optional read-only check. Set `RENPY_GAME_PATH` to a project folder.
//! Skipped when the variable is unset. The project is never modified.

use std::path::PathBuf;
use std::time::Instant;

use renpy_core::Project;

fn project_path() -> Option<PathBuf> {
    std::env::var_os("RENPY_GAME_PATH").map(PathBuf::from)
}

#[test]
fn opens_when_a_project_path_is_set() {
    let Some(path) = project_path() else {
        eprintln!("RENPY_GAME_PATH not set; skipping");
        return;
    };
    let started = Instant::now();
    let project = Project::open(&path).expect("open project");
    let elapsed = started.elapsed();
    eprintln!(
        "parsed {} files / {} lines in {:?}",
        project.files.len(),
        project.analysis.stats.lines,
        elapsed
    );
    assert!(!project.files.is_empty(), "project has no scripts");
}
