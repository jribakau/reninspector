//! Search, file operations, symbols, assets, translations, logs and project lifecycle.

use crate::error::AppError;
use std::fs;
use std::path::Path;

use serde::Serialize;
use tauri::State;

use crate::commands::AppState;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogLine {
    pub source: String,
    pub text: String,
    pub path: Option<String>,
    pub line: Option<u32>,
}

#[tauri::command(async)]
pub fn read_logs(state: State<'_, AppState>) -> Result<Vec<LogLine>, AppError> {
    crate::util::with_project(&state, |project| {
        let mut out = Vec::new();
        for name in ["traceback.txt", "errors.txt", "log.txt"] {
            let path = project.root.join(name);
            let Some(text) = tail_file(&path, 80_000) else {
                continue;
            };
            for line in text
                .lines()
                .rev()
                .take(120)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
            {
                let (file, line_no) = link_in(line);
                out.push(LogLine {
                    source: name.into(),
                    text: line.trim_end().to_string(),
                    path: file,
                    line: line_no,
                });
            }
        }
        Ok(out)
    })
}

/// Swaps the user's home folder for `~`, in both slash styles, so a pasted
/// bundle does not carry the account name.
pub(crate) fn hide_home(text: &str, home: Option<&str>) -> String {
    let Some(home) = home
        .map(|h| h.trim_end_matches(['/', '\\']))
        .filter(|h| h.len() > 3)
    else {
        return text.to_string();
    };
    let mut out = text.to_string();
    for variant in [
        home.to_string(),
        home.replace('\\', "/"),
        home.replace('/', "\\"),
    ] {
        out = out.replace(&variant, "~");
    }
    out
}

fn home_dir() -> Option<String> {
    std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .ok()
}

/// Text the user asked to copy. Nothing is sent anywhere. The home folder is
/// replaced with `~`, but log lines can still name other private folders.
#[tauri::command(async)]
pub fn diagnostic_bundle(state: State<'_, AppState>) -> String {
    let mut out = format!(
        "Ren'Inspector {}\n{} {}\n",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    // Only the facts are read under the lock. The log files are read after it is released.
    let facts = state.project.lock().ok().and_then(|guard| {
        guard.as_ref().map(|project| {
            (
                project.root.clone(),
                project.engine_version.clone(),
                project.script_version.clone(),
            )
        })
    });
    if let Some((root, engine_version, script_version)) = facts {
        out.push_str(&format!("Project: {}\n", root.display()));
        if let Some(version) = engine_version {
            out.push_str(&format!("Ren'Py: {version}\n"));
        }
        if let Some(version) = script_version {
            out.push_str(&format!("Script version: {version}\n"));
        }
        out.push('\n');
        for name in ["traceback.txt", "errors.txt", "log.txt"] {
            let Some(text) = tail_file(&root.join(name), 8_000) else {
                continue;
            };
            let lines: Vec<&str> = text.lines().rev().take(40).collect();
            if lines.is_empty() {
                continue;
            }
            out.push_str(&format!("--- {name}\n"));
            for line in lines.into_iter().rev() {
                out.push_str(line);
                out.push('\n');
            }
        }
    } else {
        out.push_str("No project is open.\n");
    }
    hide_home(&out, home_dir().as_deref())
}

pub(crate) fn tail_file(path: &Path, max: usize) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    // Only the end of a log is wanted, and a log can be large.
    let mut file = fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let start = len.saturating_sub(max as u64);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut bytes = Vec::with_capacity((len - start) as usize);
    file.take(max as u64).read_to_end(&mut bytes).ok()?;
    let start = start as usize;
    let text = String::from_utf8_lossy(&bytes);
    let text = if start > 0 {
        text.split_once('\n')
            .map(|(_, rest)| rest)
            .unwrap_or(&text)
            .to_string()
    } else {
        text.into_owned()
    };
    Some(text)
}

fn link_in(line: &str) -> (Option<String>, Option<u32>) {
    if let Some(rest) = line.split("File \"").nth(1) {
        if let Some((file, after)) = rest.split_once('"') {
            let line_no = after.split("line ").nth(1).and_then(|s| {
                s.split(|c: char| !c.is_ascii_digit())
                    .next()
                    .and_then(|n| n.parse().ok())
            });
            return (Some(clean_game_path(file)), line_no);
        }
    }
    for token in line.split_whitespace() {
        let token = token.trim_matches(|c: char| c == ',' || c == ':' || c == '(' || c == ')');
        if let Some((file, n)) = token.rsplit_once(':') {
            if (file.ends_with(".rpy") || file.ends_with(".rpym"))
                && n.chars().all(|c| c.is_ascii_digit())
            {
                if let Ok(num) = n.parse() {
                    return (Some(clean_game_path(file)), Some(num));
                }
            }
        }
    }
    (None, None)
}

fn clean_game_path(file: &str) -> String {
    let file = file.replace('\\', "/");
    file.strip_prefix("./")
        .or_else(|| file.strip_prefix("game/"))
        .unwrap_or(&file)
        .to_string()
}
