//! Download, check and run the ty language server.
//!
//! ty speaks the Language Server Protocol over stdio. The editor sends a
//! Python-only copy of each script; this module frames those messages and
//! answers the `workspace/configuration` request ty sends as soon as it starts.

use crate::error::AppError;
use std::fs::{self, File};
use std::io::{self, BufRead, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::commands::AppState;
use crate::process;

/// Release that includes the fix for GHSA-vxvm-j4xq-q7m4. Bumping is deliberate.
const TY_VERSION: &str = "0.0.84";
const MAX_FRAME: usize = 16 * 1024 * 1024;
const USER_AGENT: &str = "Ren-Inspector/0.1";

/// One running ty process. The reader thread restarts it once after a crash.
#[derive(Default)]
pub struct Proc {
    child: Option<Child>,
    /// Frames waiting for the writer thread, which owns ty's stdin.
    tx: Option<mpsc::Sender<Vec<u8>>>,
    generation: u64,
    crashes: u32,
    /// Set when the user or a project change stopped the process, so the exit
    /// is not treated as a crash.
    stop: bool,
    exe: PathBuf,
    /// Answer for ty's `workspace/configuration` request.
    settings: Option<Value>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PyStatus {
    pub installed: bool,
    pub version: &'static str,
    pub running: bool,
    pub exe: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PyEnv {
    pub script_version: Option<String>,
    pub python_version: Option<String>,
    pub sdk_root: Option<String>,
    /// Set when the project's Ren'Py is too old for ty (Python 2).
    pub unsupported: bool,
    pub reason: String,
}

#[derive(Clone, Serialize)]
struct Progress {
    phase: String,
    done: u64,
    total: u64,
    label: String,
}

pub fn stop(state: &AppState) {
    let Ok(mut proc) = state.pylsp.lock() else {
        return;
    };
    proc.stop = true;
    proc.tx.take();
    if let Some(mut child) = proc.child.take() {
        let _ = child.kill();
        let _ = child.wait();
    }
}

fn tools_dir(app: &AppHandle) -> Result<PathBuf, AppError> {
    Ok(crate::util::data_dir(app)?
        .join("tools")
        .join("ty")
        .join(TY_VERSION))
}

fn exe_in(dir: &Path) -> PathBuf {
    dir.join(if cfg!(windows) { "ty.exe" } else { "ty" })
}

fn installed_exe(app: &AppHandle) -> Result<Option<PathBuf>, AppError> {
    let path = exe_in(&tools_dir(app)?);
    Ok(path.is_file().then_some(path))
}

#[tauri::command]
pub fn pylsp_status(app: AppHandle, state: State<'_, AppState>) -> Result<PyStatus, AppError> {
    let exe = installed_exe(&app)?;
    let running = crate::util::lock(&state.pylsp).child.is_some();
    Ok(PyStatus {
        installed: exe.is_some(),
        version: TY_VERSION,
        running,
        exe: exe
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default(),
    })
}

#[tauri::command(async)]
pub fn pylsp_env(state: State<'_, AppState>) -> Result<PyEnv, AppError> {
    let guard = crate::util::lock(&state.project);
    let project = guard.as_ref().ok_or_else(crate::util::no_project)?;
    // The engine that runs the game decides the Python version. Most projects have no script_version.txt.
    let script = project
        .engine_version
        .clone()
        .or_else(|| project.script_version.clone());
    let python = script.as_deref().and_then(python_for_renpy);
    let sdk = project
        .launcher
        .as_ref()
        .and_then(|launcher| crate::sdk::sdk_root(&launcher.exe))
        .map(|p| p.to_string_lossy().into_owned());
    let unsupported = python.is_none() && script.as_deref().is_some_and(|v| v.starts_with('7'));
    let reason = if unsupported {
        "Ren'Py 7 uses Python 2, which this language server cannot read.".into()
    } else {
        String::new()
    };
    Ok(PyEnv {
        script_version: script,
        python_version: python.map(str::to_string),
        sdk_root: sdk,
        unsupported,
        reason,
    })
}

#[tauri::command(async)]
pub fn pylsp_install(
    app: AppHandle,
    state: State<'_, AppState>,
    force: bool,
) -> Result<(), AppError> {
    if state.pylsp_busy.swap(true, Ordering::SeqCst) {
        return Err("A download is already running.".into());
    }
    state.pylsp_cancel.store(false, Ordering::SeqCst);
    let result = install(&app, &state.pylsp_cancel, force);
    state.pylsp_busy.store(false, Ordering::SeqCst);
    result
}

#[tauri::command]
pub fn pylsp_cancel(state: State<'_, AppState>) {
    state.pylsp_cancel.store(true, Ordering::SeqCst);
}

#[tauri::command(async)]
pub fn pylsp_remove(app: AppHandle, state: State<'_, AppState>) -> Result<(), AppError> {
    stop(&state);
    let dir = tools_dir(&app)?;
    if dir.exists() {
        fs::remove_dir_all(&dir).map_err(|e| format!("Could not remove ty: {e}"))?;
    }
    Ok(())
}

#[tauri::command(async)]
pub fn pylsp_start(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: Option<Value>,
) -> Result<(), AppError> {
    let exe = installed_exe(&app)?.ok_or("The Python language server is not installed.")?;
    {
        let proc = crate::util::lock(&state.pylsp);
        if proc.child.is_some() && !proc.stop {
            return Ok(());
        }
    }
    launch(&app, &state, &exe, settings.unwrap_or(Value::Null), true)
}

#[tauri::command]
pub fn pylsp_send(state: State<'_, AppState>, message: Value) -> Result<(), AppError> {
    let text = serde_json::to_string(&message)?;
    if text.len() > MAX_FRAME {
        return Err("That message is too large to send.".into());
    }
    let proc = crate::util::lock(&state.pylsp);
    let Some(tx) = proc.tx.as_ref() else {
        return Err("The Python language server is not running.".into());
    };
    // Queued, not written here: a slow ty must not hold the lock or the main thread.
    tx.send(text.into_bytes())
        .map_err(|_| crate::error::AppError::new("The Python language server is not running."))
}

#[tauri::command]
pub fn pylsp_stop(state: State<'_, AppState>) {
    stop(&state);
}

fn install(app: &AppHandle, cancel: &AtomicBool, force: bool) -> Result<(), AppError> {
    let dest = tools_dir(app)?;
    if !force && exe_in(&dest).is_file() {
        return Ok(());
    }
    let asset = host_asset()?;
    let url = format!("https://github.com/astral-sh/ty/releases/download/{TY_VERSION}/{asset}");
    let sum_url = format!("{url}.sha256");
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    // `0.0.84`.with_extension() would treat `84` as the extension, so name the file explicitly.
    let archive = dest.parent().unwrap_or(Path::new(".")).join(format!(
        "{TY_VERSION}.{}",
        if asset.ends_with(".zip") {
            "zip"
        } else {
            "tar.gz"
        }
    ));
    emit(
        app,
        "download",
        0,
        0,
        &format!("Downloading ty {TY_VERSION}"),
    );
    download(app, cancel, &url, &archive)?;
    emit(app, "check", 0, 0, "Checking the download");
    let sums = http_text(&sum_url)?;
    let expected = sha256_sidecar(&sums).ok_or("The published checksum was not readable.")?;
    let got = sha256_file(&archive)?;
    if !got.eq_ignore_ascii_case(&expected) {
        let _ = fs::remove_file(&archive);
        return Err("The download did not match its published checksum.".into());
    }
    let staging = dest.with_file_name(format!(".{TY_VERSION}.partial"));
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir_all(&staging)?;
    emit(app, "extract", 0, 0, "Unpacking ty");
    let extracted = if asset.ends_with(".zip") {
        extract_zip(&archive, &staging)
    } else {
        extract_tar_gz(&archive, &staging)
    };
    if let Err(e) = extracted {
        let _ = fs::remove_dir_all(&staging);
        let _ = fs::remove_file(&archive);
        return Err(e);
    }
    let found = find_binary(&staging).ok_or("The archive did not contain ty.")?;
    let final_exe = exe_in(&staging);
    if found != final_exe {
        fs::rename(&found, &final_exe)?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&final_exe, fs::Permissions::from_mode(0o755));
    }
    if dest.exists() {
        fs::remove_dir_all(&dest)?;
    }
    fs::rename(&staging, &dest).map_err(|e| format!("Could not install ty: {e}"))?;
    let _ = fs::remove_file(&archive);
    emit(app, "done", 1, 1, "ty is installed");
    Ok(())
}

fn launch(
    app: &AppHandle,
    state: &AppState,
    exe: &Path,
    settings: Value,
    reset_crashes: bool,
) -> Result<(), AppError> {
    let mut cmd = process::background(exe);
    cmd.arg("server")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Could not start ty: {e}"))?;
    let stdout = child.stdout.take().ok_or("ty returned no output.")?;
    let stdin = child.stdin.take().ok_or("ty took no input.")?;
    if let Some(err) = child.stderr.take() {
        thread::spawn(move || drain_stderr(err));
    }
    let generation = {
        let mut proc = crate::util::lock(&state.pylsp);
        proc.stop = false;
        proc.exe = exe.to_path_buf();
        proc.settings = Some(settings);
        if reset_crashes {
            proc.crashes = 0;
        }
        proc.generation = proc.generation.wrapping_add(1);
        proc.tx = Some(spawn_writer(stdin));
        proc.child = Some(child);
        proc.generation
    };
    spawn_reader(app.clone(), stdout, generation);
    Ok(())
}

fn spawn_reader(app: AppHandle, stdout: ChildStdout, generation: u64) {
    thread::spawn(move || {
        let mut frame = FrameBuf::default();
        let mut chunk = [0u8; 8192];
        let mut stdout = stdout;
        loop {
            match stdout.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => match frame.push(&chunk[..n]) {
                    Ok(messages) => {
                        for message in messages {
                            handle_message(&app, generation, &message);
                        }
                    }
                    Err(e) => {
                        log::warn!("ty frame: {e}");
                        break;
                    }
                },
                Err(e) => {
                    log::warn!("ty read: {e}");
                    break;
                }
            }
        }
        on_exit(&app, generation);
    });
}

fn handle_message(app: &AppHandle, generation: u64, message: &str) {
    let Ok(value) = serde_json::from_str::<Value>(message) else {
        return;
    };
    if value.get("method").is_some() && value.get("id").is_some() {
        reply_request(app, generation, &value);
        return;
    }
    let _ = app.emit("pylsp:message", value);
}

fn reply_request(app: &AppHandle, generation: u64, request: &Value) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let Ok(proc) = state.pylsp.lock() else {
        return;
    };
    if proc.generation != generation || proc.stop {
        return;
    }
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let method = request.get("method").and_then(Value::as_str).unwrap_or("");
    let result = if method == "workspace/configuration" {
        let settings = proc.settings.clone().unwrap_or(Value::Null);
        let items = request
            .pointer("/params/items")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        Value::Array(
            items
                .iter()
                .map(|item| {
                    let section = item.get("section").and_then(Value::as_str).unwrap_or("");
                    if section == "ty" || section.starts_with("ty.") {
                        settings.clone()
                    } else {
                        Value::Null
                    }
                })
                .collect(),
        )
    } else {
        Value::Null
    };
    drop(proc);
    let body = json!({ "jsonrpc": "2.0", "id": id, "result": result });
    let Ok(text) = serde_json::to_string(&body) else {
        return;
    };
    let Ok(proc) = state.pylsp.lock() else {
        return;
    };
    if proc.generation != generation {
        return;
    }
    if let Some(tx) = proc.tx.as_ref() {
        let _ = tx.send(text.into_bytes());
    }
}

/// Owns ty's stdin and writes queued frames in order. Ends when the sender is dropped.
fn spawn_writer(mut stdin: ChildStdin) -> mpsc::Sender<Vec<u8>> {
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    thread::spawn(move || {
        while let Ok(body) = rx.recv() {
            if write_frame(&mut stdin, &body).is_err() {
                break;
            }
        }
    });
    tx
}

fn on_exit(app: &AppHandle, generation: u64) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let restart = {
        let Ok(mut proc) = state.pylsp.lock() else {
            return;
        };
        if proc.generation != generation || proc.stop {
            return;
        }
        proc.child.take();
        proc.tx.take();
        proc.crashes = proc.crashes.saturating_add(1);
        proc.crashes <= 1
    };
    if !restart {
        let _ = app.emit(
            "pylsp:status",
            json!({ "state": "failed", "detail": "The language server stopped." }),
        );
        return;
    }
    let exe = {
        let Ok(proc) = state.pylsp.lock() else {
            return;
        };
        proc.exe.clone()
    };
    let settings = {
        let Ok(proc) = state.pylsp.lock() else {
            return;
        };
        proc.settings.clone().unwrap_or(Value::Null)
    };
    if launch(app, &state, &exe, settings, false).is_err() {
        let _ = app.emit(
            "pylsp:status",
            json!({ "state": "failed", "detail": "The language server could not be restarted." }),
        );
        return;
    }
    let _ = app.emit("pylsp:status", json!({ "state": "restarted" }));
}

fn drain_stderr(err: impl Read) {
    let mut err = io::BufReader::new(err);
    let mut line = String::new();
    while err.read_line(&mut line).unwrap_or(0) > 0 {
        log::info!(target: "ty", "{}", line.trim_end());
        line.clear();
    }
}

fn write_frame(stdin: &mut impl Write, body: &[u8]) -> io::Result<()> {
    write!(stdin, "Content-Length: {}\r\n\r\n", body.len())?;
    stdin.write_all(body)?;
    stdin.flush()
}

fn emit(app: &AppHandle, phase: &str, done: u64, total: u64, label: &str) {
    let _ = app.emit(
        "pylsp:progress",
        Progress {
            phase: phase.into(),
            done,
            total,
            label: label.into(),
        },
    );
}

fn download(app: &AppHandle, cancel: &AtomicBool, url: &str, dest: &Path) -> Result<(), AppError> {
    if cancel.load(Ordering::Relaxed) {
        return Err("Download cancelled.".into());
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    let response = http()
        .get(url)
        .set("User-Agent", USER_AGENT)
        .call()
        .map_err(|e| format!("Could not download {url}: {e}"))?;
    let total = response
        .header("Content-Length")
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);
    let mut reader = response.into_reader();
    let mut file = File::create(dest)?;
    let mut buf = [0u8; 64 * 1024];
    let mut done = 0u64;
    let mut next = 0u64;
    loop {
        if cancel.load(Ordering::Relaxed) {
            drop(file);
            let _ = fs::remove_file(dest);
            return Err("Download cancelled.".into());
        }
        let n = reader.read(&mut buf).map_err(|e| {
            let _ = fs::remove_file(dest);
            format!("Download stopped: {e}")
        })?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])?;
        done += n as u64;
        if done >= next {
            emit(
                app,
                "download",
                done,
                total,
                &format!("Downloading ty {TY_VERSION}"),
            );
            next = done.saturating_add(512 * 1024);
        }
    }
    file.flush()?;
    Ok(())
}

fn http() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(20))
        .timeout_read(Duration::from_secs(120))
        .build()
}

fn http_text(url: &str) -> Result<String, AppError> {
    http()
        .get(url)
        .set("User-Agent", USER_AGENT)
        .call()
        .map_err(|e| format!("Could not reach {url}: {e}"))?
        .into_string()
        .map_err(crate::error::AppError::from)
}

fn sha256_file(path: &Path) -> Result<String, AppError> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// `hash  filename` or `hash *filename`, as published next to a ty release.
pub fn sha256_sidecar(text: &str) -> Option<String> {
    let token = text.split_whitespace().next()?;
    if token.len() == 64 && token.chars().all(|c| c.is_ascii_hexdigit()) {
        Some(token.to_string())
    } else {
        None
    }
}

/// Python that this Ren'Py release embeds. Ren'Py 7 is Python 2, which ty cannot read.
pub fn python_for_renpy(version: &str) -> Option<&'static str> {
    let mut parts = version.split('.');
    let major: u32 = parts.next()?.parse().ok()?;
    let minor: u32 = parts.next().unwrap_or("0").parse().unwrap_or(0);
    if major < 8 {
        None
    } else if major == 8 && minor < 3 {
        Some("3.9")
    } else {
        Some("3.12")
    }
}

pub fn asset_name(os: &str, arch: &str) -> Option<&'static str> {
    match (os, arch) {
        ("windows", "x86_64") => Some("ty-x86_64-pc-windows-msvc.zip"),
        ("windows", "aarch64") => Some("ty-aarch64-pc-windows-msvc.zip"),
        ("linux", "x86_64") => Some("ty-x86_64-unknown-linux-gnu.tar.gz"),
        ("linux", "aarch64") => Some("ty-aarch64-unknown-linux-gnu.tar.gz"),
        ("macos", "x86_64") => Some("ty-x86_64-apple-darwin.tar.gz"),
        ("macos", "aarch64") => Some("ty-aarch64-apple-darwin.tar.gz"),
        _ => None,
    }
}

fn host_asset() -> Result<&'static str, AppError> {
    let os = if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "other"
    };
    let arch = if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        "other"
    };
    asset_name(os, arch).ok_or_else(|| {
        crate::error::AppError::new(format!("There is no ty build for {os} ({arch})."))
    })
}

/// Join `name` onto `root`, refusing absolute paths and `..`.
pub fn safe_join(root: &Path, name: &str) -> Result<PathBuf, AppError> {
    let rel = Path::new(name);
    if name.contains('\0')
        || rel.is_absolute()
        || rel.components().any(|c| matches!(c, Component::ParentDir))
    {
        return Err(format!("The archive contains an unsafe path `{name}`.").into());
    }
    Ok(root.join(rel))
}

pub fn extract_zip(archive: &Path, dest: &Path) -> Result<(), AppError> {
    let file = File::open(archive)?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("Could not read the zip: {e}"))?;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        let name = entry.name().to_string();
        if name.ends_with('/') || entry.is_dir() {
            continue;
        }
        let path = safe_join(dest, &name)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut out = File::create(&path)?;
        io::copy(&mut entry, &mut out)?;
    }
    Ok(())
}

pub fn extract_tar_gz(archive: &Path, dest: &Path) -> Result<(), AppError> {
    let file = File::open(archive)?;
    let gz = flate2::read::GzDecoder::new(file);
    let mut tar = tar::Archive::new(gz);
    let entries = tar
        .entries()
        .map_err(|e| format!("Could not read the archive: {e}"))?;
    for entry in entries {
        let mut entry = entry?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let owned = entry.path()?.to_string_lossy().into_owned();
        let path = safe_join(dest, &owned)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut out = File::create(&path)?;
        io::copy(&mut entry, &mut out)?;
    }
    Ok(())
}

fn find_binary(dir: &Path) -> Option<PathBuf> {
    let direct = exe_in(dir);
    if direct.is_file() {
        return Some(direct);
    }
    let name = if cfg!(windows) { "ty.exe" } else { "ty" };
    let mut stack = vec![dir.to_path_buf()];
    while let Some(next) = stack.pop() {
        let Ok(rd) = fs::read_dir(&next) else {
            continue;
        };
        for entry in rd.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.file_name().and_then(|n| n.to_str()) == Some(name) {
                return Some(path);
            }
        }
    }
    None
}

/// Bytes of Language Server Protocol messages, split across reads.
#[derive(Default)]
pub struct FrameBuf {
    buf: Vec<u8>,
}

impl FrameBuf {
    pub fn push(&mut self, chunk: &[u8]) -> Result<Vec<String>, AppError> {
        self.buf.extend_from_slice(chunk);
        let mut out = Vec::new();
        loop {
            let Some(sep) = find_sep(&self.buf) else {
                if self.buf.len() > 64 * 1024 && !self.buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    return Err("The language server sent a header that never ended.".into());
                }
                break;
            };
            let header = String::from_utf8_lossy(&self.buf[..sep]);
            let len = content_length(&header)?;
            let body_at = sep + 4;
            if self.buf.len() < body_at + len {
                break;
            }
            let body = self.buf[body_at..body_at + len].to_vec();
            self.buf.drain(..body_at + len);
            out.push(
                String::from_utf8(body)
                    .map_err(|_| "ty sent a message that was not UTF-8.".to_string())?,
            );
        }
        Ok(out)
    }
}

fn find_sep(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

fn content_length(header: &str) -> Result<usize, AppError> {
    let line = header
        .lines()
        .find(|l| l.to_ascii_lowercase().starts_with("content-length:"))
        .ok_or("A message arrived with no Content-Length.")?;
    let raw = line.split_once(':').map(|(_, v)| v.trim()).unwrap_or("");
    let len: usize = raw
        .parse()
        .map_err(|_| format!("Content-Length `{raw}` is not a number."))?;
    if len > MAX_FRAME {
        return Err("The language server sent a message that is too large.".into());
    }
    Ok(len)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn python_version_follows_the_renpy_release() {
        assert_eq!(python_for_renpy("8.2.3"), Some("3.9"));
        assert_eq!(python_for_renpy("8.0.0"), Some("3.9"));
        assert_eq!(python_for_renpy("8.3.0"), Some("3.12"));
        assert_eq!(python_for_renpy("8.5.2"), Some("3.12"));
        assert_eq!(python_for_renpy("7.5.3"), None);
        assert_eq!(python_for_renpy("nope"), None);
    }

    #[test]
    fn asset_names_cover_the_hosts_we_ship() {
        assert_eq!(
            asset_name("windows", "x86_64"),
            Some("ty-x86_64-pc-windows-msvc.zip")
        );
        assert_eq!(
            asset_name("windows", "aarch64"),
            Some("ty-aarch64-pc-windows-msvc.zip")
        );
        assert_eq!(
            asset_name("linux", "x86_64"),
            Some("ty-x86_64-unknown-linux-gnu.tar.gz")
        );
        assert_eq!(
            asset_name("macos", "aarch64"),
            Some("ty-aarch64-apple-darwin.tar.gz")
        );
        assert_eq!(asset_name("windows", "riscv64"), None);
    }

    #[test]
    fn sidecar_checksum_is_the_first_hex_token() {
        let line = "e4b3c7cd30ff8b4ee4c2a62621f293341ad3c89fb3e7c6e4d2686150a4dbcf08 *ty.zip\n";
        assert_eq!(sha256_sidecar(line).unwrap().len(), 64);
        assert_eq!(sha256_sidecar("not a hash"), None);
        assert_eq!(sha256_sidecar(""), None);
    }

    #[test]
    fn frames_split_across_reads_and_two_in_one_read() {
        let one = frame(b"{\"a\":1}");
        let two = frame(b"{\"b\":2}");
        let mut buf = FrameBuf::default();
        assert!(buf.push(&one[..10]).unwrap().is_empty());
        let got = buf.push(&one[10..]).unwrap();
        assert_eq!(got, vec!["{\"a\":1}"]);
        let mut both = two.clone();
        both.extend_from_slice(&frame(b"{\"c\":3}"));
        let got = buf.push(&both).unwrap();
        assert_eq!(got, vec!["{\"b\":2}", "{\"c\":3}"]);
    }

    #[test]
    fn a_bad_length_is_rejected() {
        let mut buf = FrameBuf::default();
        let err = buf.push(b"Content-Length: nope\r\n\r\n").unwrap_err();
        assert!(err.contains("not a number"));
        let mut buf = FrameBuf::default();
        let huge = format!("Content-Length: {}\r\n\r\n", MAX_FRAME + 1);
        let err = buf.push(huge.as_bytes()).unwrap_err();
        assert!(err.contains("too large"));
    }

    #[test]
    fn extract_refuses_a_parent_path() {
        let dir = std::env::temp_dir().join(format!("vnide-ty-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let zip_path = dir.join("t.zip");
        {
            let file = File::create(&zip_path).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            let opts = zip::write::SimpleFileOptions::default();
            zip.start_file("ty.exe", opts).unwrap();
            zip.write_all(b"bin").unwrap();
            zip.start_file("../evil.txt", opts).unwrap();
            zip.write_all(b"no").unwrap();
            zip.finish().unwrap();
        }
        let dest = dir.join("out");
        fs::create_dir_all(&dest).unwrap();
        let err = extract_zip(&zip_path, &dest).unwrap_err();
        assert!(err.contains("unsafe"));
        assert!(!dir.join("evil.txt").exists());
        assert!(safe_join(Path::new("root"), "a/b").is_ok());
        assert!(safe_join(Path::new("root"), "a/../../b").is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    fn frame(body: &[u8]) -> Vec<u8> {
        let mut out = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
        out.extend_from_slice(body);
        out
    }
}
