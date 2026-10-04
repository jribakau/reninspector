//! Installed Ren'Py SDKs: where they live, how they are downloaded, and how a
//! launcher command is run with live output.

use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::commands::AppState;
use crate::process::background;

const RELEASE_LIST: &str = "https://www.renpy.org/release_list.html";
const USER_AGENT: &str = "Ren-Inspector/0.1";

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SdkInfo {
    pub path: String,
    pub version: Option<String>,
    pub exe: String,
    pub has_web: bool,
    pub source: String,
    pub unverified: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SdkList {
    pub folder: String,
    pub items: Vec<SdkInfo>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SdkProgress {
    pub version: String,
    pub phase: String,
    pub done: u64,
    pub total: u64,
    pub label: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildLine {
    pub stream: String,
    pub text: String,
}

#[derive(Serialize, Deserialize, Default)]
struct RegistryFile {
    folder: Option<String>,
    #[serde(default)]
    extra: Vec<String>,
    #[serde(default)]
    unverified: Vec<String>,
}

struct Registry {
    folder: PathBuf,
    extra: Vec<PathBuf>,
    unverified: Vec<String>,
}

fn data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path().app_data_dir().map_err(|e| e.to_string())
}

fn registry_path(data: &Path) -> PathBuf {
    data.join("sdks.json")
}

fn load_registry(data: &Path) -> Result<Registry, String> {
    let default_folder = data.join("sdks");
    let path = registry_path(data);
    if !path.is_file() {
        return Ok(empty_registry(default_folder));
    }
    let text =
        fs::read_to_string(&path).map_err(|e| format!("Could not read {}: {e}", path.display()))?;
    parse_registry(&text, default_folder).map_err(|_| {
        format!(
            "{} is damaged, so the SDK list could not be read. The file was left unchanged.",
            path.display()
        )
    })
}

fn empty_registry(folder: PathBuf) -> Registry {
    Registry {
        folder,
        extra: Vec::new(),
        unverified: Vec::new(),
    }
}

fn parse_registry(text: &str, default_folder: PathBuf) -> Result<Registry, ()> {
    let parsed: RegistryFile = serde_json::from_str(text).map_err(|_| ())?;
    Ok(Registry {
        folder: parsed
            .folder
            .filter(|s| !s.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or(default_folder),
        extra: parsed.extra.into_iter().map(PathBuf::from).collect(),
        unverified: parsed.unverified,
    })
}

fn save_registry(data: &Path, reg: &Registry) -> Result<(), String> {
    if let Some(parent) = registry_path(data).parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let file = RegistryFile {
        folder: Some(reg.folder.to_string_lossy().into_owned()),
        extra: reg
            .extra
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect(),
        unverified: reg.unverified.clone(),
    };
    let text = serde_json::to_string_pretty(&file).map_err(|e| e.to_string())?;
    fs::write(registry_path(data), text).map_err(|e| e.to_string())
}

/// `renpy.exe` on Windows, `renpy.sh` everywhere else.
pub(crate) fn sdk_executable(root: &Path) -> PathBuf {
    if cfg!(windows) {
        root.join("renpy.exe")
    } else {
        let sh = root.join("renpy.sh");
        if sh.is_file() {
            sh
        } else {
            root.join("renpy.app")
                .join("Contents")
                .join("MacOS")
                .join("renpy")
        }
    }
}

fn has_sdk_layout(dir: &Path) -> bool {
    dir.join("launcher").is_dir() && dir.join("gui").join("game").is_dir()
}

pub(crate) fn is_sdk_dir(dir: &Path) -> bool {
    has_sdk_layout(dir) && sdk_executable(dir).is_file()
}

/// SDK folder holding `launcher/` and `gui/`. `renpy.app` keeps its executable a few folders down.
pub(crate) fn sdk_root(exe: &Path) -> Option<PathBuf> {
    exe.ancestors()
        .skip(1)
        .take(4)
        .find(|dir| has_sdk_layout(dir))
        .map(Path::to_path_buf)
}

fn same_path(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

fn inside(parent: &Path, child: &Path) -> bool {
    let parent = parent
        .canonicalize()
        .unwrap_or_else(|_| parent.to_path_buf());
    let child = child.canonicalize().unwrap_or_else(|_| child.to_path_buf());
    child.starts_with(&parent)
}

fn describe(path: &Path, source: &str, unverified: &[String]) -> Option<SdkInfo> {
    let root = if is_sdk_dir(path) {
        path.to_path_buf()
    } else {
        sdk_root(path)?
    };
    let exe = sdk_executable(&root);
    if !exe.is_file() {
        return None;
    }
    let path_text = root.to_string_lossy().into_owned();
    Some(SdkInfo {
        version: renpy_core::project::read_engine_version(&root),
        exe: exe.to_string_lossy().into_owned(),
        has_web: root.join("web").is_dir(),
        unverified: unverified.iter().any(|p| same_path(Path::new(p), &root)),
        source: source.to_string(),
        path: path_text,
    })
}

fn scan(reg: &Registry) -> Vec<SdkInfo> {
    let mut items = Vec::new();
    if let Ok(entries) = fs::read_dir(&reg.folder) {
        let mut dirs: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.is_dir()
                    && !p
                        .file_name()
                        .is_some_and(|n| n.to_string_lossy().starts_with('.'))
            })
            .collect();
        dirs.sort();
        for dir in dirs {
            if let Some(info) = describe(&dir, "downloaded", &reg.unverified) {
                items.push(info);
            }
        }
    }
    for extra in &reg.extra {
        if inside(&reg.folder, extra) {
            continue;
        }
        if items.iter().any(|i| same_path(Path::new(&i.path), extra)) {
            continue;
        }
        if let Some(info) = describe(extra, "added", &reg.unverified) {
            items.push(info);
        }
    }
    items
}

fn resolve_sdk(path: &str) -> Result<(PathBuf, PathBuf), String> {
    let given = PathBuf::from(path.trim());
    let root = if is_sdk_dir(&given) {
        given
    } else {
        sdk_root(&given).ok_or_else(|| {
            format!(
                "{} is not a Ren'Py SDK (it needs launcher/, gui/ and renpy.exe or renpy.sh).",
                given.display()
            )
        })?
    };
    let exe = sdk_executable(&root);
    if !exe.is_file() {
        return Err(format!("No launcher executable in {}.", root.display()));
    }
    Ok((root, exe))
}

#[tauri::command]
pub fn sdk_list(app: AppHandle) -> Result<SdkList, String> {
    let data = data_dir(&app)?;
    let reg = load_registry(&data)?;
    let folder = reg.folder.to_string_lossy().into_owned();
    Ok(SdkList {
        items: scan(&reg),
        folder,
    })
}

#[tauri::command]
pub fn sdk_set_folder(app: AppHandle, folder: String) -> Result<SdkList, String> {
    let folder = PathBuf::from(folder.trim());
    if folder.as_os_str().is_empty() {
        return Err("Choose a folder for the SDKs.".into());
    }
    fs::create_dir_all(&folder).map_err(|e| format!("Could not use that folder: {e}"))?;
    let data = data_dir(&app)?;
    let mut reg = load_registry(&data)?;
    reg.folder = folder;
    save_registry(&data, &reg)?;
    let folder = reg.folder.to_string_lossy().into_owned();
    Ok(SdkList {
        items: scan(&reg),
        folder,
    })
}

#[tauri::command]
pub fn sdk_add(app: AppHandle, path: String) -> Result<SdkInfo, String> {
    let (root, _) = resolve_sdk(&path)?;
    let data = data_dir(&app)?;
    let mut reg = load_registry(&data)?;
    if !inside(&reg.folder, &root) && !reg.extra.iter().any(|p| same_path(p, &root)) {
        reg.extra.push(root.clone());
        save_registry(&data, &reg)?;
    }
    describe(
        &root,
        if inside(&reg.folder, &root) {
            "downloaded"
        } else {
            "added"
        },
        &reg.unverified,
    )
    .ok_or_else(|| format!("{} is not a Ren'Py SDK.", root.display()))
}

#[tauri::command]
pub fn sdk_remove(app: AppHandle, path: String, delete_files: bool) -> Result<SdkList, String> {
    let (root, _) = resolve_sdk(&path)?;
    let data = data_dir(&app)?;
    let mut reg = load_registry(&data)?;
    let in_folder = inside(&reg.folder, &root);
    if delete_files {
        if !in_folder {
            return Err(
                "This SDK was added from outside the SDK folder, so its files stay where they are."
                    .into(),
            );
        }
        fs::remove_dir_all(&root)
            .map_err(|e| format!("Could not delete {}: {e}", root.display()))?;
    } else if in_folder {
        return Err("That SDK lives in the SDK folder. Delete its files to remove it.".into());
    }
    reg.extra.retain(|p| !same_path(p, &root));
    reg.unverified.retain(|p| !same_path(Path::new(p), &root));
    save_registry(&data, &reg)?;
    let folder = reg.folder.to_string_lossy().into_owned();
    Ok(SdkList {
        items: scan(&reg),
        folder,
    })
}

static CATALOG: Mutex<Option<Vec<String>>> = Mutex::new(None);

#[tauri::command(async)]
pub fn sdk_catalog() -> Result<Vec<String>, String> {
    if let Some(cached) = CATALOG.lock().map_err(|e| e.to_string())?.clone() {
        return Ok(cached);
    }
    let html = http_text(RELEASE_LIST)?;
    let versions = parse_catalog(&html);
    if versions.is_empty() {
        return Err("The Ren'Py release list did not contain any versions.".into());
    }
    *CATALOG.lock().map_err(|e| e.to_string())? = Some(versions.clone());
    Ok(versions)
}

#[tauri::command]
pub fn sdk_cancel(state: State<'_, AppState>) {
    state.sdk_cancel.store(true, Ordering::Relaxed);
}

#[tauri::command]
pub async fn sdk_install(
    app: AppHandle,
    state: State<'_, AppState>,
    version: String,
) -> Result<SdkInfo, String> {
    let version = version.trim().to_string();
    if !valid_version(&version) {
        return Err(format!("{version} is not a Ren'Py version."));
    }
    if state.sdk_busy.swap(true, Ordering::AcqRel) {
        return Err("An SDK download is already running.".into());
    }
    let cancel = state.sdk_cancel.clone();
    cancel.store(false, Ordering::Relaxed);
    let result = tauri::async_runtime::spawn_blocking(move || install_sdk(&app, &cancel, &version))
        .await
        .map_err(|e| e.to_string());
    state.sdk_busy.store(false, Ordering::Release);
    result?
}

#[tauri::command]
pub async fn sdk_install_web(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<SdkInfo, String> {
    let (root, _) = resolve_sdk(&path)?;
    let version = renpy_core::project::read_engine_version(&root)
        .ok_or_else(|| format!("Could not read the Ren'Py version in {}.", root.display()))?;
    if state.sdk_busy.swap(true, Ordering::AcqRel) {
        return Err("An SDK download is already running.".into());
    }
    let cancel = state.sdk_cancel.clone();
    cancel.store(false, Ordering::Relaxed);
    let result =
        tauri::async_runtime::spawn_blocking(move || install_web(&app, &cancel, &root, &version))
            .await
            .map_err(|e| e.to_string());
    state.sdk_busy.store(false, Ordering::Release);
    result?
}

fn install_sdk(app: &AppHandle, cancel: &AtomicBool, version: &str) -> Result<SdkInfo, String> {
    let data = data_dir(app)?;
    let reg = load_registry(&data)?;
    fs::create_dir_all(&reg.folder).map_err(|e| e.to_string())?;
    let dest = reg.folder.join(format!("renpy-{version}-sdk"));
    if is_sdk_dir(&dest) {
        return Err(format!("Ren'Py {version} is already installed."));
    }
    let partial = reg.folder.join(".partial");
    fs::create_dir_all(&partial).map_err(|e| e.to_string())?;
    let zip_path = partial.join(format!("renpy-{version}-sdk.zip"));
    let filename = format!("renpy-{version}-sdk.zip");
    let unverified = download_checked(
        app,
        cancel,
        version,
        &sdk_zip_url(version),
        &checksums_url(version),
        &filename,
        &zip_path,
    )?;
    let staging = partial.join(format!("staging-{version}"));
    emit_progress(app, version, "extract", 0, 0, "Extracting");
    let placed = install_zip_at(&zip_path, &staging, &dest, cancel);
    let _ = fs::remove_file(&zip_path);
    placed?;
    let mut unverified_paths = Vec::new();
    if unverified {
        let mut reg = load_registry(&data)?;
        let text = dest.to_string_lossy().into_owned();
        if !reg
            .unverified
            .iter()
            .any(|p| same_path(Path::new(p), &dest))
        {
            reg.unverified.push(text.clone());
            save_registry(&data, &reg)?;
        }
        unverified_paths.push(text);
    }
    describe(&dest, "downloaded", &unverified_paths)
        .ok_or_else(|| format!("The archive for Ren'Py {version} did not contain an SDK."))
}

fn install_web(
    app: &AppHandle,
    cancel: &AtomicBool,
    root: &Path,
    version: &str,
) -> Result<SdkInfo, String> {
    let data = data_dir(app)?;
    let reg = load_registry(&data)?;
    let partial = reg.folder.join(".partial");
    fs::create_dir_all(&partial).map_err(|e| e.to_string())?;
    let zip_path = partial.join(format!("renpy-{version}-web.zip"));
    let filename = format!("renpy-{version}-web.zip");
    let unverified = download_checked(
        app,
        cancel,
        version,
        &web_zip_url(version),
        &checksums_url(version),
        &filename,
        &zip_path,
    )?;
    emit_progress(app, version, "extract", 0, 0, "Extracting Web support");
    let staging = partial.join(format!("web-{version}"));
    let extracted: Result<(), String> = (|| {
        if staging.exists() {
            fs::remove_dir_all(&staging).map_err(|e| e.to_string())?;
        }
        fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
        extract_zip(&zip_path, &staging, cancel)?;
        merge_tree(&strip_renpy_root(&staging), root)?;
        Ok(())
    })();
    let _ = fs::remove_dir_all(&staging);
    let _ = fs::remove_file(&zip_path);
    extracted?;
    if !root.join("web").is_dir() {
        return Err("The Web package did not contain a web folder.".into());
    }
    let mut reg = reg;
    if unverified && !reg.unverified.iter().any(|p| same_path(Path::new(p), root)) {
        reg.unverified.push(root.to_string_lossy().into_owned());
        save_registry(&data, &reg)?;
    }
    let source = if inside(&reg.folder, root) {
        "downloaded"
    } else {
        "added"
    };
    describe(root, source, &reg.unverified)
        .ok_or_else(|| format!("{} is not a Ren'Py SDK.", root.display()))
}

fn download_checked(
    app: &AppHandle,
    cancel: &AtomicBool,
    version: &str,
    url: &str,
    sums_url: &str,
    filename: &str,
    dest: &Path,
) -> Result<bool, String> {
    if cancel.load(Ordering::Relaxed) {
        return Err("Download cancelled.".into());
    }
    emit_progress(
        app,
        version,
        "download",
        0,
        0,
        &format!("Downloading {filename}"),
    );
    download_file(app, cancel, version, url, dest)?;
    emit_progress(app, version, "check", 0, 0, "Checking the download");
    // Old releases publish no checksums (404) and install as unverified. Any other
    // failure means the check could not run, so the download is not trusted.
    let sums = match http().get(sums_url).set("User-Agent", USER_AGENT).call() {
        Ok(resp) => resp.into_string().map_err(|e| {
            let _ = fs::remove_file(dest);
            format!("Could not read the published checksums for {filename}: {e}")
        })?,
        Err(ureq::Error::Status(404, _)) => String::new(),
        Err(e) => {
            let _ = fs::remove_file(dest);
            return Err(format!(
                "Could not check {filename} against its published checksum ({e}). Try again later."
            ));
        }
    };
    match sha256_in_checksums(&sums, filename) {
        Some(expected) => {
            let got = sha256_file(dest)?;
            if !got.eq_ignore_ascii_case(&expected) {
                let _ = fs::remove_file(dest);
                return Err(format!(
                    "The download of {filename} did not match its published checksum."
                ));
            }
            Ok(false)
        }
        None => Ok(true),
    }
}

fn download_file(
    app: &AppHandle,
    cancel: &AtomicBool,
    version: &str,
    url: &str,
    dest: &Path,
) -> Result<(), String> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
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
    let mut file = File::create(dest).map_err(|e| e.to_string())?;
    let mut buf = [0u8; 64 * 1024];
    let mut done = 0u64;
    let mut next_emit = 0u64;
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
        file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        done += n as u64;
        if done >= next_emit {
            emit_progress(
                app,
                version,
                "download",
                done,
                total,
                &format!("Downloading Ren'Py {version}"),
            );
            next_emit = done.saturating_add(512 * 1024);
        }
    }
    file.flush().map_err(|e| e.to_string())?;
    emit_progress(
        app,
        version,
        "download",
        done,
        total.max(done),
        "Download finished",
    );
    Ok(())
}

fn emit_progress(app: &AppHandle, version: &str, phase: &str, done: u64, total: u64, label: &str) {
    let _ = app.emit(
        "sdk:progress",
        SdkProgress {
            version: version.to_string(),
            phase: phase.to_string(),
            done,
            total,
            label: label.to_string(),
        },
    );
}

fn http() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(20))
        .timeout_read(Duration::from_secs(120))
        .build()
}

fn http_text(url: &str) -> Result<String, String> {
    http()
        .get(url)
        .set("User-Agent", USER_AGENT)
        .call()
        .map_err(|e| format!("Could not reach {url}: {e}"))?
        .into_string()
        .map_err(|e| e.to_string())
}

fn sdk_zip_url(version: &str) -> String {
    format!("https://www.renpy.org/dl/{version}/renpy-{version}-sdk.zip")
}

fn web_zip_url(version: &str) -> String {
    format!("https://www.renpy.org/dl/{version}/renpy-{version}-web.zip")
}

fn checksums_url(version: &str) -> String {
    format!("https://www.renpy.org/dl/{version}/checksums.txt")
}

/// Extracts `zip_path` into `staging`, then renames the SDK folder to `dest`.
/// A failure deletes `staging` and leaves `dest` absent.
fn install_zip_at(
    zip_path: &Path,
    staging: &Path,
    dest: &Path,
    cancel: &AtomicBool,
) -> Result<(), String> {
    if dest.exists() {
        return Err(format!("{} already exists.", dest.display()));
    }
    if staging.exists() {
        fs::remove_dir_all(staging).map_err(|e| e.to_string())?;
    }
    fs::create_dir_all(staging).map_err(|e| e.to_string())?;
    let extracted =
        extract_zip(zip_path, staging, cancel).and_then(|_| place_extracted_sdk(staging, dest));
    if staging.exists() {
        let _ = fs::remove_dir_all(staging);
    }
    extracted
}

fn extract_zip(zip_path: &Path, dest: &Path, cancel: &AtomicBool) -> Result<(), String> {
    let file =
        File::open(zip_path).map_err(|e| format!("Could not open {}: {e}", zip_path.display()))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| format!("Could not read the archive: {e}"))?;
    for i in 0..archive.len() {
        if cancel.load(Ordering::Relaxed) {
            return Err("Download cancelled.".into());
        }
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("Could not read the archive: {e}"))?;
        let Some(rel) = entry.enclosed_name().map(|p| p.to_path_buf()) else {
            continue;
        };
        let out = dest.join(&rel);
        if entry.is_dir() {
            fs::create_dir_all(&out).map_err(|e| e.to_string())?;
            continue;
        }
        let mode = entry.unix_mode();
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut written = File::create(&out).map_err(|e| e.to_string())?;
        std::io::copy(&mut entry, &mut written).map_err(|e| e.to_string())?;
        drop(written);
        apply_unix_mode(&out, mode);
    }
    Ok(())
}

/// Ren'Py SDK zips are packed on Unix, so `renpy.sh` and the engine binaries
/// carry their executable bit in the zip entry. Windows has no equivalent.
fn apply_unix_mode(path: &Path, mode: Option<u32>) {
    #[cfg(unix)]
    if let Some(mode) = mode {
        use std::os::unix::fs::PermissionsExt;
        let perms = fs::Permissions::from_mode(mode & 0o777);
        let _ = fs::set_permissions(path, perms);
    }
    #[cfg(not(unix))]
    {
        let _ = (path, mode);
    }
}

fn place_extracted_sdk(staging: &Path, dest: &Path) -> Result<(), String> {
    let found = find_sdk(staging)
        .ok_or_else(|| "The archive does not contain a Ren'Py SDK.".to_string())?;
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    if same_path(&found, staging) {
        fs::rename(staging, dest).map_err(|e| format!("Could not finish the install: {e}"))?;
    } else {
        fs::rename(&found, dest).map_err(|e| format!("Could not finish the install: {e}"))?;
    }
    Ok(())
}

fn find_sdk(dir: &Path) -> Option<PathBuf> {
    if is_sdk_dir(dir) {
        return Some(dir.to_path_buf());
    }
    let mut stack = vec![dir.to_path_buf()];
    for _depth in 0..3 {
        let level = std::mem::take(&mut stack);
        for dir in level {
            let Ok(entries) = fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                if is_sdk_dir(&path) {
                    return Some(path);
                }
                stack.push(path);
            }
        }
    }
    None
}

/// The Web zip sometimes wraps its files in one `renpy-<version>/` folder.
fn strip_renpy_root(staging: &Path) -> PathBuf {
    let Ok(entries) = fs::read_dir(staging) else {
        return staging.to_path_buf();
    };
    let dirs: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    if dirs.len() == 1 {
        let name = dirs[0]
            .file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if name.starts_with("renpy-") {
            return dirs[0].clone();
        }
    }
    staging.to_path_buf()
}

fn merge_tree(from: &Path, into: &Path) -> Result<(), String> {
    fs::create_dir_all(into).map_err(|e| e.to_string())?;
    for entry in fs::read_dir(from).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let target = into.join(entry.file_name());
        if entry.path().is_dir() {
            merge_tree(&entry.path(), &target)?;
        } else {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            fs::copy(entry.path(), &target)
                .map_err(|e| format!("Could not write {}: {e}", target.display()))?;
        }
    }
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn sha256_in_checksums(text: &str, filename: &str) -> Option<String> {
    let mut in_sha = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            in_sha = line.eq_ignore_ascii_case("# sha256");
            continue;
        }
        if !in_sha || line.is_empty() {
            continue;
        }
        let mut parts = line.split_whitespace();
        let Some(hash) = parts.next() else { continue };
        let Some(name) = parts.next() else { continue };
        if name == filename && hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit()) {
            return Some(hash.to_ascii_lowercase());
        }
    }
    None
}

fn parse_catalog(html: &str) -> Vec<String> {
    let mut found = Vec::new();
    let lower = html.replace("&#39;", "'").replace("&apos;", "'");
    let bytes = lower.as_str();
    let marker = "Ren'Py ";
    let mut rest = bytes;
    while let Some(at) = rest.find(marker) {
        rest = &rest[at + marker.len()..];
        let end = rest
            .find(|c: char| !(c.is_ascii_digit() || c == '.'))
            .unwrap_or(rest.len());
        let version = &rest[..end];
        if valid_version(version)
            && supported_release(version)
            && !found.iter().any(|v: &String| v == version)
        {
            found.push(version.to_string());
        }
        rest = &rest[end..];
    }
    found.sort_by(|a, b| cmp_version(b, a));
    found
}

fn valid_version(version: &str) -> bool {
    let parts: Vec<&str> = version.split('.').collect();
    (2..=4).contains(&parts.len())
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
}

fn supported_release(version: &str) -> bool {
    match major_minor(version) {
        Some((major, minor)) => major > 7 || (major == 7 && minor >= 4),
        None => false,
    }
}

fn major_minor(version: &str) -> Option<(u32, u32)> {
    let mut parts = version.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    Some((major, minor))
}

fn cmp_version(a: &str, b: &str) -> std::cmp::Ordering {
    let pa = version_key(a);
    let pb = version_key(b);
    let n = pa.len().max(pb.len());
    for i in 0..n {
        let x = pa.get(i).copied().unwrap_or(0);
        let y = pb.get(i).copied().unwrap_or(0);
        match x.cmp(&y) {
            std::cmp::Ordering::Equal => {}
            other => return other,
        }
    }
    std::cmp::Ordering::Equal
}

fn version_key(version: &str) -> Vec<u32> {
    version.split('.').filter_map(|p| p.parse().ok()).collect()
}

const PC_PACKAGES: &[&str] = &["pc", "mac", "linux"];

fn normalize_packages(packages: &[String]) -> Result<Vec<String>, String> {
    if packages.is_empty() {
        return Err("Choose a package to build.".into());
    }
    let mut out = Vec::new();
    for name in packages {
        let name = name.trim();
        if !PC_PACKAGES.contains(&name) {
            return Err(format!("Unknown package `{name}`."));
        }
        if !out.iter().any(|e: &String| e == name) {
            out.push(name.to_string());
        }
    }
    Ok(out)
}

#[tauri::command]
pub async fn build_pc(
    app: AppHandle,
    state: State<'_, AppState>,
    sdk: String,
    dest: Option<String>,
    packages: Vec<String>,
) -> Result<String, String> {
    let (sdk_root, exe) = resolve_sdk(&sdk)?;
    let packages = normalize_packages(&packages)?;
    let (root, aside) = build_project(&state)?;
    let slot = state.build.clone();
    let cancel = state.build_cancel.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _aside = aside;
        let mut args = vec![root.to_string_lossy().into_owned()];
        if let Some(dest) = dest.filter(|s| !s.trim().is_empty()) {
            args.push("--destination".into());
            args.push(dest);
        }
        for package in packages {
            args.push("--package".into());
            args.push(package);
        }
        run_launcher(
            Some(&app),
            slot,
            cancel,
            &exe,
            &sdk_root,
            "distribute",
            &args,
            None,
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn build_web(
    app: AppHandle,
    state: State<'_, AppState>,
    sdk: String,
    dest: Option<String>,
    launch: bool,
) -> Result<String, String> {
    let (sdk_root, exe) = resolve_sdk(&sdk)?;
    if !sdk_root.join("web").is_dir() {
        return Err("This SDK has no Web support yet. Install it and build again.".into());
    }
    let (root, aside) = build_project(&state)?;
    let slot = state.build.clone();
    let cancel = state.build_cancel.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _aside = aside;
        let mut args = vec![root.to_string_lossy().into_owned()];
        if let Some(dest) = dest.filter(|s| !s.trim().is_empty()) {
            args.push("--destination".into());
            args.push(dest);
        }
        if launch {
            args.push("--launch".into());
        }
        run_launcher(
            Some(&app),
            slot,
            cancel,
            &exe,
            &sdk_root,
            "web_build",
            &args,
            None,
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn build_cancel(state: State<'_, AppState>) {
    state.build_cancel.store(true, Ordering::Relaxed);
    if let Ok(mut guard) = state.build.lock() {
        if let Some(child) = guard.as_mut() {
            let _ = child.kill();
        }
    }
}

#[tauri::command]
pub fn reveal_path(path: String) -> Result<(), String> {
    let path = PathBuf::from(path.trim());
    if !path.exists() {
        return Err(format!("{} does not exist yet.", path.display()));
    }
    // explorer, open and xdg-open run a file instead of showing it.
    if !path.is_dir() {
        return Err(format!("{} is not a folder.", path.display()));
    }
    let mut cmd = if cfg!(windows) {
        let mut cmd = std::process::Command::new("explorer");
        cmd.arg(&path);
        cmd
    } else if cfg!(target_os = "macos") {
        let mut cmd = std::process::Command::new("open");
        cmd.arg(&path);
        cmd
    } else {
        let mut cmd = std::process::Command::new("xdg-open");
        cmd.arg(&path);
        cmd
    };
    cmd.spawn()
        .map_err(|e| format!("Could not open {}: {e}", path.display()))?;
    Ok(())
}

/// The project root for a build, with nothing the IDE wrote into `game/` left
/// for the build to package. The game has to be closed: live preview keeps its
/// scripts there for the whole session.
fn build_project(
    state: &State<'_, AppState>,
) -> Result<(PathBuf, crate::ide::AutoreloadAside), String> {
    let guard = state.project.lock().map_err(|e| e.to_string())?;
    let project = guard
        .as_ref()
        .ok_or_else(|| "No project is open.".to_string())?;
    crate::commands::ensure_game_closed(project)
        .map_err(|_| "Close the game before building.".to_string())?;
    crate::launch::remove_one_launch_shims(&project.game_dir);
    let aside = crate::ide::AutoreloadAside::take(&project.game_dir)?;
    Ok((project.root.clone(), aside))
}

/// Runs `<exe> <sdk>/launcher <command> ...`. Stdout and stderr are emitted as
/// `build:line`. The child is stored so `build_cancel` can kill it.
pub(crate) fn run_launcher(
    app: Option<&AppHandle>,
    slot: Arc<Mutex<Option<Child>>>,
    cancel: Arc<AtomicBool>,
    exe: &Path,
    sdk: &Path,
    command: &str,
    args: &[String],
    timeout: Option<Duration>,
) -> Result<String, String> {
    run_renpy(
        app,
        slot,
        cancel,
        exe,
        sdk,
        &sdk.join("launcher"),
        command,
        args,
        &[],
        timeout,
    )
}

/// Runs `<exe> <base> <command> ...` from the SDK folder, where `base` is the
/// launcher or a game. `env` is added to the child's environment.
#[allow(clippy::too_many_arguments)]
pub(crate) fn run_renpy(
    app: Option<&AppHandle>,
    slot: Arc<Mutex<Option<Child>>>,
    cancel: Arc<AtomicBool>,
    exe: &Path,
    sdk: &Path,
    base: &Path,
    command: &str,
    args: &[String],
    env: &[(&str, &str)],
    timeout: Option<Duration>,
) -> Result<String, String> {
    if !exe.is_file() {
        return Err(format!("Launcher {} does not exist.", exe.display()));
    }
    {
        let guard = slot.lock().map_err(|e| e.to_string())?;
        if guard.is_some() {
            return Err("A build is already running.".into());
        }
    }
    cancel.store(false, Ordering::Relaxed);
    let mut child = background(exe)
        .arg(base)
        .arg(command)
        .args(args)
        .envs(env.iter().copied())
        .current_dir(sdk)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Could not start {}: {e}", exe.display()))?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    {
        let mut guard = slot.lock().map_err(|e| e.to_string())?;
        if guard.is_some() {
            let _ = child.kill();
            let _ = child.wait();
            return Err("A build is already running.".into());
        }
        *guard = Some(child);
    }
    let tail = Arc::new(Mutex::new(String::new()));
    let app_for_out = app.cloned();
    let app_for_err = app.cloned();
    let tail_out = tail.clone();
    let tail_err = tail.clone();
    let out_thread = std::thread::spawn(move || {
        if let Some(pipe) = stdout {
            pump(app_for_out, pipe, "out", &tail_out);
        }
    });
    let err_thread = std::thread::spawn(move || {
        if let Some(pipe) = stderr {
            pump(app_for_err, pipe, "err", &tail_err);
        }
    });
    let started = Instant::now();
    let status = loop {
        if let Some(limit) = timeout {
            if started.elapsed() > limit {
                kill_build(&slot);
                let _ = out_thread.join();
                let _ = err_thread.join();
                reap(&slot);
                return Err("The SDK did not finish within two minutes.".into());
            }
        }
        let waited = {
            let mut guard = slot.lock().map_err(|e| e.to_string())?;
            match guard.as_mut() {
                Some(child) => child.try_wait().map_err(|e| e.to_string())?,
                None => None,
            }
        };
        if let Some(status) = waited {
            break status;
        }
        std::thread::sleep(Duration::from_millis(80));
    };
    let _ = out_thread.join();
    let _ = err_thread.join();
    reap(&slot);
    let text = tail.lock().map(|t| t.clone()).unwrap_or_default();
    if cancel.load(Ordering::Relaxed) {
        return Ok(if text.trim().is_empty() {
            "Stopped.".into()
        } else {
            format!("Stopped.\n{text}")
        });
    }
    finish_status(status, &text)
}

fn pump(app: Option<AppHandle>, pipe: impl Read, stream: &str, tail: &Mutex<String>) {
    let reader = BufReader::new(pipe);
    for line in reader.lines() {
        let Ok(text) = line else { break };
        push_tail(tail, &text);
        if let Some(app) = &app {
            let _ = app.emit(
                "build:line",
                BuildLine {
                    stream: stream.to_string(),
                    text,
                },
            );
        }
    }
}

fn push_tail(tail: &Mutex<String>, line: &str) {
    let Ok(mut text) = tail.lock() else { return };
    text.push_str(line);
    text.push('\n');
    if text.len() > 8_000 {
        let mut end = text.len() - 4_000;
        while end < text.len() && !text.is_char_boundary(end) {
            end += 1;
        }
        text.drain(..end);
    }
}

fn kill_build(slot: &Mutex<Option<Child>>) {
    if let Ok(mut guard) = slot.lock() {
        if let Some(child) = guard.as_mut() {
            let _ = child.kill();
        }
    }
}

fn reap(slot: &Mutex<Option<Child>>) {
    if let Ok(mut guard) = slot.lock() {
        if let Some(mut child) = guard.take() {
            let _ = child.wait();
        }
    }
}

fn finish_status(status: ExitStatus, text: &str) -> Result<String, String> {
    if status.success() {
        Ok(if text.trim().is_empty() {
            "Finished.".into()
        } else {
            text.to_string()
        })
    } else {
        Err(format!("Build failed.\n{text}"))
    }
}

/// The launcher command behind Ren'Py's "Create New Project", including the GUI.
pub(crate) fn generate_from_sdk(
    app: Option<&AppHandle>,
    slot: Option<Arc<Mutex<Option<Child>>>>,
    cancel: Option<Arc<AtomicBool>>,
    exe: &Path,
    root: &Path,
) -> Result<(), String> {
    let sdk = sdk_root(exe).filter(|_| exe.is_file()).ok_or_else(|| {
        format!(
            "{} is not inside a Ren'Py SDK (no launcher and gui folders next to it).",
            exe.display()
        )
    })?;
    let slot = slot.unwrap_or_else(|| Arc::new(Mutex::new(None)));
    let cancel = cancel.unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
    let args = vec![
        root.to_string_lossy().into_owned(),
        "--start".into(),
        "--template".into(),
        sdk.join("gui").to_string_lossy().into_owned(),
    ];
    run_launcher(
        app,
        slot.clone(),
        cancel.clone(),
        exe,
        &sdk,
        "generate_gui",
        &args,
        Some(Duration::from_secs(120)),
    )?;
    let game = root.join("game");
    if !game.join("gui.rpy").is_file() || !game.join("screens.rpy").is_file() {
        return Err("The SDK could not create the project.".into());
    }
    // `generate_gui` only writes the textbox, menu overlays and choice buttons.
    // The plain button, bar, slider and scrollbar images come from the game's own
    // `gui_images` command, once for desktop and once for phones.
    for variant in [Some("small phone"), None] {
        let env: Vec<(&str, &str)> = variant.map(|v| ("RENPY_VARIANT", v)).into_iter().collect();
        run_renpy(
            app,
            slot.clone(),
            cancel.clone(),
            exe,
            &sdk,
            root,
            "gui_images",
            &[],
            &env,
            Some(Duration::from_secs(120)),
        )?;
    }
    if !game
        .join("gui")
        .join("button")
        .join("idle_background.png")
        .is_file()
    {
        return Err("The SDK could not create the button images.".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const CATALOG_HTML: &str = r#"
        <table>
        <tr><td>185</td><td><a href="/release/8.5.3">Ren'Py 8.5.3</a></td></tr>
        <tr><td>140</td><td>Ren'Py 7.5.0</td></tr>
        <tr><td>128</td><td>Ren&#39;Py 7.4.0</td></tr>
        <tr><td>127</td><td>Ren'Py 7.3.5</td></tr>
        <tr><td>100</td><td>Ren'Py 6.99.14.3</td></tr>
        <tr><td>1</td><td>Ren'Py 8.5.3</td></tr>
        </table>
    "#;

    const CHECKSUMS: &str = r#"
        # md5
        aaaa file.zip
        # sha256
        ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad renpy-8.5.2-sdk.zip
        ffff not-the-one.zip
        # sha1
        bbbb renpy-8.5.2-sdk.zip
    "#;

    #[test]
    fn catalog_keeps_final_releases_from_7_4() {
        let versions = parse_catalog(CATALOG_HTML);
        assert_eq!(
            versions,
            vec![
                "8.5.3".to_string(),
                "7.5.0".to_string(),
                "7.4.0".to_string()
            ]
        );
    }

    #[test]
    fn checksums_read_the_sha256_line() {
        assert_eq!(
            sha256_in_checksums(CHECKSUMS, "renpy-8.5.2-sdk.zip").as_deref(),
            Some("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
        );
        assert_eq!(sha256_in_checksums(CHECKSUMS, "missing.zip"), None);
        assert_eq!(
            sha256_in_checksums("# md5\nabc file.zip\n", "file.zip"),
            None
        );
    }

    #[test]
    fn sha256_of_abc_matches() {
        let dir = std::env::temp_dir().join(format!("vnide-sha-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("abc.txt");
        fs::write(&file, b"abc").unwrap();
        assert_eq!(
            sha256_file(&file).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    fn touch_sdk(dir: &Path, version: &str) {
        fs::create_dir_all(dir.join("launcher")).unwrap();
        fs::create_dir_all(dir.join("gui").join("game")).unwrap();
        fs::create_dir_all(dir.join("renpy")).unwrap();
        fs::write(dir.join("renpy.exe"), b"").unwrap();
        fs::write(dir.join("renpy.sh"), b"").unwrap();
        fs::write(
            dir.join("renpy").join("vc_version.py"),
            format!("version = \"{version}\"\n"),
        )
        .unwrap();
    }

    #[test]
    fn a_sdk_tree_reports_its_version() {
        let dir = std::env::temp_dir().join(format!("vnide-sdk-scan-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let folder = dir.join("sdks");
        let added = dir.join("elsewhere").join("renpy-7.4.11-sdk");
        touch_sdk(&folder.join("renpy-8.5.2-sdk"), "8.5.2");
        fs::create_dir_all(folder.join("web")).unwrap();
        touch_sdk(&added, "7.4.11");
        let reg = Registry {
            folder: folder.clone(),
            extra: vec![added.clone()],
            unverified: vec![added.to_string_lossy().into_owned()],
        };
        let items = scan(&reg);
        assert_eq!(items.len(), 2, "{items:?}");
        assert_eq!(items[0].version.as_deref(), Some("8.5.2"));
        assert_eq!(items[0].source, "downloaded");
        assert!(!items[0].unverified);
        assert_eq!(items[1].version.as_deref(), Some("7.4.11"));
        assert_eq!(items[1].source, "added");
        assert!(items[1].unverified);
        assert!(sdk_root(&folder.join("renpy-8.5.2-sdk").join("renpy.exe")).is_some());
        let _ = fs::remove_dir_all(&dir);
    }

    fn sample_zip(path: &Path, names: &[&str]) {
        let file = File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        for name in names {
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            zip.start_file(*name, opts).unwrap();
            zip.write_all(b"x").unwrap();
        }
        zip.finish().unwrap();
    }

    #[test]
    fn a_finished_archive_is_moved_into_place() {
        let dir = std::env::temp_dir().join(format!("vnide-sdk-zip-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let zip_path = dir.join("sdk.zip");
        sample_zip(
            &zip_path,
            &[
                "renpy-1.2.3-sdk/launcher/keep.txt",
                "renpy-1.2.3-sdk/gui/game/keep.txt",
                "renpy-1.2.3-sdk/renpy.exe",
                "renpy-1.2.3-sdk/renpy.sh",
                "renpy-1.2.3-sdk/renpy/vc_version.py",
            ],
        );
        let dest = dir.join("renpy-1.2.3-sdk");
        let staging = dir.join("staging");
        install_zip_at(&zip_path, &staging, &dest, &AtomicBool::new(false)).unwrap();
        assert!(is_sdk_dir(&dest), "dest should be the sdk");
        assert!(!staging.exists(), "staging should be removed");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_cancelled_extract_does_not_leave_an_sdk() {
        let dir = std::env::temp_dir().join(format!("vnide-sdk-cancel-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let zip_path = dir.join("sdk.zip");
        sample_zip(
            &zip_path,
            &[
                "renpy-1.2.3-sdk/launcher/keep.txt",
                "renpy-1.2.3-sdk/renpy.exe",
            ],
        );
        let dest = dir.join("renpy-1.2.3-sdk");
        let staging = dir.join("staging");
        let err = install_zip_at(&zip_path, &staging, &dest, &AtomicBool::new(true)).unwrap_err();
        assert!(err.contains("cancelled"), "{err}");
        assert!(!dest.exists());
        assert!(!staging.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_damaged_registry_is_reported_and_left_alone() {
        let dir = std::env::temp_dir().join(format!("vnide-sdk-reg-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let missing = load_registry(&dir).unwrap();
        assert!(missing.extra.is_empty());
        fs::write(dir.join("sdks.json"), "{").unwrap();
        let err = match load_registry(&dir) {
            Err(err) => err,
            Ok(_) => panic!("a damaged registry should fail"),
        };
        assert!(err.contains("damaged"), "{err}");
        assert_eq!(fs::read_to_string(dir.join("sdks.json")).unwrap(), "{");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn extract_keeps_the_unix_execute_bit() {
        let dir = std::env::temp_dir().join(format!("vnide-sdk-mode-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let zip_path = dir.join("mode.zip");
        let file = File::create(&zip_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored)
            .unix_permissions(0o755);
        zip.start_file("renpy.sh", opts).unwrap();
        zip.write_all(b"#!/bin/sh\n").unwrap();
        zip.finish().unwrap();

        let dest = dir.join("out");
        fs::create_dir_all(&dest).unwrap();
        extract_zip(&zip_path, &dest, &AtomicBool::new(false)).unwrap();

        let mut archive = zip::ZipArchive::new(File::open(&zip_path).unwrap()).unwrap();
        let entry = archive.by_name("renpy.sh").unwrap();
        let stored = entry
            .unix_mode()
            .expect("zip entry should keep the unix mode");
        assert_eq!(stored & 0o777, 0o755, "{stored:#o}");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(dest.join("renpy.sh"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o755, "{mode:#o}");
        }
        let _ = fs::remove_dir_all(&dir);
    }

    /// Builds a PC package of a generated project.
    /// `VNIDE_SDK` is the SDK folder or its `renpy.exe`.
    #[test]
    #[ignore]
    fn build_pc_on_a_generated_project() {
        let given = std::env::var("VNIDE_SDK").expect("VNIDE_SDK");
        let (sdk, exe) = resolve_sdk(&given).unwrap();
        let parent = std::env::temp_dir().join(format!("vnide-build-pc-{}", std::process::id()));
        let _ = fs::remove_dir_all(&parent);
        let root = parent.join("Demo");
        fs::create_dir_all(root.join("game")).unwrap();
        generate_from_sdk(None, None, None, &exe, &root).unwrap();
        let dest = parent.join("out");
        fs::create_dir_all(&dest).unwrap();
        let slot = Arc::new(Mutex::new(None));
        let cancel = Arc::new(AtomicBool::new(false));
        let args = vec![
            root.to_string_lossy().into_owned(),
            "--destination".into(),
            dest.to_string_lossy().into_owned(),
            "--package".into(),
            "pc".into(),
        ];
        let text = run_launcher(None, slot, cancel, &exe, &sdk, "distribute", &args, None).unwrap();
        assert!(
            fs::read_dir(&dest).unwrap().flatten().any(|e| {
                e.path()
                    .extension()
                    .and_then(|x| x.to_str())
                    .is_some_and(|x| x.eq_ignore_ascii_case("zip"))
            }),
            "no zip in {} ({text})",
            dest.display()
        );
        let _ = fs::remove_dir_all(&parent);
    }

    /// Serves nothing: just packages the web build. Needs the SDK's `web/` folder.
    #[test]
    #[ignore]
    fn web_build_on_a_generated_project() {
        let given = std::env::var("VNIDE_SDK").expect("VNIDE_SDK");
        let (sdk, exe) = resolve_sdk(&given).unwrap();
        if !sdk.join("web").is_dir() {
            eprintln!("skipping: install Web support into {} first", sdk.display());
            return;
        }
        let parent = std::env::temp_dir().join(format!("vnide-build-web-{}", std::process::id()));
        let _ = fs::remove_dir_all(&parent);
        let root = parent.join("Demo");
        fs::create_dir_all(root.join("game")).unwrap();
        generate_from_sdk(None, None, None, &exe, &root).unwrap();
        let dest = parent.join("out");
        fs::create_dir_all(&dest).unwrap();
        let slot = Arc::new(Mutex::new(None));
        let cancel = Arc::new(AtomicBool::new(false));
        let args = vec![
            root.to_string_lossy().into_owned(),
            "--destination".into(),
            dest.to_string_lossy().into_owned(),
        ];
        run_launcher(None, slot, cancel, &exe, &sdk, "web_build", &args, None).unwrap();
        assert!(
            dest.join("index.html").is_file()
                || fs::read_dir(&dest)
                    .unwrap()
                    .flatten()
                    .any(|e| e.path().join("index.html").is_file())
        );
        let _ = fs::remove_dir_all(&parent);
    }
}
