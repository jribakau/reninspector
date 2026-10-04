//! Git status, history, and the commands the source control panel uses.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Output, Stdio};
use std::time::{Duration, Instant, SystemTime};

use serde::Serialize;
use tauri::{AppHandle, State};

use crate::commands::{self, AppState};
use crate::process::background;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitChange {
    pub path: String,
    pub status: String,
    pub staged: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitStatus {
    pub branch: String,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub has_remote: bool,
    pub has_ignore: bool,
    /// Staged paths outside the project folder, at most 20.
    pub outside_staged: Vec<String>,
    /// True when HEAD is already contained in the upstream branch.
    pub head_pushed: bool,
    /// Remote a first push would use. Empty when the choice is ambiguous.
    pub push_remote: Option<String>,
    pub changes: Vec<GitChange>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitCommit {
    pub hash: String,
    pub author: String,
    pub date: String,
    pub subject: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitCommitFile {
    pub status: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitBranch {
    pub name: String,
    pub current: bool,
    pub remote: bool,
    pub upstream: String,
}

const NOT_A_REPO: &str = "This folder is not a git repository.";
const GIT_TEXT_MAX: usize = 2 * 1024 * 1024;

const RENPY_IGNORE: &str = "\
*.rpyc
*.rpymc
*.rpyb
cache/
game/cache/
saves/
game/saves/
log.txt
errors.txt
traceback.txt
files.txt
";

fn no_project() -> String {
    "No project is open.".into()
}

/// A git command that never waits for a prompt and speaks English, so the messages
/// this module recognises read the same on every machine.
fn git_command(root: &Path, args: &[&str]) -> std::process::Command {
    let mut cmd = background("git");
    // `-c` has to precede the subcommand. quotePath keeps names like `scène.rpy` literal.
    cmd.arg("-c")
        .arg("core.quotePath=false")
        .args(args)
        .current_dir(root)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LANGUAGE", "C")
        .env("LC_MESSAGES", "C");
    cmd
}

fn git(root: &Path, args: &[&str]) -> Result<Output, String> {
    git_command(root, args)
        .output()
        .map_err(|_| "git is not available on PATH.".to_string())
}

const NET_TIMEOUT: Duration = Duration::from_secs(120);

/// Fetch, pull, and push. SSH may not ask for a passphrase, and a stuck server is
/// stopped after two minutes instead of leaving the panel busy forever.
fn git_net(root: &Path, args: &[&str]) -> Result<Output, String> {
    let mut cmd = git_command(root, args);
    let configured = git(root, &["config", "--get", "core.sshCommand"])
        .map(|out| !String::from_utf8_lossy(&out.stdout).trim().is_empty())
        .unwrap_or(false);
    if !configured
        && std::env::var_os("GIT_SSH_COMMAND").is_none()
        && std::env::var_os("GIT_SSH").is_none()
    {
        cmd.env("GIT_SSH_COMMAND", "ssh -o BatchMode=yes");
    }
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| "git is not available on PATH.".to_string())?;
    let mut out_pipe = child.stdout.take();
    let mut err_pipe = child.stderr.take();
    let out_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(pipe) = out_pipe.as_mut() {
            let _ = pipe.read_to_end(&mut buf);
        }
        buf
    });
    let err_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(pipe) = err_pipe.as_mut() {
            let _ = pipe.read_to_end(&mut buf);
        }
        buf
    });
    let deadline = Instant::now() + NET_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                // A helper git started may still hold the pipes. Leave the readers behind.
                return Err("git took longer than two minutes and was stopped.".into());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            Err(e) => return Err(e.to_string()),
        }
    };
    Ok(Output {
        status,
        stdout: out_reader.join().unwrap_or_default(),
        stderr: err_reader.join().unwrap_or_default(),
    })
}

fn git_stdin(root: &Path, args: &[&str], input: &[u8]) -> Result<Output, String> {
    let mut child = git_command(root, args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| "git is not available on PATH.".to_string())?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(input).map_err(|e| e.to_string())?;
    }
    child.wait_with_output().map_err(|e| e.to_string())
}

fn git_ok(output: &Output) -> Result<String, String> {
    if !output.status.success() {
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return Err(text.trim().to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn command_text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
    .trim()
    .to_string()
}

fn git_root(state: &State<'_, AppState>) -> Result<PathBuf, String> {
    let guard = state.project.lock().map_err(|e| e.to_string())?;
    Ok(guard.as_ref().ok_or_else(no_project)?.root.clone())
}

fn inside_work_tree(root: &Path) -> Result<bool, String> {
    Ok(git(root, &["rev-parse", "--is-inside-work-tree"])?
        .status
        .success())
}

/// Where the project folder sits inside its repository, such as `games/demo/`.
/// Empty when the project folder is the repository root.
fn repo_prefix(root: &Path) -> String {
    let Ok(out) = git(root, &["rev-parse", "--show-prefix"]) else {
        return String::new();
    };
    if !out.status.success() {
        return String::new();
    }
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .replace('\\', "/")
}

/// A path git printed, without a rename's old name and without quotes.
fn porcelain_path(raw: &str) -> String {
    let mut path = raw.trim().to_string();
    if let Some((_, after)) = path.split_once(" -> ") {
        path = after.to_string();
    }
    if path.len() >= 2 && path.starts_with('"') && path.ends_with('"') {
        path = path[1..path.len() - 1]
            .replace("\\\\", "\\")
            .replace("\\\"", "\"");
    }
    path.replace('\\', "/")
}

fn git_path(path: &str) -> Result<String, String> {
    let path = porcelain_path(path);
    if path.is_empty()
        || path.starts_with('/')
        || path.contains(':')
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err("That path is not inside the project.".into());
    }
    Ok(path)
}

fn git_rev(rev: &str) -> Result<String, String> {
    let rev = rev.trim();
    if rev.is_empty()
        || rev.starts_with('-')
        || rev.len() > 80
        || !rev
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '~' | '^' | '/' | '-'))
    {
        return Err("That revision is not valid.".into());
    }
    Ok(rev.to_string())
}

fn push_change(changes: &mut Vec<GitChange>, path: String, code: u8, staged: bool) {
    if code == b' ' {
        return;
    }
    let status = if code == b'?' {
        "?".to_string()
    } else {
        (code as char).to_string()
    };
    changes.push(GitChange {
        path,
        status,
        staged,
    });
}

struct BranchHead {
    name: String,
    upstream: Option<String>,
    ahead: u32,
    behind: u32,
}

/// Parses the text after `## ` from `git status --porcelain --branch`.
fn parse_branch_header(header: &str) -> BranchHead {
    let header = header.trim();
    let (head, tracking) = match header.split_once(" [") {
        Some((head, rest)) => (head.trim(), Some(rest.trim_end_matches(']').trim())),
        None => (header, None),
    };
    let (name, upstream) = if let Some((name, up)) = head.split_once("...") {
        let up = up.trim();
        (
            name.trim().to_string(),
            if up.is_empty() {
                None
            } else {
                Some(up.to_string())
            },
        )
    } else {
        (head.to_string(), None)
    };
    let name = name
        .strip_prefix("No commits yet on ")
        .unwrap_or(&name)
        .trim()
        .to_string();
    let name = if name == "HEAD (no branch)" {
        "HEAD".to_string()
    } else {
        name
    };
    let mut ahead = 0;
    let mut behind = 0;
    if let Some(tracking) = tracking {
        for part in tracking.split(',') {
            let part = part.trim();
            if let Some(n) = part.strip_prefix("ahead ") {
                ahead = n.trim().parse().unwrap_or(0);
            } else if let Some(n) = part.strip_prefix("behind ") {
                behind = n.trim().parse().unwrap_or(0);
            }
        }
    }
    BranchHead {
        name,
        upstream,
        ahead,
        behind,
    }
}

fn nul_records(text: &str) -> Vec<&str> {
    let mut records: Vec<&str> = text.split('\0').collect();
    if records.last().is_some_and(|record| record.is_empty()) {
        records.pop();
    }
    records
}

struct ParsedStatus {
    head: BranchHead,
    changes: Vec<GitChange>,
    outside_staged: Vec<String>,
}

/// `git status -z`: `XY <path>\0`, and for a rename or copy the next record is the old path.
/// The path in the status record is the new one.
fn parse_status_z(text: &str, prefix: &str) -> ParsedStatus {
    let records = nul_records(text);
    let mut head = BranchHead {
        name: String::new(),
        upstream: None,
        ahead: 0,
        behind: 0,
    };
    let mut changes = Vec::new();
    let mut outside_staged = Vec::new();
    let mut i = 0;
    while i < records.len() {
        let record = records[i];
        i += 1;
        if let Some(rest) = record.strip_prefix("## ") {
            head = parse_branch_header(rest);
            continue;
        }
        let bytes = record.as_bytes();
        if bytes.len() < 4 || bytes[2] != b' ' {
            continue;
        }
        let (x, y) = (bytes[0], bytes[1]);
        let full = record[3..].replace('\\', "/");
        if x == b'R' || x == b'C' || y == b'R' || y == b'C' {
            i += 1;
        }
        if full.is_empty() {
            continue;
        }
        let path = match full.strip_prefix(prefix) {
            Some(rest) => rest.to_string(),
            None => {
                if x != b' ' && x != b'?' && outside_staged.len() < 20 {
                    outside_staged.push(full);
                }
                continue;
            }
        };
        if path.is_empty() {
            continue;
        }
        if x == b'?' {
            push_change(&mut changes, path, b'?', false);
        } else {
            if x != b' ' {
                push_change(&mut changes, path.clone(), x, true);
            }
            if y != b' ' {
                push_change(&mut changes, path, y, false);
            }
        }
    }
    ParsedStatus {
        head,
        changes,
        outside_staged,
    }
}

fn remotes_at(root: &Path) -> Vec<String> {
    let Ok(out) = git(root, &["remote"]) else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect()
}

fn config_value(root: &Path, key: &str) -> Option<String> {
    let out = git(root, &["config", "--get", key]).ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

/// Branch pushRemote, then remote.pushDefault, then origin, then the only remote.
fn choose_push_remote(root: &Path, branch: &str, remotes: &[String]) -> Option<String> {
    if !branch.is_empty() && branch != "HEAD" {
        if let Some(remote) = config_value(root, &format!("branch.{branch}.pushRemote")) {
            return Some(remote);
        }
    }
    if let Some(remote) = config_value(root, "remote.pushDefault") {
        return Some(remote);
    }
    if remotes.iter().any(|remote| remote == "origin") {
        return Some("origin".to_string());
    }
    if remotes.len() == 1 {
        return Some(remotes[0].clone());
    }
    None
}

fn head_is_pushed(root: &Path, upstream: Option<&str>) -> bool {
    if upstream.map(str::is_empty).unwrap_or(true) {
        return false;
    }
    git(
        root,
        &["merge-base", "--is-ancestor", "HEAD", "@{upstream}"],
    )
    .map(|out| out.status.success())
    .unwrap_or(false)
}

fn status_at(root: &Path) -> Result<GitStatus, String> {
    if !inside_work_tree(root)? {
        return Err(NOT_A_REPO.into());
    }
    let output = git(
        root,
        &[
            "status",
            "--porcelain=v1",
            "-z",
            "--branch",
            "--untracked-files=all",
        ],
    )?;
    if !output.status.success() {
        return Err(command_text(&output));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let prefix = repo_prefix(root);
    let parsed = parse_status_z(&text, &prefix);
    let remotes = remotes_at(root);
    Ok(GitStatus {
        branch: parsed.head.name.clone(),
        upstream: parsed.head.upstream.clone(),
        ahead: parsed.head.ahead,
        behind: parsed.head.behind,
        has_remote: !remotes.is_empty(),
        has_ignore: root.join(".gitignore").is_file(),
        outside_staged: parsed.outside_staged,
        head_pushed: head_is_pushed(root, parsed.head.upstream.as_deref()),
        push_remote: choose_push_remote(root, &parsed.head.name, &remotes),
        changes: parsed.changes,
    })
}

fn write_ignore_file(root: &Path) -> Result<(), String> {
    let path = root.join(".gitignore");
    if path.exists() {
        return Err("A .gitignore is already in this folder.".into());
    }
    fs::write(&path, RENPY_IGNORE).map_err(|e| format!("Could not write .gitignore: {e}"))
}

fn ensure_ignore(root: &Path) -> Result<(), String> {
    if root.join(".gitignore").exists() {
        return Ok(());
    }
    write_ignore_file(root)
}

fn ignore_line(root: &Path, path: &str) -> Result<(), String> {
    let rel = git_path(path)?;
    if rel.chars().any(|c| c.is_control() || c == '#') {
        return Err("That path cannot be added to .gitignore.".into());
    }
    let file = root.join(".gitignore");
    let mut text = if file.is_file() {
        fs::read_to_string(&file).map_err(|e| format!("Could not read .gitignore: {e}"))?
    } else {
        String::new()
    };
    if text.lines().any(|line| line.trim() == rel) {
        return Ok(());
    }
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(&rel);
    text.push('\n');
    fs::write(&file, text).map_err(|e| format!("Could not write .gitignore: {e}"))
}

fn init_at(root: &Path) -> Result<String, String> {
    if inside_work_tree(root)? {
        return Err("This folder is already in a git repository.".into());
    }
    ensure_ignore(root)?;
    let text = git_ok(&git(root, &["init"])?)?;
    Ok(text.trim().to_string())
}

fn check_branch(root: &Path, name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() || name.chars().any(|c| c.is_control()) {
        return Err("That branch name is not valid.".into());
    }
    let out = git(root, &["check-ref-format", "--branch", name])?;
    if !out.status.success() {
        return Err("That branch name is not valid.".into());
    }
    Ok(name.to_string())
}

fn switch_failed_without_switch(err: &str) -> bool {
    let err = err.to_lowercase();
    err.contains("not a git command") || err.contains("is not a git command")
}

fn ref_exists(root: &Path, spec: &str) -> bool {
    git(root, &["show-ref", "--verify", "--quiet", spec])
        .map(|out| out.status.success())
        .unwrap_or(false)
}

fn switch_named(root: &Path, switch_args: &[&str], checkout_args: &[&str]) -> Result<(), String> {
    let out = git(root, switch_args)?;
    if out.status.success() {
        return Ok(());
    }
    let err = command_text(&out);
    if !switch_failed_without_switch(&err) {
        return Err(err);
    }
    git_ok(&git(root, checkout_args)?)?;
    Ok(())
}

/// `origin/feature/x` -> `feature/x`, using the longest configured remote name.
fn local_branch_for_remote(name: &str, remotes: &[String]) -> Option<String> {
    let mut remotes: Vec<&str> = remotes.iter().map(String::as_str).collect();
    remotes.sort_by_key(|remote| std::cmp::Reverse(remote.len()));
    for remote in remotes {
        let prefix = format!("{remote}/");
        if let Some(branch) = name.strip_prefix(&prefix) {
            if !branch.is_empty() {
                return Some(branch.to_string());
            }
        }
    }
    None
}

fn switch_at(root: &Path, name: &str) -> Result<(), String> {
    let name = check_branch(root, name)?;
    let local = ref_exists(root, &format!("refs/heads/{name}"));
    let remote = ref_exists(root, &format!("refs/remotes/{name}"));
    if !local && remote {
        if let Some(branch) = local_branch_for_remote(&name, &remotes_at(root)) {
            if ref_exists(root, &format!("refs/heads/{branch}")) {
                return switch_named(root, &["switch", &branch], &["checkout", &branch]);
            }
        }
        return switch_named(
            root,
            &["switch", "--track", &name],
            &["checkout", "--track", &name],
        );
    }
    switch_named(root, &["switch", &name], &["checkout", &name])
}

fn create_branch_at(root: &Path, name: &str) -> Result<(), String> {
    let name = check_branch(root, name)?;
    let out = git(root, &["switch", "-c", &name])?;
    if out.status.success() {
        return Ok(());
    }
    let err = command_text(&out);
    if !switch_failed_without_switch(&err) {
        return Err(err);
    }
    git_ok(&git(root, &["checkout", "-b", &name])?)?;
    Ok(())
}

fn branches_at(root: &Path) -> Result<Vec<GitBranch>, String> {
    if !inside_work_tree(root)? {
        return Err(NOT_A_REPO.into());
    }
    let text = git_ok(&git(
        root,
        &[
            "for-each-ref",
            "--format=%(refname)%x1f%(refname:short)%x1f%(HEAD)%x1f%(upstream:short)",
            "refs/heads",
            "refs/remotes",
        ],
    )?)?;
    let mut branches = Vec::new();
    for line in text.lines() {
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split('\u{1f}');
        let Some(full) = parts.next() else { continue };
        let Some(name) = parts.next() else { continue };
        if name.is_empty() || name.ends_with("/HEAD") {
            continue;
        }
        let head = parts.next().unwrap_or("");
        let upstream = parts.next().unwrap_or("").to_string();
        branches.push(GitBranch {
            name: name.to_string(),
            current: head == "*",
            remote: full.starts_with("refs/remotes/"),
            upstream,
        });
    }
    Ok(branches)
}

fn push_at(root: &Path) -> Result<String, String> {
    let status = status_at(root)?;
    if !status.has_remote {
        return Err("This repository has no remote. Add one before pushing.".into());
    }
    if status.branch.is_empty() || status.branch == "HEAD" {
        return Err("Switch to a branch before pushing.".into());
    }
    let text = if status.upstream.is_none() {
        let remote = status.push_remote.clone().ok_or_else(|| {
            "This repository has several remotes. Set an upstream first.".to_string()
        })?;
        git_ok(&git_net(root, &["push", "-u", &remote, &status.branch])?)?
    } else {
        git_ok(&git_net(root, &["push"])?)?
    };
    Ok(text.trim().to_string())
}

fn add_remote_at(root: &Path, url: &str) -> Result<(), String> {
    let url = url.trim();
    let lower = url.to_ascii_lowercase();
    if url.is_empty()
        || url.starts_with('-')
        || lower.starts_with("ext::")
        || lower.starts_with("fd::")
        || url.chars().any(|c| c.is_control() || c.is_whitespace())
    {
        return Err("That remote address is not valid.".into());
    }
    git_ok(&git(root, &["remote", "add", "origin", url])?)?;
    Ok(())
}

/// Staged paths that sit outside the project folder, relative to the repository root.
fn outside_staged(root: &Path) -> Result<Vec<String>, String> {
    let output = git(root, &["diff", "--cached", "--name-only", "-z"])?;
    if !output.status.success() {
        return Err(command_text(&output));
    }
    let prefix = repo_prefix(root);
    let text = String::from_utf8_lossy(&output.stdout);
    let mut paths = Vec::new();
    for path in nul_records(&text) {
        let path = path.replace('\\', "/");
        if path.strip_prefix(prefix.as_str()).is_none() {
            paths.push(path);
        }
    }
    Ok(paths)
}

fn outside_staged_error(paths: &[String]) -> String {
    let shown = paths
        .iter()
        .take(20)
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    let more = if paths.len() > 20 { ", …" } else { "" };
    format!(
        "Files outside this project are staged: {shown}{more}. Unstage them in another tool before committing here."
    )
}

fn commit_at(root: &Path, message: &str, amend: bool) -> Result<String, String> {
    let message = message.trim();
    if message.is_empty() && !amend {
        return Err("Write a commit message first.".into());
    }
    if !amend {
        let staged = git(root, &["diff", "--cached", "--name-only"])?;
        if !staged.status.success() {
            return Err(command_text(&staged));
        }
        if String::from_utf8_lossy(&staged.stdout).trim().is_empty() {
            let add = git(root, &["add", "--", "game"])?;
            if !add.status.success() {
                return Err(command_text(&add));
            }
        }
    }
    let outside = outside_staged(root)?;
    if !outside.is_empty() {
        return Err(outside_staged_error(&outside));
    }
    let commit = if amend && message.is_empty() {
        git(root, &["commit", "--amend", "--no-edit"])?
    } else if amend {
        git(root, &["commit", "--amend", "-m", message])?
    } else {
        git(root, &["commit", "-m", message])?
    };
    if !commit.status.success() {
        return Err(command_text(&commit));
    }
    Ok(command_text(&commit))
}

fn undo_at(root: &Path) -> Result<String, String> {
    let head = git(root, &["rev-parse", "--verify", "HEAD"])?;
    if !head.status.success() {
        return Err("There is no commit to undo.".into());
    }
    let subject = git_ok(&git(root, &["log", "-1", "--format=%s"])?)?
        .trim()
        .to_string();
    let parent = git(root, &["rev-parse", "--verify", "HEAD^"])?;
    if parent.status.success() {
        git_ok(&git(root, &["reset", "--soft", "HEAD~1"])?)?;
    } else {
        git_ok(&git(root, &["update-ref", "-d", "HEAD"])?)?;
    }
    Ok(subject)
}

/// A file that is not in a revision reads as empty. So does HEAD in a repository with no commits.
fn show_missing(err: &str) -> bool {
    let err = err.to_lowercase();
    err.contains("does not exist")
        || err.contains("exists on disk")
        || err.contains("not in ")
        || err.contains("invalid object name 'head'")
}

fn read_worktree(root: &Path, rel: &str) -> Result<String, String> {
    let abs = root.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
    match fs::read(&abs) {
        Ok(bytes) => {
            if bytes.len() > GIT_TEXT_MAX {
                return Err("That file is too large to show.".into());
            }
            String::from_utf8(bytes).map_err(|_| "That file is not text.".to_string())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(format!("Could not read {rel}: {e}")),
    }
}

fn show_at(root: &Path, rev: &str, path: &str) -> Result<String, String> {
    let rel = git_path(path)?;
    if rev == "WORKTREE" {
        return read_worktree(root, &rel);
    }
    // `./` makes the path relative to the project folder, not the repository root.
    let spec = if rev == "INDEX" {
        format!(":./{rel}")
    } else {
        format!("{}:./{rel}", git_rev(rev)?)
    };
    let output = git(root, &["show", &spec])?;
    if !output.status.success() {
        let err = command_text(&output);
        if show_missing(&err) {
            return Ok(String::new());
        }
        return Err(err);
    }
    if output.stdout.len() > GIT_TEXT_MAX {
        return Err("That file is too large to show.".into());
    }
    String::from_utf8(output.stdout).map_err(|_| "That file is not text.".to_string())
}

fn index_mode(root: &Path, rel: &str) -> String {
    let Ok(out) = git(root, &["ls-files", "-s", "--", rel]) else {
        return "100644".into();
    };
    if !out.status.success() {
        return "100644".into();
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mode = text.split_whitespace().next().unwrap_or("");
    if mode.len() == 6 && mode.chars().all(|c| c.is_ascii_digit()) {
        mode.to_string()
    } else {
        "100644".into()
    }
}

fn stage_text_at(root: &Path, path: &str, text: &str) -> Result<(), String> {
    let rel = git_path(path)?;
    if text.len() > GIT_TEXT_MAX {
        return Err("That file is too large to stage.".into());
    }
    let hashed = git_stdin(
        root,
        &["hash-object", "-w", "--path", &rel, "--stdin"],
        text.as_bytes(),
    )?;
    let hash = git_ok(&hashed)?.trim().to_string();
    if hash.len() < 40 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("git did not return a blob.".into());
    }
    let mode = index_mode(root, &rel);
    // `--cacheinfo` takes a path from the repository root.
    let from_root = format!("{}{rel}", repo_prefix(root));
    git_ok(&git(
        root,
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &mode,
            &hash,
            &from_root,
        ],
    )?)?;
    Ok(())
}

fn checked_paths(paths: &[String]) -> Result<Vec<String>, String> {
    if paths.is_empty() {
        return Err("Choose a file first.".into());
    }
    paths.iter().map(|p| git_path(p)).collect()
}

fn stage_at(root: &Path, paths: &[String]) -> Result<(), String> {
    let rels = checked_paths(paths)?;
    let mut args = vec!["add", "--"];
    for rel in &rels {
        args.push(rel.as_str());
    }
    git_ok(&git(root, &args)?)?;
    Ok(())
}

/// A second discard of the same path keeps the earlier backup and writes `<name>.<unix-ms>` beside it.
fn spare_backup(backup: &Path) -> PathBuf {
    if !backup.is_file() {
        return backup.to_path_buf();
    }
    let name = backup
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file");
    let ms = SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let mut target = backup.with_file_name(format!("{name}.{ms}"));
    let mut n = 0u32;
    while target.exists() {
        n += 1;
        target = backup.with_file_name(format!("{name}.{ms}.{n}"));
    }
    target
}

fn discard_untracked_file(abs: &Path, backup: &Path, rel: &str) -> Result<(), String> {
    if !abs.is_file() {
        return Err("That untracked path is not a single file.".into());
    }
    let target = spare_backup(backup);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::copy(abs, &target).map_err(|e| format!("Could not back up {rel}: {e}"))?;
    fs::remove_file(abs).map_err(|e| format!("Could not discard {rel}: {e}"))?;
    Ok(())
}

fn parse_commit_files(text: &str) -> Vec<GitCommitFile> {
    let records = nul_records(text);
    let mut files = Vec::new();
    let mut i = 0;
    while i < records.len() {
        let status = records[i];
        i += 1;
        if status.is_empty() || i >= records.len() {
            continue;
        }
        let code = status.chars().next().unwrap_or('M');
        let mut path = records[i];
        i += 1;
        if (code == 'R' || code == 'C') && i < records.len() {
            path = records[i];
            i += 1;
        }
        let path = path.replace('\\', "/");
        if path.is_empty() {
            continue;
        }
        files.push(GitCommitFile {
            status: code.to_string(),
            path,
        });
    }
    files
}

#[tauri::command(async)]
pub fn git_status(state: State<'_, AppState>) -> Result<GitStatus, String> {
    status_at(&git_root(&state)?)
}

#[tauri::command(async)]
pub fn git_init(state: State<'_, AppState>) -> Result<String, String> {
    init_at(&git_root(&state)?)
}

#[tauri::command(async)]
pub fn git_write_ignore(state: State<'_, AppState>) -> Result<(), String> {
    write_ignore_file(&git_root(&state)?)
}

#[tauri::command(async)]
pub fn git_ignore(state: State<'_, AppState>, path: String) -> Result<(), String> {
    ignore_line(&git_root(&state)?, &path)
}

#[tauri::command(async)]
pub fn git_branches(state: State<'_, AppState>) -> Result<Vec<GitBranch>, String> {
    branches_at(&git_root(&state)?)
}

#[tauri::command(async)]
pub fn git_switch(state: State<'_, AppState>, name: String) -> Result<(), String> {
    switch_at(&git_root(&state)?, &name)
}

#[tauri::command(async)]
pub fn git_create_branch(state: State<'_, AppState>, name: String) -> Result<(), String> {
    create_branch_at(&git_root(&state)?, &name)
}

#[tauri::command(async)]
pub fn git_fetch(state: State<'_, AppState>) -> Result<String, String> {
    let root = git_root(&state)?;
    Ok(git_ok(&git_net(&root, &["fetch", "--prune"])?)?
        .trim()
        .to_string())
}

#[tauri::command(async)]
pub fn git_pull(state: State<'_, AppState>) -> Result<String, String> {
    let root = git_root(&state)?;
    Ok(git_ok(&git_net(&root, &["pull", "--ff-only"])?)?
        .trim()
        .to_string())
}

#[tauri::command(async)]
pub fn git_push(state: State<'_, AppState>) -> Result<String, String> {
    push_at(&git_root(&state)?)
}

#[tauri::command(async)]
pub fn git_add_remote(state: State<'_, AppState>, url: String) -> Result<(), String> {
    add_remote_at(&git_root(&state)?, &url)
}

#[tauri::command(async)]
pub fn git_commit(
    state: State<'_, AppState>,
    message: String,
    amend: bool,
) -> Result<String, String> {
    commit_at(&git_root(&state)?, &message, amend)
}

#[tauri::command(async)]
pub fn git_undo_commit(state: State<'_, AppState>) -> Result<String, String> {
    undo_at(&git_root(&state)?)
}

#[tauri::command(async)]
pub fn git_stage(state: State<'_, AppState>, paths: Vec<String>) -> Result<(), String> {
    stage_at(&git_root(&state)?, &paths)
}

#[tauri::command(async)]
pub fn git_unstage(state: State<'_, AppState>, paths: Vec<String>) -> Result<(), String> {
    let root = git_root(&state)?;
    let rels = checked_paths(&paths)?;
    let head = git(&root, &["rev-parse", "--verify", "HEAD"])?;
    let mut args = if head.status.success() {
        vec!["restore", "--staged", "--"]
    } else {
        vec!["rm", "--cached", "--"]
    };
    for rel in &rels {
        args.push(rel.as_str());
    }
    git_ok(&git(&root, &args)?)?;
    Ok(())
}

#[tauri::command(async)]
pub fn git_discard(app: AppHandle, state: State<'_, AppState>, path: String) -> Result<(), String> {
    let rel = git_path(&path)?;
    let root = git_root(&state)?;
    let status = git(
        &root,
        &["status", "--porcelain", "--untracked-files=all", "--", &rel],
    )?;
    if !status.status.success() {
        return Err(command_text(&status));
    }
    let line = String::from_utf8_lossy(&status.stdout);
    let untracked = line.lines().any(|row| row.starts_with("??"));
    let abs = root.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
    if untracked {
        let backup_root = commands::backup_dir(&app, &root)?;
        let backup = backup_root.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
        discard_untracked_file(&abs, &backup, &rel)?;
        return Ok(());
    }
    git_ok(&git(&root, &["restore", "--", &rel])?)?;
    Ok(())
}

#[tauri::command(async)]
pub fn git_show(state: State<'_, AppState>, rev: String, path: String) -> Result<String, String> {
    show_at(&git_root(&state)?, &rev, &path)
}

#[tauri::command(async)]
pub fn git_stage_text(
    state: State<'_, AppState>,
    path: String,
    text: String,
) -> Result<(), String> {
    stage_text_at(&git_root(&state)?, &path, &text)
}

#[tauri::command(async)]
pub fn git_log(
    state: State<'_, AppState>,
    path: Option<String>,
    limit: u32,
) -> Result<Vec<GitCommit>, String> {
    let root = git_root(&state)?;
    let limit = limit.clamp(1, 100);
    let n = limit.to_string();
    let rel = match path {
        Some(path) if !path.trim().is_empty() => Some(git_path(&path)?),
        _ => None,
    };
    let head = git(&root, &["rev-parse", "--verify", "HEAD"])?;
    if !head.status.success() {
        return Ok(Vec::new());
    }
    let mut args = vec![
        "log",
        "-n",
        n.as_str(),
        "--format=%H%x1f%an%x1f%ad%x1f%s",
        "--date=short",
    ];
    if let Some(rel) = &rel {
        args.push("--");
        args.push(rel.as_str());
    }
    let text = git_ok(&git(&root, &args)?)?;
    let mut commits = Vec::new();
    for line in text.lines() {
        let mut parts = line.split('\u{1f}');
        let Some(hash) = parts.next() else { continue };
        if hash.is_empty() {
            continue;
        }
        commits.push(GitCommit {
            hash: hash.to_string(),
            author: parts.next().unwrap_or("").to_string(),
            date: parts.next().unwrap_or("").to_string(),
            subject: parts.next().unwrap_or("").to_string(),
        });
    }
    Ok(commits)
}

#[tauri::command(async)]
pub fn git_commit_files(
    state: State<'_, AppState>,
    rev: String,
) -> Result<Vec<GitCommitFile>, String> {
    let root = git_root(&state)?;
    let rev = git_rev(&rev)?;
    let text = git_ok(&git(
        &root,
        &[
            "show",
            "--name-status",
            "-z",
            "--relative",
            "--format=",
            &rev,
        ],
    )?)?;
    Ok(parse_commit_files(&text))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let dir = std::env::temp_dir().join(format!(
                "vnide-git-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0)
            ));
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn identity(root: &Path) {
        git_ok(&git(root, &["config", "user.email", "vn-ide@example.com"]).unwrap()).unwrap();
        git_ok(&git(root, &["config", "user.name", "vn-ide"]).unwrap()).unwrap();
    }

    #[test]
    fn branch_header_tracks_ahead_and_behind() {
        let head = parse_branch_header("main...origin/main [ahead 1, behind 2]");
        assert_eq!(head.name, "main");
        assert_eq!(head.upstream.as_deref(), Some("origin/main"));
        assert_eq!(head.ahead, 1);
        assert_eq!(head.behind, 2);
    }

    #[test]
    fn branch_header_covers_a_plain_branch_an_empty_repo_and_a_detached_head() {
        let plain = parse_branch_header("main");
        assert_eq!(plain.name, "main");
        assert!(plain.upstream.is_none());
        assert_eq!(plain.ahead, 0);

        let empty = parse_branch_header("No commits yet on master");
        assert_eq!(empty.name, "master");
        assert!(empty.upstream.is_none());

        let detached = parse_branch_header("HEAD (no branch)");
        assert_eq!(detached.name, "HEAD");
    }

    #[test]
    fn init_writes_a_renpy_gitignore_that_hides_rpyc() {
        let dir = TempDir::new();
        init_at(&dir.0).unwrap();
        let ignore = fs::read_to_string(dir.0.join(".gitignore")).unwrap();
        assert!(ignore.contains("*.rpyc"));
        assert!(ignore.contains("game/saves/"));
        fs::write(dir.0.join("compiled.rpyc"), "x").unwrap();
        fs::create_dir_all(dir.0.join("game")).unwrap();
        fs::write(dir.0.join("game").join("script.rpy"), "label start:\n").unwrap();
        let status = status_at(&dir.0).unwrap();
        assert!(status.has_ignore);
        assert!(status.changes.iter().any(|c| c.path == "game/script.rpy"));
        assert!(status.changes.iter().all(|c| !c.path.ends_with(".rpyc")));
        assert!(write_ignore_file(&dir.0).is_err());
    }

    #[test]
    fn stage_text_stages_one_change_and_leaves_the_rest() {
        let dir = TempDir::new();
        init_at(&dir.0).unwrap();
        identity(&dir.0);
        fs::create_dir_all(dir.0.join("game")).unwrap();
        fs::write(dir.0.join("game").join("script.rpy"), "line1\nline2\n").unwrap();
        git_ok(&git(&dir.0, &["add", "--", "game/script.rpy"]).unwrap()).unwrap();
        git_ok(&git(&dir.0, &["commit", "-m", "first"]).unwrap()).unwrap();
        fs::write(
            dir.0.join("game").join("script.rpy"),
            "line1\nchanged\nline3\n",
        )
        .unwrap();
        stage_text_at(&dir.0, "game/script.rpy", "line1\nline2\nline3\n").unwrap();
        let cached =
            git_ok(&git(&dir.0, &["diff", "--cached", "--", "game/script.rpy"]).unwrap()).unwrap();
        let work = git_ok(&git(&dir.0, &["diff", "--", "game/script.rpy"]).unwrap()).unwrap();
        assert!(cached.contains("line3"), "{cached}");
        assert!(!cached.contains("changed"), "{cached}");
        assert!(work.contains("changed"), "{work}");
    }

    #[test]
    fn undo_removes_the_first_commit_and_keeps_the_index() {
        let dir = TempDir::new();
        init_at(&dir.0).unwrap();
        identity(&dir.0);
        fs::create_dir_all(dir.0.join("game")).unwrap();
        fs::write(dir.0.join("game").join("script.rpy"), "label start:\n").unwrap();
        git_ok(&git(&dir.0, &["add", "--", "game/script.rpy"]).unwrap()).unwrap();
        git_ok(&git(&dir.0, &["commit", "-m", "first"]).unwrap()).unwrap();
        let subject = undo_at(&dir.0).unwrap();
        assert_eq!(subject, "first");
        assert!(!git(&dir.0, &["rev-parse", "--verify", "HEAD"])
            .unwrap()
            .status
            .success());
        let cached = git_ok(&git(&dir.0, &["diff", "--cached", "--name-only"]).unwrap()).unwrap();
        assert!(cached.contains("game/script.rpy"), "{cached}");
        assert!(fs::read_to_string(dir.0.join("game").join("script.rpy"))
            .unwrap()
            .contains("label start"));
    }

    #[test]
    fn staged_diff_in_a_repo_without_commits_reads_head_as_empty() {
        let dir = TempDir::new();
        init_at(&dir.0).unwrap();
        fs::create_dir_all(dir.0.join("game")).unwrap();
        fs::write(dir.0.join("game").join("script.rpy"), "label start:\n").unwrap();
        git_ok(&git(&dir.0, &["add", "--", "game/script.rpy"]).unwrap()).unwrap();
        assert_eq!(show_at(&dir.0, "HEAD", "game/script.rpy").unwrap(), "");
        assert!(show_at(&dir.0, "INDEX", "game/script.rpy")
            .unwrap()
            .contains("label start"));
    }

    #[test]
    fn a_project_inside_a_larger_repo_sees_its_own_paths() {
        let dir = TempDir::new();
        init_at(&dir.0).unwrap();
        identity(&dir.0);
        let project = dir.0.join("games").join("demo");
        fs::create_dir_all(project.join("game")).unwrap();
        fs::write(project.join("game").join("script.rpy"), "label start:\n").unwrap();
        fs::write(dir.0.join("outside.txt"), "x").unwrap();
        git_ok(&git(&project, &["add", "--", "game/script.rpy"]).unwrap()).unwrap();
        git_ok(&git(&project, &["commit", "-m", "first"]).unwrap()).unwrap();

        fs::write(
            project.join("game").join("script.rpy"),
            "label start:\n    pass\n",
        )
        .unwrap();
        let status = status_at(&project).unwrap();
        let paths: Vec<&str> = status.changes.iter().map(|c| c.path.as_str()).collect();
        assert_eq!(paths, vec!["game/script.rpy"], "{paths:?}");

        assert!(show_at(&project, "HEAD", "game/script.rpy")
            .unwrap()
            .ends_with("start:\n"));
        stage_text_at(&project, "game/script.rpy", "label start:\n    pass\n").unwrap();
        let cached = git_ok(&git(&project, &["diff", "--cached", "--name-only"]).unwrap()).unwrap();
        assert!(cached.contains("games/demo/game/script.rpy"), "{cached}");
        let files = git_ok(
            &git(
                &project,
                &["show", "--name-status", "--relative", "--format=", "HEAD"],
            )
            .unwrap(),
        )
        .unwrap();
        assert!(
            files.contains("game/script.rpy") && !files.contains("games/demo"),
            "{files}"
        );
    }

    #[test]
    fn show_of_a_missing_path_is_empty() {
        let dir = TempDir::new();
        init_at(&dir.0).unwrap();
        identity(&dir.0);
        fs::create_dir_all(dir.0.join("game")).unwrap();
        fs::write(dir.0.join("game").join("script.rpy"), "label start:\n").unwrap();
        git_ok(&git(&dir.0, &["add", "--", "game/script.rpy"]).unwrap()).unwrap();
        git_ok(&git(&dir.0, &["commit", "-m", "first"]).unwrap()).unwrap();
        assert_eq!(show_at(&dir.0, "INDEX", "game/missing.rpy").unwrap(), "");
        assert_eq!(show_at(&dir.0, "HEAD", "game/missing.rpy").unwrap(), "");
        assert_eq!(show_at(&dir.0, "WORKTREE", "game/missing.rpy").unwrap(), "");
        assert!(show_at(&dir.0, "INDEX", "game/script.rpy")
            .unwrap()
            .contains("label start"));
    }

    #[test]
    fn status_reads_non_ascii_and_spaced_names() {
        let dir = TempDir::new();
        init_at(&dir.0).unwrap();
        fs::create_dir_all(dir.0.join("game")).unwrap();
        fs::write(dir.0.join("game").join("scène.rpy"), "x").unwrap();
        fs::write(dir.0.join("game").join("a b.rpy"), "y").unwrap();
        let status = status_at(&dir.0).unwrap();
        let mut paths: Vec<String> = status.changes.iter().map(|c| c.path.clone()).collect();
        paths.sort();
        assert!(paths.iter().any(|p| p == "game/scène.rpy"), "{paths:?}");
        assert!(paths.iter().any(|p| p == "game/a b.rpy"), "{paths:?}");
        stage_at(&dir.0, &paths).unwrap();
        let staged = status_at(&dir.0).unwrap();
        assert!(
            staged.changes.iter().all(|c| c.staged),
            "{:?}",
            staged.changes
        );
        assert!(staged.changes.iter().any(|c| c.path == "game/scène.rpy"));
    }

    #[test]
    fn status_reports_a_rename_by_its_new_name() {
        let dir = TempDir::new();
        init_at(&dir.0).unwrap();
        identity(&dir.0);
        fs::create_dir_all(dir.0.join("game")).unwrap();
        fs::write(dir.0.join("game").join("old.rpy"), "x").unwrap();
        git_ok(&git(&dir.0, &["add", "--", "game/old.rpy"]).unwrap()).unwrap();
        git_ok(&git(&dir.0, &["commit", "-m", "first"]).unwrap()).unwrap();
        git_ok(&git(&dir.0, &["mv", "game/old.rpy", "game/new.rpy"]).unwrap()).unwrap();
        let status = status_at(&dir.0).unwrap();
        let renamed = status.changes.iter().find(|c| c.path == "game/new.rpy");
        assert!(renamed.is_some(), "{:?}", status.changes);
        let renamed = renamed.unwrap();
        assert_eq!(renamed.status, "R");
        assert!(renamed.staged);
        assert!(status.changes.iter().all(|c| c.path != "game/old.rpy"));

        git_ok(&git(&dir.0, &["commit", "-m", "rename"]).unwrap()).unwrap();
        let shown = git_ok(
            &git(
                &dir.0,
                &[
                    "show",
                    "--name-status",
                    "-z",
                    "--relative",
                    "--format=",
                    "HEAD",
                ],
            )
            .unwrap(),
        )
        .unwrap();
        let files = parse_commit_files(&shown);
        assert_eq!(files.len(), 1, "{files:?} from {shown:?}");
        assert_eq!(files[0].path, "game/new.rpy");
        assert_eq!(files[0].status, "R");
    }

    #[test]
    fn git_rev_rejects_option_like_input() {
        assert!(git_rev("-hack").is_err());
        assert!(git_rev("--all").is_err());
        assert_eq!(git_rev("HEAD").unwrap(), "HEAD");
        assert_eq!(git_rev("abc123^").unwrap(), "abc123^");
        assert!(add_remote_at(&TempDir::new().0, "-o").is_err());
        assert!(add_remote_at(&TempDir::new().0, "ext::sh").is_err());
        assert!(add_remote_at(&TempDir::new().0, "fd::3").is_err());
    }

    #[test]
    fn commit_refuses_when_outside_files_are_staged() {
        let dir = TempDir::new();
        init_at(&dir.0).unwrap();
        identity(&dir.0);
        let project = dir.0.join("games").join("demo");
        fs::create_dir_all(project.join("game")).unwrap();
        fs::write(project.join("game").join("script.rpy"), "label start:\n").unwrap();
        fs::write(dir.0.join("outside.txt"), "x").unwrap();
        git_ok(&git(&dir.0, &["add", "--", "outside.txt"]).unwrap()).unwrap();
        let err = commit_at(&project, "nope", false).unwrap_err();
        assert!(err.contains("outside.txt"), "{err}");
        assert!(err.contains("outside this project"), "{err}");
        let status = status_at(&project).unwrap();
        assert_eq!(status.outside_staged, vec!["outside.txt".to_string()]);
        assert!(!git(&project, &["rev-parse", "--verify", "HEAD"])
            .unwrap()
            .status
            .success());
    }

    #[test]
    fn push_remote_prefers_origin_then_single_remote() {
        let dir = TempDir::new();
        init_at(&dir.0).unwrap();
        let branch = status_at(&dir.0).unwrap().branch;
        assert!(choose_push_remote(&dir.0, &branch, &[]).is_none());

        git_ok(
            &git(
                &dir.0,
                &["remote", "add", "upstream", "https://example.com/a.git"],
            )
            .unwrap(),
        )
        .unwrap();
        let one = remotes_at(&dir.0);
        assert_eq!(
            choose_push_remote(&dir.0, &branch, &one).as_deref(),
            Some("upstream")
        );
        assert_eq!(
            status_at(&dir.0).unwrap().push_remote.as_deref(),
            Some("upstream")
        );

        git_ok(
            &git(
                &dir.0,
                &["remote", "add", "origin", "https://example.com/b.git"],
            )
            .unwrap(),
        )
        .unwrap();
        let two = remotes_at(&dir.0);
        assert_eq!(
            choose_push_remote(&dir.0, &branch, &two).as_deref(),
            Some("origin")
        );

        git_ok(
            &git(
                &dir.0,
                &["config", &format!("branch.{branch}.pushRemote"), "other"],
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            choose_push_remote(&dir.0, &branch, &two).as_deref(),
            Some("other")
        );
    }

    #[test]
    fn switch_to_remote_branch_uses_existing_local() {
        let dir = TempDir::new();
        init_at(&dir.0).unwrap();
        identity(&dir.0);
        fs::create_dir_all(dir.0.join("game")).unwrap();
        fs::write(dir.0.join("game").join("script.rpy"), "label start:\n").unwrap();
        git_ok(&git(&dir.0, &["add", "--", "game/script.rpy"]).unwrap()).unwrap();
        git_ok(&git(&dir.0, &["commit", "-m", "first"]).unwrap()).unwrap();
        git_ok(&git(&dir.0, &["branch", "feature"]).unwrap()).unwrap();
        git_ok(
            &git(
                &dir.0,
                &["remote", "add", "origin", "https://example.com/demo.git"],
            )
            .unwrap(),
        )
        .unwrap();
        git_ok(
            &git(
                &dir.0,
                &["update-ref", "refs/remotes/origin/feature", "HEAD"],
            )
            .unwrap(),
        )
        .unwrap();
        switch_at(&dir.0, "origin/feature").unwrap();
        assert_eq!(status_at(&dir.0).unwrap().branch, "feature");
    }

    #[test]
    fn head_pushed_tracks_upstream() {
        let work = TempDir::new();
        let bare = TempDir::new();
        git_ok(&git(&bare.0, &["init", "--bare"]).unwrap()).unwrap();
        init_at(&work.0).unwrap();
        identity(&work.0);
        fs::create_dir_all(work.0.join("game")).unwrap();
        fs::write(work.0.join("game").join("script.rpy"), "label start:\n").unwrap();
        git_ok(&git(&work.0, &["add", "--", "game/script.rpy"]).unwrap()).unwrap();
        git_ok(&git(&work.0, &["commit", "-m", "first"]).unwrap()).unwrap();
        let url = bare.0.to_string_lossy().replace('\\', "/");
        git_ok(&git(&work.0, &["remote", "add", "origin", &url]).unwrap()).unwrap();
        let branch = status_at(&work.0).unwrap().branch;
        git_ok(&git(&work.0, &["push", "-u", "origin", &branch]).unwrap()).unwrap();
        let status = status_at(&work.0).unwrap();
        assert!(status.head_pushed, "upstream {:?}", status.upstream);
        assert_eq!(status.ahead, 0);

        fs::write(
            work.0.join("game").join("script.rpy"),
            "label start:\n    pass\n",
        )
        .unwrap();
        git_ok(&git(&work.0, &["add", "--", "game/script.rpy"]).unwrap()).unwrap();
        git_ok(&git(&work.0, &["commit", "-m", "second"]).unwrap()).unwrap();
        let status = status_at(&work.0).unwrap();
        assert!(!status.head_pushed);
        assert_eq!(status.ahead, 1);
    }

    #[test]
    fn discard_keeps_every_backup() {
        let dir = TempDir::new();
        let file = dir.0.join("script.rpy");
        let backup = dir.0.join("backup").join("script.rpy");
        fs::write(&file, "first").unwrap();
        discard_untracked_file(&file, &backup, "script.rpy").unwrap();
        assert_eq!(fs::read_to_string(&backup).unwrap(), "first");
        assert!(!file.exists());

        fs::write(&file, "second").unwrap();
        discard_untracked_file(&file, &backup, "script.rpy").unwrap();
        assert_eq!(fs::read_to_string(&backup).unwrap(), "first");
        let kept = fs::read_dir(backup.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("script.rpy.")
                    && fs::read_to_string(path).ok().as_deref() == Some("second")
            });
        assert!(
            kept.is_some(),
            "second copy was not kept beside the first backup"
        );
    }
}
