//! Project discovery, file loading and the in-memory `Project`.

use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

use rayon::prelude::*;
use serde::Serialize;

use crate::analysis::{analyze, Analysis, External, Stats};
use crate::ast::{Kind, Stmt};
use crate::diagnostics::Diagnostic;
use crate::engine::{
    EngineDump, EngineRun, EngineSummary, ImageRun, LintItem, StageImagesSummary,
    IMAGE_DUMP_VERSION,
};
use crate::lexer::lex;
use crate::parser::{collapse_ws, parse, scan_meta, Issue, Meta};
use crate::rpa::{self, Archive};

/// Where a script's bytes come from. A loose file always wins over an archive,
/// matching `renpy/loader.py` (`load_from_filesystem` runs first).
#[derive(Clone, Debug)]
pub enum Origin {
    Loose,
    /// No loose file. `archive` is relative to `game/`.
    Archived {
        archive: String,
    },
    /// A loose file that hides the copy in `archive`.
    Override {
        archive: String,
    },
    /// Recovered from a `.rpyc` that had no `.rpy`. `archive` is set when that
    /// compiled file lives in an archive rather than loose in `game/`.
    Compiled {
        archive: Option<String>,
        editable: bool,
    },
}

impl Origin {
    pub fn kind(&self) -> &'static str {
        match self {
            Origin::Loose => "loose",
            Origin::Archived { .. } => "archived",
            Origin::Override { .. } => "override",
            Origin::Compiled { .. } => "compiled",
        }
    }

    pub fn archive(&self) -> Option<&str> {
        match self {
            Origin::Loose => None,
            Origin::Archived { archive } | Origin::Override { archive } => Some(archive),
            Origin::Compiled { archive, .. } => archive.as_deref(),
        }
    }

    pub fn editable(&self) -> bool {
        match self {
            Origin::Compiled { editable, .. } => *editable,
            _ => true,
        }
    }
}

#[derive(Clone)]
pub struct SourceFile {
    /// Path relative to `game/`, forward slashes.
    pub rel: String,
    /// Loose path under `game/`. It may not exist yet for an archived script.
    pub abs: PathBuf,
    pub origin: Origin,
    pub bytes: u64,
    /// Hash of the file bytes. Same-length edits still change it, so an engine
    /// dump cached against `source_key` is not reused after a silent rewrite.
    pub content_hash: u64,
    /// Hash of the top-level statements that run at init. Labels, screens and
    /// dialogue are left out, so editing a line of speech does not change it.
    pub init_hash: u64,
    pub total_lines: u32,
    /// Shared, so a snapshot for off-lock analysis is a pointer copy.
    pub stmts: Arc<Vec<Stmt>>,
    pub issues: Vec<Issue>,
    pub opaque: u32,
    pub meta: Arc<Meta>,
    /// Decompiled text, when this file exists only as a `.rpyc`.
    pub source: Option<Arc<str>>,
    /// Why a decompiled script is read-only. Empty for loose files and full decompiles.
    pub decompile_reasons: Vec<String>,
    /// Compiled `(filename relative to game/, linenumber per decompiled line)`.
    /// Set only for a file recovered from a `.rpyc`.
    pub engine: Option<(String, Vec<u32>)>,
}

#[derive(Debug, Clone, Default)]
pub struct ImageIndex {
    pub names: HashSet<String>,
    pub tags: HashSet<String>,
    /// Lowercased image name to a path relative to `game/`. Loose files win
    /// over an archive entry with the same name.
    pub files: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct Launcher {
    pub exe: PathBuf,
    pub prefix_args: Vec<String>,
}

pub struct Project {
    pub root: PathBuf,
    pub game_dir: PathBuf,
    pub files: Vec<SourceFile>,
    /// `rel` (lowercased on Windows) to index in `files`.
    file_of: HashMap<String, usize>,
    /// `(source_key, init_key)` filled by `reanalyze`.
    key_cache: Option<(String, String)>,
    graph_cache: Mutex<HashMap<(String, bool), crate::flow::LabelGraph>>,
    pub auto_images: ImageIndex,
    /// Version of the engine bundled with the game.
    pub engine_version: Option<String>,
    /// Version the scripts were written for (`script_version.txt`).
    pub script_version: Option<String>,
    pub has_archives: bool,
    /// `.rpyc` files without a `.rpy` next to them (relative to `game/`, or the
    /// path inside an archive).
    pub compiled_only: Vec<String>,
    /// Opened `.rpa` / `.rpi` archives. A damaged one is listed with `error` set.
    pub archives: Vec<LoadedArchive>,
    /// Lower-priority copies of a script that also exists as a loose file or in
    /// an earlier archive.
    pub shadowed: Vec<String>,
    /// Patch entries whose source archive no longer matches the baked checksum.
    pub stale: Vec<String>,
    compiled_fp: u64,
    pub launcher: Option<Launcher>,
    pub analysis: Arc<Analysis>,
    /// Bumped every time analysis is started. A finished run is stored only
    /// when this still matches the epoch it captured.
    analysis_epoch: u64,
    /// Epoch of the analysis stored in `analysis`. Differs from `analysis_epoch`
    /// while a run started by `fork_analysis` has not been published yet.
    installed_epoch: u64,
    pub parse_ms: u128,
    pub engine: Option<EngineRun>,
    /// Images the game's engine registered at init. `None` until a dump has run.
    pub stage_images: Option<ImageRun>,
    pub lint: Option<Vec<LintItem>>,
    /// App-data folder for decompile results, keyed by the `.rpyc` checksum.
    cache_dir: Option<PathBuf>,
    /// Rebuilt when scripts or the engine image list change. Not part of the project data.
    stage_cache: Mutex<Option<Arc<crate::stage::StageIndex>>>,
}

/// Owned inputs for `analyze`, so the work can run without the project mutex.
pub struct AnalysisFork {
    pub epoch: u64,
    files: Vec<SourceFile>,
    auto_images: ImageIndex,
    compiled_only: Vec<String>,
    unreadable: Vec<String>,
    shadowed: Vec<String>,
    stale: Vec<String>,
    engine: Option<EngineDump>,
    lint: Option<Vec<LintItem>>,
}

impl AnalysisFork {
    pub fn run(&self) -> Arc<Analysis> {
        let ext = External {
            engine: self.engine.as_ref(),
            compiled_only: &self.compiled_only,
            archives: &self.unreadable,
            shadowed: &self.shadowed,
            stale: &self.stale,
        };
        let mut analysis = analyze(&self.files, &self.auto_images, &ext);
        if let Some(lint) = &self.lint {
            let known: HashSet<&str> = self.files.iter().map(|f| f.rel.as_str()).collect();
            let items: Vec<Diagnostic> = lint
                .iter()
                .filter(|l| known.contains(l.path.as_str()))
                .map(|l| Diagnostic {
                    severity: "warning",
                    code: "engine-lint",
                    message: format!("[engine lint] {}", l.message),
                    path: l.path.clone(),
                    line: l.line,
                    label: None,
                })
                .collect();
            analysis.diagnostics.extend_sorted(items);
        }
        Arc::new(analysis)
    }
}

/// One label's statements, copied so the graph can be built off the project lock.
pub struct GraphRequest {
    file: SourceFile,
    line: u32,
    detail: bool,
    screens: crate::screens::ScreenTable,
    name: String,
    installed_epoch: u64,
    fresh: bool,
}

impl GraphRequest {
    pub fn build(&self) -> Option<crate::flow::LabelGraph> {
        let mut found = None;
        crate::flow::visit_labels(&self.file.stmts, None, &mut |stmt, cont| {
            if found.is_none() && stmt.line == self.line && stmt.is_label_like() {
                found = Some(crate::flow::build_graph_with(
                    stmt,
                    cont,
                    false,
                    &self.screens,
                    self.detail,
                ));
            }
        });
        if let Some(graph) = &mut found {
            graph.name = self.name.clone();
        }
        found
    }
}

/// Everything needed to run the bundled engine without holding the project lock.
#[derive(Debug, Clone)]
pub struct EngineTarget {
    pub root: PathBuf,
    pub game_dir: PathBuf,
    pub launcher: Launcher,
    pub key: String,
    pub engine_version: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileInfo {
    pub path: String,
    pub lines: u32,
    pub bytes: u64,
    pub opaque: u32,
    pub labels: u32,
    pub issues: u32,
    /// `loose`, `archived`, `override` or `compiled`.
    pub origin: String,
    /// Archive this script came from, when it is not a plain loose file.
    pub archive: Option<String>,
    /// False when a compiled script could not be fully recovered.
    pub editable: bool,
    /// True when the text was decompiled from a `.rpyc`.
    pub decompiled: bool,
    /// Why a decompiled file is read-only. Empty when it is editable.
    pub reasons: Vec<String>,
}

/// One `.rpa` / `.rpi` in the project. `error` is set when the file could not be read;
/// the rest of the project still opens.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveInfo {
    pub path: String,
    pub version: String,
    /// True for ALT, ZiX, RPA-3.2, RPA-4.0 and other non-official headers.
    /// They can be read and extracted. New archives are still written as RPA-3.0.
    pub read_only_format: bool,
    pub entries: u32,
    pub scripts: u32,
    pub compiled_only: u32,
    pub other_bytes: u64,
    pub is_patch: bool,
    /// Inside a subfolder. Ren'Py 7 only loads archives at the top of `game/`.
    pub nested: bool,
    pub overrides: u32,
    pub stale: Vec<String>,
    pub error: Option<String>,
}

/// How the scripts and archives of one game are laid out.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameLayout {
    pub loose_scripts: u32,
    pub override_scripts: u32,
    pub archived_scripts: u32,
    pub compiled_scripts: u32,
    pub compiled_only: u32,
    pub archives: u32,
    pub nested_archives: u32,
    /// Archive header label to how many archives use it. Unreadable files use `unreadable`.
    pub formats: Vec<FormatCount>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FormatCount {
    pub version: String,
    pub count: u32,
}

/// One top-level archive. `sha256` stays empty until [`sha256_file`] fills it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveDigest {
    pub path: String,
    pub bytes: u64,
    /// Modification time as UTC, when the filesystem reports one.
    pub modified: Option<String>,
    pub sha256: Option<String>,
}

/// Identity of the opened game. File dates are a heuristic: Ren'Py stores no build date.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameInfo {
    pub engine_version: Option<String>,
    pub script_version: Option<String>,
    pub name: Option<String>,
    pub version: Option<String>,
    pub build_name: Option<String>,
    pub save_directory: Option<String>,
    /// Oldest modification time among `.rpa` / `.rpi` / `.rpyc` files.
    pub files_oldest: Option<String>,
    /// Newest modification time among those files.
    pub files_newest: Option<String>,
    pub layout: GameLayout,
    /// Top-level archives only. Nested archives are counted in `layout` but not hashed.
    pub archives: Vec<ArchiveDigest>,
    pub notes: Vec<String>,
}

pub struct LoadedArchive {
    pub info: ArchiveInfo,
    /// Shared so a reader can drop the project lock before reading entry bytes.
    pub archive: Option<Arc<Archive>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectInfo {
    pub root: String,
    pub game_dir: String,
    pub name: String,
    pub engine_version: Option<String>,
    /// Only set when it differs from `engine_version`.
    pub script_version: Option<String>,
    pub launcher: Option<String>,
    /// The project ships the executable that runs it, rather than borrowing an SDK.
    pub bundled_engine: bool,
    pub developer: Option<String>,
    pub has_archives: bool,
    pub compiled_only: Vec<String>,
    pub archives: Vec<ArchiveInfo>,
    pub files: Vec<FileInfo>,
    pub stats: Stats,
    pub parse_ms: u64,
    pub engine: Option<EngineSummary>,
    pub stage_images: Option<StageImagesSummary>,
    pub lint_count: Option<u32>,
    pub game: GameInfo,
}

const SKIP_DIRS: &[&str] = &["cache", "saves", "tl"];
const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "webp", "avif", "gif", "svg"];

/// Accepts the project root, its `game/` folder, or a folder containing the project.
pub fn resolve_root(path: &Path) -> Result<(PathBuf, PathBuf), String> {
    if !path.is_dir() {
        return Err(format!("{} is not a folder", path.display()));
    }
    let game = path.join("game");
    if game.is_dir() {
        return Ok((path.to_path_buf(), game));
    }
    let is_game_dir = path
        .file_name()
        .map(|n| n.to_string_lossy().eq_ignore_ascii_case("game"))
        .unwrap_or(false);
    if is_game_dir {
        if let Some(parent) = path.parent() {
            return Ok((parent.to_path_buf(), path.to_path_buf()));
        }
    }
    if let Ok(entries) = fs::read_dir(path) {
        for e in entries.flatten() {
            let p = e.path();
            if p.join("game").is_dir() {
                return Ok((p.clone(), p.join("game")));
            }
        }
    }
    Err(format!(
        "No Ren'Py project found: {} has no `game` folder.",
        path.display()
    ))
}

fn is_skipped_dir(name: &str) -> bool {
    name.starts_with('.') || SKIP_DIRS.contains(&name.to_ascii_lowercase().as_str())
}

/// `game/vnide_developer.rpy`, `game/vnide_live.rpy` and `game/vnide_images.rpy`
/// are written for one launch and deleted when it ends. `vnide_autoreload.rpy`
/// stays until the user turns auto-reload off. None of them are part of the project.
pub fn is_ide_shim(path: &Path) -> bool {
    matches!(
        path.file_stem().and_then(|n| n.to_str()),
        Some("vnide_developer")
            | Some("vnide_live")
            | Some("vnide_images")
            | Some("vnide_autoreload")
    )
}

pub fn is_script_ext(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .as_deref(),
        Some("rpy") | Some("rpym")
    )
}

/// True when `path` (inside `game_dir`) would be loaded as a script.
pub fn is_project_script(game_dir: &Path, path: &Path) -> bool {
    if !is_script_ext(path) || is_ide_shim(path) {
        return false;
    }
    if path.strip_prefix(game_dir).is_err() && !case_inside(game_dir, path) {
        return false;
    }
    let rel = rel_string(game_dir, path);
    if rel.is_empty() || rel.contains(':') || rel.split('/').any(|p| p == "..") {
        return false;
    }
    let comps: Vec<_> = std::path::Path::new(&rel).components().collect();
    comps
        .iter()
        .take(comps.len().saturating_sub(1))
        .all(|c| !is_skipped_dir(&c.as_os_str().to_string_lossy()))
}

fn case_inside(game_dir: &Path, abs: &Path) -> bool {
    let game = game_dir.to_string_lossy().replace('\\', "/");
    let path = abs
        .to_string_lossy()
        .replace('\\', "/")
        .to_ascii_lowercase();
    let prefix = game.trim_end_matches('/').to_ascii_lowercase();
    path == prefix || path.starts_with(&(prefix.clone() + "/"))
}

fn rel_string(game_dir: &Path, abs: &Path) -> String {
    if let Ok(rel) = abs.strip_prefix(game_dir) {
        return rel.to_string_lossy().replace('\\', "/");
    }
    // Watchers on Windows sometimes report a different drive-letter case.
    let game = game_dir.to_string_lossy().replace('\\', "/");
    let path = abs.to_string_lossy().replace('\\', "/");
    let prefix = game.trim_end_matches('/').to_ascii_lowercase();
    let lower = path.to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix(&prefix) {
        let rest = rest.trim_start_matches('/');
        let keep = path.len() - rest.len();
        return path[keep..].trim_start_matches('/').to_string();
    }
    path
}

fn collect_scripts(game_dir: &Path) -> Vec<(String, PathBuf)> {
    fn walk(dir: &Path, game_dir: &Path, out: &mut Vec<(String, PathBuf)>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for e in entries.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().into_owned();
            let Ok(ty) = e.file_type() else { continue };
            if ty.is_dir() {
                if !is_skipped_dir(&name) {
                    walk(&p, game_dir, out);
                }
            } else if is_script_ext(&p) && !is_ide_shim(&p) {
                out.push((rel_string(game_dir, &p), p));
            }
        }
    }
    let mut out = Vec::new();
    walk(game_dir, game_dir, &mut out);
    sort_scripts(&mut out);
    out
}

/// Top-level files win over nested copies when a label is defined twice.
fn sort_scripts(list: &mut [(String, PathBuf)]) {
    list.sort_by(|a, b| {
        let da = a.0.matches('/').count();
        let db = b.0.matches('/').count();
        da.cmp(&db)
            .then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase()))
    });
}

pub fn load_file(abs: &Path, rel: &str) -> SourceFile {
    let bytes = fs::read(abs).unwrap_or_default();
    load_from_bytes(rel, abs, &bytes, Origin::Loose)
}

fn load_from_bytes(rel: &str, abs: &Path, bytes: &[u8], origin: Origin) -> SourceFile {
    let size = bytes.len() as u64;
    let text = String::from_utf8_lossy(bytes);
    let lexed = lex(&text);
    let mut parsed = parse(&lexed.lines);
    let mut meta = scan_meta(&lexed.lines);
    meta.data_refs = std::mem::take(&mut parsed.data_refs);
    let mut issues: Vec<Issue> = lexed
        .issues
        .into_iter()
        .map(|i| Issue {
            line: i.line,
            message: i.message,
        })
        .collect();
    issues.extend(parsed.issues);
    let init_hash = hash_init(&lexed.lines, &parsed.stmts);
    SourceFile {
        rel: rel.to_string(),
        abs: abs.to_path_buf(),
        origin,
        bytes: size,
        content_hash: hash_bytes(bytes),
        init_hash,
        total_lines: lexed.total_lines,
        stmts: Arc::new(parsed.stmts),
        issues,
        opaque: parsed.opaque,
        meta: Arc::new(meta),
        source: None,
        engine: None,
        decompile_reasons: Vec::new(),
    }
}

fn hash_bytes(bytes: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut h);
    h.finish()
}

/// Source text of every top-level statement that is not a label or a screen.
fn hash_init(lines: &[crate::lexer::LLine], stmts: &[Stmt]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    let mut cursor = 0usize;
    for stmt in stmts {
        if matches!(stmt.kind, Kind::Label { .. } | Kind::Screen { .. }) {
            continue;
        }
        while cursor < lines.len() && lines[cursor].line < stmt.line {
            cursor += 1;
        }
        let mut j = cursor;
        while j < lines.len() && lines[j].line <= stmt.end_line {
            lines[j].text.hash(&mut h);
            j += 1;
        }
    }
    h.finish()
}

fn index_images(game_dir: &Path) -> ImageIndex {
    let mut idx = ImageIndex::default();
    let images = fs::read_dir(game_dir).ok().and_then(|rd| {
        rd.flatten()
            .find(|e| {
                e.file_name()
                    .to_string_lossy()
                    .eq_ignore_ascii_case("images")
            })
            .map(|e| e.path())
    });
    let Some(images) = images else { return idx };
    fn walk(dir: &Path, game_dir: &Path, idx: &mut ImageIndex) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for e in entries.flatten() {
            let p = e.path();
            let Ok(ty) = e.file_type() else { continue };
            if ty.is_dir() {
                walk(&p, game_dir, idx);
                continue;
            }
            let ext_ok = p
                .extension()
                .and_then(|x| x.to_str())
                .map(|x| IMAGE_EXTS.contains(&x.to_ascii_lowercase().as_str()))
                .unwrap_or(false);
            if !ext_ok {
                continue;
            }
            if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                let rel = p
                    .strip_prefix(game_dir)
                    .unwrap_or(&p)
                    .to_string_lossy()
                    .replace('\\', "/");
                remember_image(&mut *idx, stem, &rel);
            }
        }
    }
    walk(&images, game_dir, &mut idx);
    idx
}

fn remember_image(idx: &mut ImageIndex, stem: &str, rel: &str) {
    let name = collapse_ws(&stem.to_lowercase());
    if name.is_empty() {
        return;
    }
    if let Some(tag) = name.split(' ').next() {
        if !tag.is_empty() {
            idx.tags.insert(tag.to_string());
        }
    }
    idx.names.insert(name.clone());
    idx.files.entry(name).or_insert_with(|| rel.to_string());
}

/// Archive entries under `images/`. A loose file of the same name is kept.
fn index_archive_images(loaded: &[LoadedArchive], idx: &mut ImageIndex) {
    for loaded in loaded {
        let Some(archive) = &loaded.archive else {
            continue;
        };
        for name in archive.entries.keys() {
            let rel = name.replace('\\', "/");
            let lower = rel.to_ascii_lowercase();
            let Some(rest) = lower.strip_prefix("images/") else {
                continue;
            };
            if rest.is_empty() || rest.ends_with('/') {
                continue;
            }
            let ext_ok = rest
                .rsplit('.')
                .next()
                .is_some_and(|ext| IMAGE_EXTS.contains(&ext));
            if !ext_ok {
                continue;
            }
            let file = rest.rsplit('/').next().unwrap_or(rest);
            let stem = file.rsplit_once('.').map(|(stem, _)| stem).unwrap_or(file);
            let image = collapse_ws(&stem.to_lowercase());
            if image.is_empty() || idx.files.contains_key(&image) {
                continue;
            }
            remember_image(idx, stem, &rel);
        }
    }
}

/// Version of the Ren'Py engine bundled with the game: `renpy/vc_version.py`
/// (8.x), then a literal `version_tuple` in `renpy/__init__.py` (7.x).
pub fn read_engine_version(root: &Path) -> Option<String> {
    let first_three = |nums: Vec<&str>| -> Option<String> {
        let nums: Vec<&str> = nums.into_iter().take(3).collect();
        (nums.len() == 3
            && nums
                .iter()
                .all(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit())))
        .then(|| nums.join("."))
    };
    if let Ok(text) = fs::read_to_string(root.join("renpy").join("vc_version.py")) {
        for line in text.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("version") {
                let rest = rest.trim_start();
                if let Some(rest) = rest.strip_prefix('=') {
                    let v = rest.trim().trim_matches(|c| c == '\'' || c == '"');
                    if let Some(found) = first_three(v.split('.').collect()) {
                        return Some(found);
                    }
                }
            }
        }
    }
    let init = fs::read_to_string(root.join("renpy").join("__init__.py")).ok()?;
    let at = init.find("version_tuple = (")?;
    let tail = &init[at + "version_tuple = (".len()..];
    let close = tail.find(')')?;
    first_three(tail[..close].split(',').map(|s| s.trim()).collect())
}

fn version_at_least(version: Option<&str>, major: u32, minor: u32) -> bool {
    let Some(version) = version else {
        return false;
    };
    let mut parts = version.split('.');
    let maj: u32 = parts.next().unwrap_or("0").parse().unwrap_or(0);
    let min: u32 = parts.next().unwrap_or("0").parse().unwrap_or(0);
    (maj, min) >= (major, minor)
}

/// Strip a Ren'Py string literal, including a `_("...")` translation wrapper.
fn plain_config(raw: &str) -> String {
    let mut s = raw.trim().trim_end_matches(',').trim();
    if let Some(rest) = s.strip_prefix("_(").or_else(|| s.strip_prefix("__(")) {
        if let Some(end) = rest.rfind(')') {
            s = rest[..end].trim();
        }
    }
    for q in ['"', '\''] {
        if s.len() >= 2 && s.starts_with(q) && s.ends_with(q) {
            return s[1..s.len() - 1].replace("\\\"", "\"").replace("\\'", "'");
        }
    }
    s.to_string()
}

fn format_system_time(t: SystemTime) -> Option<String> {
    t.duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| crate::rpa::format_utc(d.as_secs()))
}

/// Oldest and newest modification times of archive and compiled-script files.
fn file_date_span(game_dir: &Path) -> (Option<String>, Option<String>) {
    let mut oldest: Option<SystemTime> = None;
    let mut newest: Option<SystemTime> = None;
    fn walk(dir: &Path, oldest: &mut Option<SystemTime>, newest: &mut Option<SystemTime>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for e in entries.flatten() {
            let Ok(ty) = e.file_type() else { continue };
            if ty.is_dir() {
                if !is_skipped_dir(&e.file_name().to_string_lossy()) {
                    walk(&e.path(), oldest, newest);
                }
                continue;
            }
            let ext = e
                .path()
                .extension()
                .and_then(|x| x.to_str())
                .map(|x| x.to_ascii_lowercase())
                .unwrap_or_default();
            if !matches!(ext.as_str(), "rpa" | "rpi" | "rpyc" | "rpymc") {
                continue;
            }
            let Ok(modified) = e.metadata().and_then(|m| m.modified()) else {
                continue;
            };
            match *oldest {
                Some(t) if modified >= t => {}
                _ => *oldest = Some(modified),
            }
            match *newest {
                Some(t) if modified <= t => {}
                _ => *newest = Some(modified),
            }
        }
    }
    walk(game_dir, &mut oldest, &mut newest);
    (
        oldest.and_then(format_system_time),
        newest.and_then(format_system_time),
    )
}

/// SHA-256 of a file, streamed so a large archive is not loaded whole.
pub fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|e| format!("Could not read {}: {e}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| format!("Could not read {}: {e}", path.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn read_script_version(game_dir: &Path) -> Option<String> {
    if let Ok(text) = fs::read_to_string(game_dir.join("script_version.txt")) {
        let nums: Vec<&str> = text
            .trim()
            .trim_matches(|c| c == '(' || c == ')')
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();
        if !nums.is_empty() && nums.iter().all(|n| n.chars().all(|c| c.is_ascii_digit())) {
            return Some(nums.join("."));
        }
    }
    None
}

/// `.rpyc` files with no `.rpy` beside them and `.rpa` archives, plus a
/// fingerprint of their sizes (used to tell when a cached engine dump is stale).
fn scan_compiled(game_dir: &Path) -> (Vec<String>, Vec<String>, u64) {
    use std::hash::{Hash, Hasher};
    let mut compiled = Vec::new();
    let mut archives = Vec::new();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    fn walk(
        dir: &Path,
        game_dir: &Path,
        compiled: &mut Vec<String>,
        archives: &mut Vec<String>,
        h: &mut impl Hasher,
    ) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let p = e.path();
            let name = e.file_name().to_string_lossy().into_owned();
            let Ok(ty) = e.file_type() else { continue };
            if ty.is_dir() {
                if !is_skipped_dir(&name) {
                    walk(&p, game_dir, compiled, archives, h);
                }
                continue;
            }
            let ext = p
                .extension()
                .and_then(|x| x.to_str())
                .map(|x| x.to_ascii_lowercase())
                .unwrap_or_default();
            match ext.as_str() {
                "rpyc" | "rpymc" => {
                    let source = p.with_extension(if ext == "rpyc" { "rpy" } else { "rpym" });
                    rel_string(game_dir, &p).hash(h);
                    e.metadata().map(|m| m.len()).unwrap_or(0).hash(h);
                    if !source.is_file() {
                        compiled.push(rel_string(game_dir, &p));
                    }
                }
                "rpa" => {
                    rel_string(game_dir, &p).hash(h);
                    e.metadata().map(|m| m.len()).unwrap_or(0).hash(h);
                    // RPA-1.0 keeps the index in a sibling .rpi and raw bytes in the .rpa.
                    let rpi = p.with_extension("rpi");
                    if rpi.is_file() {
                        archives.push(rel_string(game_dir, &rpi));
                    } else {
                        archives.push(rel_string(game_dir, &p));
                    }
                }
                "rpi" => {
                    let rpa = p.with_extension("rpa");
                    if !rpa.is_file() {
                        rel_string(game_dir, &p).hash(h);
                        archives.push(rel_string(game_dir, &p));
                    }
                }
                _ => {}
            }
        }
    }
    walk(
        game_dir,
        game_dir,
        &mut compiled,
        &mut archives,
        &mut hasher,
    );
    (compiled, archives, hasher.finish())
}

pub fn is_archive_path(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .as_deref(),
        Some("rpa") | Some("rpi")
    )
}

fn loose_path(game_dir: &Path, rel: &str) -> PathBuf {
    let mut path = game_dir.to_path_buf();
    for part in rel.split('/') {
        if !part.is_empty() {
            path.push(part);
        }
    }
    path
}

fn name_ext(name: &str) -> &str {
    name.rsplit_once('.').map(|(_, ext)| ext).unwrap_or("")
}

fn is_script_name(name: &str) -> bool {
    matches!(name_ext(name).to_ascii_lowercase().as_str(), "rpy" | "rpym")
}

fn is_compiled_name(name: &str) -> bool {
    matches!(
        name_ext(name).to_ascii_lowercase().as_str(),
        "rpyc" | "rpymc"
    )
}

fn source_of_compiled(name: &str) -> Option<String> {
    let rel = name.replace('\\', "/");
    match name_ext(&rel).to_ascii_lowercase().as_str() {
        "rpyc" if rel.len() > 5 => Some(format!("{}.rpy", &rel[..rel.len() - 5])),
        "rpymc" if rel.len() > 6 => Some(format!("{}.rpym", &rel[..rel.len() - 6])),
        _ => None,
    }
}

struct CompiledJob {
    rpyc_rel: String,
    rpy_rel: String,
    bytes: Vec<u8>,
    archive: Option<String>,
}

fn recover_compiled(
    game_dir: &Path,
    jobs: &[CompiledJob],
    cache: Option<&Path>,
) -> (Vec<SourceFile>, Vec<String>) {
    let out: Vec<Result<SourceFile, String>> = jobs
        .par_iter()
        .map(|job| match cached_decompile(&job.bytes, cache) {
            Ok(got) => Ok(source_from_decompiled(game_dir, job, &got)),
            Err(e) => Err(format!("{} ({e})", job.rpyc_rel)),
        })
        .collect();
    let mut files = Vec::new();
    let mut failed = Vec::new();
    for item in out {
        match item {
            Ok(file) => files.push(file),
            Err(name) => failed.push(name),
        }
    }
    (files, failed)
}

fn source_from_decompiled(
    game_dir: &Path,
    job: &CompiledJob,
    got: &crate::rpyc::Decompiled,
) -> SourceFile {
    let abs = loose_path(game_dir, &job.rpy_rel);
    let mut file = load_from_bytes(
        &job.rpy_rel,
        &abs,
        got.text.as_bytes(),
        Origin::Compiled {
            archive: job.archive.clone(),
            editable: got.editable,
        },
    );
    file.source = Some(Arc::from(got.text.as_str()));
    file.decompile_reasons = got.reasons.clone();
    if let Some(name) = got.engine_file.clone() {
        if !got.engine_lines.is_empty() {
            file.engine = Some((name, got.engine_lines.clone()));
        }
    }
    file
}

fn cached_decompile(bytes: &[u8], cache: Option<&Path>) -> Result<crate::rpyc::Decompiled, String> {
    let crc = crc32fast::hash(bytes);
    if let Some(dir) = cache {
        if let Some(got) = read_decompile_cache(dir, crc) {
            return Ok(got);
        }
    }
    let got = crate::rpyc::decompile(bytes)?;
    if let Some(dir) = cache {
        let _ = write_decompile_cache(dir, crc, &got);
    }
    Ok(got)
}

fn read_decompile_cache(dir: &Path, crc: u32) -> Option<crate::rpyc::Decompiled> {
    let text = fs::read_to_string(dir.join(format!("{crc:08x}.txt"))).ok()?;
    let (version, rest) = text.split_once('\n')?;
    if version != "v2" && version != "v3" {
        return None;
    }
    // v2 is flag, file, lines, then the script. v3 inserts a reasons line before the script.
    let parts = if version == "v3" { 5 } else { 4 };
    let mut lines = rest.splitn(parts, '\n');
    let editable = lines.next()? == "1";
    let file = lines.next()?;
    let engine_file = if file.is_empty() || file == "-" {
        None
    } else {
        Some(file.to_string())
    };
    let nums = lines.next()?;
    let mut engine_lines = Vec::new();
    if !nums.is_empty() {
        for part in nums.split(',') {
            engine_lines.push(part.parse().ok()?);
        }
    }
    let (reasons, body) = if version == "v3" {
        let raw = lines.next().unwrap_or("");
        let reasons = if raw.is_empty() {
            Vec::new()
        } else {
            raw.split('\t').map(|s| s.to_string()).collect()
        };
        (reasons, lines.next().unwrap_or("").to_string())
    } else {
        (Vec::new(), lines.next().unwrap_or("").to_string())
    };
    Some(crate::rpyc::Decompiled {
        text: body,
        editable,
        partial: !editable,
        reasons,
        engine_lines,
        engine_file,
    })
}

fn write_decompile_cache(
    dir: &Path,
    crc: u32,
    got: &crate::rpyc::Decompiled,
) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let flag = if got.editable { "1" } else { "0" };
    let file = got
        .engine_file
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or("-");
    let mut nums = String::new();
    for (i, n) in got.engine_lines.iter().enumerate() {
        if i > 0 {
            nums.push(',');
        }
        nums.push_str(&n.to_string());
    }
    let reasons = got
        .reasons
        .iter()
        .map(|r| r.replace(['\t', '\n', '\r'], " "))
        .collect::<Vec<_>>()
        .join("\t");
    let body = format!("v3\n{flag}\n{file}\n{nums}\n{reasons}\n{}", got.text);
    fs::write(dir.join(format!("{crc:08x}.txt")), body).map_err(|e| e.to_string())
}

fn loose_compiled_jobs(game_dir: &Path, rpyc_rels: &[String]) -> Vec<CompiledJob> {
    rpyc_rels
        .iter()
        .filter_map(|rel| {
            let rpy = source_of_compiled(rel)?;
            let bytes = fs::read(loose_path(game_dir, rel)).ok()?;
            Some(CompiledJob {
                rpyc_rel: rel.clone(),
                rpy_rel: rpy,
                bytes,
                archive: None,
            })
        })
        .collect()
}

/// Stem used by Ren'Py when it sorts archives (`path.rpartition(".")`).
fn archive_rank(rel: &str) -> (String, String, String) {
    match rel.rsplit_once('.') {
        Some((stem, ext)) => (stem.to_string(), format!(".{ext}"), rel.to_string()),
        None => (rel.to_string(), String::new(), rel.to_string()),
    }
}

fn sort_archives(list: &mut [LoadedArchive]) {
    // `index_archives` does `arc_files.sort(reverse=True)`, so the name that
    // sorts last is searched first.
    list.sort_by(|a, b| archive_rank(&b.info.path).cmp(&archive_rank(&a.info.path)));
}

fn sort_source_files(files: &mut [SourceFile]) {
    let mut order: Vec<(String, PathBuf)> = files
        .iter()
        .map(|f| (f.rel.clone(), f.abs.clone()))
        .collect();
    sort_scripts(&mut order);
    let rank: HashMap<&str, usize> = order
        .iter()
        .enumerate()
        .map(|(i, (rel, _))| (rel.as_str(), i))
        .collect();
    files.sort_by_key(|f| rank.get(f.rel.as_str()).copied().unwrap_or(usize::MAX));
}

fn rel_key(rel: &str) -> String {
    #[cfg(windows)]
    {
        rel.to_ascii_lowercase()
    }
    #[cfg(not(windows))]
    {
        rel.to_string()
    }
}

fn open_archives(game_dir: &Path, paths: &[String]) -> Vec<LoadedArchive> {
    paths
        .iter()
        .map(|rel| {
            let abs = loose_path(game_dir, rel);
            match Archive::open(&abs) {
                Ok(archive) => LoadedArchive {
                    info: ArchiveInfo {
                        path: rel.clone(),
                        version: archive.version_label(),
                        read_only_format: archive.read_only_format(),
                        entries: archive.entries.len() as u32,
                        scripts: 0,
                        compiled_only: 0,
                        other_bytes: 0,
                        is_patch: rel.contains("vnide_patch")
                            || archive.entries.contains_key(rpa::MANIFEST_NAME),
                        nested: rel.contains('/'),
                        overrides: 0,
                        stale: Vec::new(),
                        error: None,
                    },
                    archive: Some(Arc::new(archive)),
                },
                Err(e) => LoadedArchive {
                    info: ArchiveInfo {
                        path: rel.clone(),
                        version: e.known_version.clone().unwrap_or_default(),
                        read_only_format: e.known_version.is_some(),
                        entries: 0,
                        scripts: 0,
                        compiled_only: 0,
                        other_bytes: 0,
                        is_patch: rel.contains("vnide_patch"),
                        nested: rel.contains('/'),
                        overrides: 0,
                        stale: Vec::new(),
                        error: Some(e.to_string()),
                    },
                    archive: None,
                },
            }
        })
        .collect()
}

/// When an archive header is unknown, say which script registers a custom loader.
fn annotate_custom_handlers(game_dir: &Path, loaded: &mut [LoadedArchive]) {
    let unknown = loaded.iter().any(|a| {
        a.info
            .error
            .as_ref()
            .is_some_and(|e| e.contains("unsupported"))
    });
    if !unknown {
        return;
    }
    let Some(script) = find_handler_script(game_dir) else {
        return;
    };
    for arch in loaded.iter_mut() {
        if let Some(err) = &mut arch.info.error {
            if err.contains("unsupported") {
                err.push_str(&format!(
                    " A custom archive handler is registered in {script}."
                ));
            }
        }
    }
}

fn find_handler_script(game_dir: &Path) -> Option<String> {
    find_handler_in(game_dir, game_dir)
}

fn find_handler_in(game_dir: &Path, dir: &Path) -> Option<String> {
    let entries = fs::read_dir(dir).ok()?;
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let Ok(ty) = e.file_type() else { continue };
        if ty.is_dir() {
            if is_skipped_dir(&name) {
                continue;
            }
            if let Some(hit) = find_handler_in(game_dir, &e.path()) {
                return Some(hit);
            }
            continue;
        }
        let lower = name.to_ascii_lowercase();
        if !(lower.ends_with(".rpy") || lower.ends_with(".py")) {
            continue;
        }
        let Ok(meta) = e.metadata() else { continue };
        if meta.len() > 2_000_000 {
            continue;
        }
        let Ok(text) = fs::read_to_string(e.path()) else {
            continue;
        };
        if text.contains("archive_handlers") {
            return Some(rel_string(game_dir, &e.path()));
        }
    }
    None
}

fn winning_archive(loaded: &[LoadedArchive], rel: &str) -> Option<String> {
    loaded.iter().find_map(|arch| {
        let archive = arch.archive.as_ref()?;
        archive
            .entries
            .contains_key(rel)
            .then(|| arch.info.path.clone())
    })
}

fn take_archived_scripts(
    game_dir: &Path,
    loaded: &[LoadedArchive],
    loose: &[SourceFile],
    cache: Option<&Path>,
) -> (Vec<SourceFile>, Vec<String>, Vec<String>) {
    let mut seen: HashSet<String> = loose.iter().map(|f| f.rel.clone()).collect();
    let mut jobs: Vec<(String, String)> = Vec::new();
    let mut shadowed = Vec::new();
    let mut compiled = Vec::new();
    let mut compiled_jobs: Vec<CompiledJob> = Vec::new();
    for arch in loaded {
        let Some(archive) = &arch.archive else {
            continue;
        };
        for name in archive.entries.keys() {
            let rel = name.replace('\\', "/");
            if is_script_name(&rel) {
                if !seen.insert(rel.clone()) {
                    shadowed.push(format!(
                        "{rel} in {} (hidden by a loose file or a higher-priority archive)",
                        arch.info.path
                    ));
                    continue;
                }
                jobs.push((arch.info.path.clone(), rel));
            } else if is_compiled_name(&rel) {
                let Some(source) = source_of_compiled(&rel) else {
                    continue;
                };
                if !seen.insert(source.clone()) {
                    continue;
                }
                match archive.read_entry(&rel, rpa::SCRIPT_MAX) {
                    Ok(bytes) => compiled_jobs.push(CompiledJob {
                        rpyc_rel: rel,
                        rpy_rel: source,
                        bytes,
                        archive: Some(arch.info.path.clone()),
                    }),
                    Err(e) => compiled.push(format!("{rel} ({e})")),
                }
            }
        }
    }
    let mut files: Vec<SourceFile> = jobs
        .par_iter()
        .filter_map(|(archive_rel, rel)| read_archived_script(game_dir, loaded, archive_rel, rel))
        .collect();
    let (decompiled, failed) = recover_compiled(game_dir, &compiled_jobs, cache);
    files.extend(decompiled);
    compiled.extend(failed);
    (files, compiled, shadowed)
}

fn read_archived_script(
    game_dir: &Path,
    loaded: &[LoadedArchive],
    archive_rel: &str,
    rel: &str,
) -> Option<SourceFile> {
    let archive = loaded
        .iter()
        .find(|a| a.info.path == archive_rel)?
        .archive
        .as_ref()?;
    let bytes = archive.read_entry(rel, rpa::SCRIPT_MAX).ok()?;
    Some(load_from_bytes(
        rel,
        &loose_path(game_dir, rel),
        &bytes,
        Origin::Archived {
            archive: archive_rel.to_string(),
        },
    ))
}

fn fill_archive_stats(loaded: &mut [LoadedArchive], files: &[SourceFile]) {
    for arch in loaded.iter_mut() {
        let Some(archive) = &arch.archive else {
            continue;
        };
        let mut scripts = 0u32;
        let mut compiled = 0u32;
        let mut other = 0u64;
        for (name, entry) in &archive.entries {
            let len = entry.declared_len().unwrap_or(0);
            if name == rpa::MANIFEST_NAME {
                continue;
            } else if is_script_name(name) {
                scripts += 1;
            } else if is_compiled_name(name) {
                let recovered =
                    source_of_compiled(name).is_some_and(|src| files.iter().any(|f| f.rel == src));
                if recovered {
                    scripts += 1;
                } else {
                    compiled += 1;
                }
            } else {
                other += len;
            }
        }
        let overrides = files
            .iter()
            .filter(
                |f| matches!(&f.origin, Origin::Override { archive } if archive == &arch.info.path),
            )
            .count() as u32;
        arch.info.entries = archive.entries.len() as u32;
        arch.info.scripts = scripts;
        arch.info.compiled_only = compiled;
        arch.info.other_bytes = other;
        arch.info.overrides = overrides;
    }
}

fn stale_entries(game_dir: &Path, loaded: &[LoadedArchive]) -> Vec<String> {
    let mut found = Vec::new();
    for arch in loaded {
        let Some(archive) = &arch.archive else {
            continue;
        };
        let Ok(Some(manifest)) = rpa::read_manifest(archive) else {
            continue;
        };
        let stale = rpa::check_stale(&manifest, |source, name| {
            let other = loaded
                .iter()
                .find(|a| a.info.path == source)?
                .archive
                .as_ref()?;
            other
                .copy_entry(name, &mut std::io::sink())
                .ok()
                .map(|s| s.crc32)
        });
        found.extend(stale);
        found.extend(loose_stale(game_dir, &manifest));
    }
    found
}

/// Patches of a loose `.rpyc` have no source archive. Compare the file on disk,
/// when the game update put one back, with the CRC recorded at bake time.
fn loose_stale(game_dir: &Path, manifest: &rpa::PatchManifest) -> Vec<String> {
    let mut stale = Vec::new();
    for (name, entry) in &manifest.entries {
        if entry.generated || entry.source.is_some() {
            continue;
        }
        let Some(expected) = entry.base_crc32 else {
            continue;
        };
        let Some(rpyc) = compiled_sibling(name) else {
            continue;
        };
        let Ok(bytes) = fs::read(loose_path(game_dir, &rpyc)) else {
            continue;
        };
        if crc32fast::hash(&bytes) != expected {
            stale.push(format!("{name} on disk changed after it was patched"));
        }
    }
    stale
}

fn compiled_sibling(name: &str) -> Option<String> {
    if let Some(stem) = name.strip_suffix(".rpy") {
        Some(format!("{stem}.rpyc"))
    } else if let Some(stem) = name.strip_suffix(".rpym") {
        Some(format!("{stem}.rpymc"))
    } else {
        None
    }
}

fn apply_stale(loaded: &mut [LoadedArchive], stale: &[String]) {
    for arch in loaded.iter_mut() {
        if arch.info.is_patch {
            arch.info.stale = stale.to_vec();
            continue;
        }
        let path = &arch.info.path;
        arch.info.stale = stale
            .iter()
            .filter(|line| {
                line.contains(&format!(" {path} ")) || line.contains(&format!(" {path},"))
            })
            .cloned()
            .collect();
    }
}

fn unreadable_archives(archives: &[LoadedArchive]) -> Vec<String> {
    archives
        .iter()
        .filter_map(|a| {
            if let Some(err) = &a.info.error {
                Some(format!("{} ({err})", a.info.path))
            } else if a.info.scripts == 0 && a.info.compiled_only > 0 {
                Some(format!("{} (compiled scripts only)", a.info.path))
            } else {
                None
            }
        })
        .collect()
}

fn find_launcher(root: &Path) -> Option<Launcher> {
    let mut exes: Vec<PathBuf> = Vec::new();
    if let Ok(entries) = fs::read_dir(root) {
        for e in entries.flatten() {
            let p = e.path();
            let is_exe = p
                .extension()
                .and_then(|x| x.to_str())
                .map(|x| x.eq_ignore_ascii_case("exe"))
                .unwrap_or(false);
            let name = e.file_name().to_string_lossy().to_lowercase();
            if is_exe && !name.starts_with("python") && !name.starts_with("unins") {
                exes.push(p);
            }
        }
    }
    // Prefer the exe that has a matching `.py`, and the default build over `-32`.
    exes.sort_by_key(|p| {
        let stem = p
            .file_stem()
            .map(|s| s.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        (!p.with_extension("py").exists(), stem.ends_with("-32"))
    });
    if let Some(exe) = exes.into_iter().next() {
        return Some(Launcher {
            exe,
            prefix_args: Vec::new(),
        });
    }
    if let Some(launcher) = script_launcher(root, &[]) {
        return Some(launcher);
    }
    // Project inside an SDK: <sdk>/renpy.exe <project>, or renpy.sh / renpy.app.
    let mut cur = root.parent();
    for _ in 0..3 {
        let Some(dir) = cur else { break };
        let sdk = dir.join("renpy.exe");
        if sdk.is_file() {
            return Some(Launcher {
                exe: sdk,
                prefix_args: vec![root.to_string_lossy().into_owned()],
            });
        }
        if let Some(launcher) = script_launcher(dir, &[root.to_string_lossy().into_owned()]) {
            return Some(launcher);
        }
        cur = dir.parent();
    }
    None
}

/// `renpy.sh`, or the executable inside `renpy.app` on macOS.
fn script_launcher(dir: &Path, prefix: &[String]) -> Option<Launcher> {
    let sh = dir.join("renpy.sh");
    if sh.is_file() {
        return Some(Launcher {
            exe: sh,
            prefix_args: prefix.to_vec(),
        });
    }
    let mac = dir
        .join("renpy.app")
        .join("Contents")
        .join("MacOS")
        .join("renpy");
    if mac.is_file() {
        return Some(Launcher {
            exe: mac,
            prefix_args: prefix.to_vec(),
        });
    }
    None
}

/// Where Ren'Py looks for a file name that has no folder (`config.search_prefixes`).
const SEARCH_PREFIXES: [&str; 2] = ["", "images/"];

impl Project {
    /// The path relative to `game/` of a picture or sound as the game would
    /// load it: the name as written, then under `images/`. Loose files and
    /// archive entries both count, and a loose file wins. `None` when neither
    /// place has it.
    pub fn find_asset(&self, rel: &str) -> Option<String> {
        let clean = rel.replace('\\', "/");
        let clean = clean.trim_start_matches('/');
        if clean.is_empty() || clean.split('/').any(|part| part == "..") {
            return None;
        }
        for prefix in SEARCH_PREFIXES {
            let candidate = format!("{prefix}{clean}");
            if prefix == "images/" && clean.to_ascii_lowercase().starts_with("images/") {
                continue;
            }
            if self
                .game_dir
                .join(candidate.replace('/', std::path::MAIN_SEPARATOR_STR))
                .is_file()
            {
                return Some(candidate);
            }
            for loaded in &self.archives {
                let Some(archive) = &loaded.archive else {
                    continue;
                };
                if archive.entries.contains_key(&candidate) {
                    return Some(candidate);
                }
                let hit = archive.entries.keys().find(|name| {
                    name.replace('\\', "/")
                        .trim_start_matches('/')
                        .eq_ignore_ascii_case(&candidate)
                });
                if let Some(name) = hit {
                    return Some(name.replace('\\', "/").trim_start_matches('/').to_string());
                }
            }
        }
        None
    }

    pub fn open(path: &Path) -> Result<Project, String> {
        Self::open_with(path, None)
    }

    /// `cache_dir` stores decompiled scripts by the `.rpyc` checksum so the
    /// next open does not repeat the work. `None` decompiles every time.
    pub fn open_with(path: &Path, cache_dir: Option<&Path>) -> Result<Project, String> {
        let started = Instant::now();
        let (root, game_dir) = resolve_root(path)?;
        let scripts = collect_scripts(&game_dir);
        let (compiled_only, archive_paths, compiled_fp) = scan_compiled(&game_dir);
        if scripts.is_empty() && compiled_only.is_empty() && archive_paths.is_empty() {
            return Err(format!(
                "No Ren'Py scripts found under {} (no .rpy, .rpyc or .rpa files).",
                game_dir.display()
            ));
        }
        let mut files: Vec<SourceFile> = scripts
            .par_iter()
            .map(|(rel, abs)| load_file(abs, rel))
            .collect();
        let loose_jobs = loose_compiled_jobs(&game_dir, &compiled_only);
        let (decompiled, failed) = recover_compiled(&game_dir, &loose_jobs, cache_dir);
        files.extend(decompiled);
        let mut loaded = open_archives(&game_dir, &archive_paths);
        annotate_custom_handlers(&game_dir, &mut loaded);
        sort_archives(&mut loaded);
        let (archived, extra_compiled, shadowed) =
            take_archived_scripts(&game_dir, &loaded, &files, cache_dir);
        for file in &mut files {
            if matches!(file.origin, Origin::Loose) {
                if let Some(archive) = winning_archive(&loaded, &file.rel) {
                    file.origin = Origin::Override { archive };
                }
            }
        }
        files.extend(archived);
        sort_source_files(&mut files);
        let mut compiled_only = failed;
        compiled_only.extend(extra_compiled);
        compiled_only.sort();
        compiled_only.dedup();
        fill_archive_stats(&mut loaded, &files);
        let stale = stale_entries(&game_dir, &loaded);
        apply_stale(&mut loaded, &stale);
        let mut auto_images = index_images(&game_dir);
        index_archive_images(&loaded, &mut auto_images);
        let mut project = Project {
            engine_version: read_engine_version(&root),
            script_version: read_script_version(&game_dir),
            has_archives: !archive_paths.is_empty(),
            compiled_only,
            archives: loaded,
            shadowed,
            stale,
            compiled_fp,
            launcher: find_launcher(&root),
            root,
            game_dir,
            files,
            file_of: HashMap::new(),
            key_cache: None,
            graph_cache: Mutex::new(HashMap::new()),
            auto_images,
            analysis: Arc::new(Analysis::default()),
            analysis_epoch: 0,
            installed_epoch: 0,
            parse_ms: 0,
            engine: None,
            stage_images: None,
            lint: None,
            cache_dir: cache_dir.map(|p| p.to_path_buf()),
            stage_cache: Mutex::new(None),
        };
        project.reanalyze();
        project.parse_ms = started.elapsed().as_millis();
        Ok(project)
    }

    pub fn reanalyze(&mut self) {
        self.analysis_epoch = self.analysis_epoch.wrapping_add(1);
        let analysis = self.analysis_fork().run();
        self.install_analysis(analysis);
    }

    /// Clone the inputs analysis needs and bump the epoch. The caller drops the
    /// project mutex, runs `AnalysisFork::run`, then `publish_analysis`.
    pub fn fork_analysis(&mut self) -> AnalysisFork {
        self.analysis_epoch = self.analysis_epoch.wrapping_add(1);
        self.analysis_fork()
    }

    fn analysis_fork(&self) -> AnalysisFork {
        AnalysisFork {
            epoch: self.analysis_epoch,
            files: self.files.clone(),
            auto_images: self.auto_images.clone(),
            compiled_only: self.compiled_only.clone(),
            unreadable: unreadable_archives(&self.archives),
            shadowed: self.shadowed.clone(),
            stale: self.stale.clone(),
            engine: self.engine.as_ref().map(|e| e.dump.clone()),
            lint: self.lint.clone(),
        }
    }

    fn install_analysis(&mut self, analysis: Arc<Analysis>) {
        self.analysis = analysis;
        self.installed_epoch = self.analysis_epoch;
        self.rebuild_file_index();
        self.key_cache = Some((self.compute_source_key(), self.compute_init_key()));
        self.clear_stage_cache();
    }

    /// Store `analysis` when no newer run has started. Returns false when a
    /// later edit already owns the epoch, so a slow run cannot overwrite it.
    pub fn publish_analysis(&mut self, epoch: u64, analysis: Arc<Analysis>) -> bool {
        if self.analysis_epoch != epoch {
            return false;
        }
        self.install_analysis(analysis);
        true
    }

    fn rebuild_file_index(&mut self) {
        self.file_of = self
            .files
            .iter()
            .enumerate()
            .map(|(i, f)| (rel_key(&f.rel), i))
            .collect();
    }

    /// Fingerprint of everything an engine dump depends on.
    pub fn source_key(&self) -> String {
        if let Some((key, _)) = &self.key_cache {
            return key.clone();
        }
        self.compute_source_key()
    }

    fn compute_source_key(&self) -> String {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.compiled_fp.hash(&mut h);
        for f in &self.files {
            f.rel.hash(&mut h);
            f.content_hash.hash(&mut h);
        }
        format!("{:016x}", h.finish())
    }

    /// Fingerprint of init code and image files. Dialogue and label edits do
    /// not change it, so an image list can stay fresh across those saves.
    pub fn init_key(&self) -> String {
        if let Some((_, key)) = &self.key_cache {
            return key.clone();
        }
        self.compute_init_key()
    }

    fn compute_init_key(&self) -> String {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        IMAGE_DUMP_VERSION.hash(&mut h);
        self.compiled_fp.hash(&mut h);
        for f in &self.files {
            f.rel.hash(&mut h);
            f.init_hash.hash(&mut h);
        }
        let mut images: Vec<(&String, &String)> = self.auto_images.files.iter().collect();
        images.sort_by(|a, b| a.0.cmp(b.0).then(a.1.cmp(b.1)));
        for (name, path) in images {
            name.hash(&mut h);
            path.hash(&mut h);
        }
        format!("{:016x}", h.finish())
    }

    pub fn engine_target(&self) -> Result<EngineTarget, String> {
        let launcher = self.launcher.clone().ok_or_else(|| {
            "No launcher found next to the project. Choose one with \"Set launcher\".".to_string()
        })?;
        Ok(EngineTarget {
            root: self.root.clone(),
            game_dir: self.game_dir.clone(),
            launcher,
            key: self.source_key(),
            engine_version: self.engine_version.clone(),
        })
    }

    pub fn set_engine(&mut self, run: EngineRun) {
        self.engine = Some(run);
        self.reanalyze();
    }

    pub fn set_stage_images(&mut self, run: ImageRun) {
        self.stage_images = Some(run);
        self.clear_stage_cache();
    }

    pub(crate) fn stage_index(&self) -> Arc<crate::stage::StageIndex> {
        if let Some(hit) = self
            .stage_cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
        {
            return Arc::clone(hit);
        }
        let built = Arc::new(crate::stage::StageIndex::build(self));
        let mut guard = self.stage_cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(hit) = guard.as_ref() {
            return Arc::clone(hit);
        }
        *guard = Some(Arc::clone(&built));
        built
    }

    fn clear_stage_cache(&self) {
        *self.stage_cache.lock().unwrap_or_else(|e| e.into_inner()) = None;
        if let Ok(mut graphs) = self.graph_cache.lock() {
            graphs.clear();
        }
    }

    pub fn set_lint(&mut self, items: Vec<LintItem>) {
        self.lint = Some(items);
        self.reanalyze();
    }

    /// Re-read the given paths (modified, created or deleted) and re-run the analysis.
    /// Returns true when anything changed.
    pub fn refresh_paths(&mut self, changed: &[PathBuf]) -> bool {
        let touched = self.ingest_paths(changed);
        if touched {
            self.reanalyze();
        }
        touched
    }

    /// Re-read paths without building analysis. The caller runs `fork_analysis`
    /// after dropping its lock.
    pub fn ingest_paths(&mut self, changed: &[PathBuf]) -> bool {
        if changed.iter().any(|p| is_archive_path(p)) {
            return self.ingest_archives();
        }
        let mut touched = false;
        for p in changed {
            if !is_project_script(&self.game_dir, p) {
                continue;
            }
            let rel = rel_string(&self.game_dir, p);
            let key = rel_key(&rel);
            let existing = self.files.iter().position(|f| rel_key(&f.rel) == key);
            if p.is_file() {
                let mut file = load_file(p, &rel);
                if let Some(archive) = self.winning(&rel) {
                    file.origin = Origin::Override { archive };
                }
                match existing {
                    Some(i) => self.files[i] = file,
                    None => self.files.push(file),
                }
                touched = true;
            } else if let Some(i) = existing {
                if let Some(file) = self.archived_source(&rel) {
                    self.files[i] = file;
                } else if let Some(file) = self.compiled_source(&rel) {
                    self.files[i] = file;
                } else {
                    self.files.remove(i);
                }
                touched = true;
            }
        }
        if touched {
            sort_source_files(&mut self.files);
            self.rebuild_file_index();
        }
        touched
    }

    /// Re-read every archive and rebuild the scripts that come from them.
    /// Loose files stay as they are. Returns true (the map is always rebuilt).
    pub fn refresh_archives(&mut self) -> bool {
        self.ingest_archives();
        self.reanalyze();
        true
    }

    fn ingest_archives(&mut self) -> bool {
        let (loose_compiled, archive_paths, compiled_fp) = scan_compiled(&self.game_dir);
        let mut loaded = open_archives(&self.game_dir, &archive_paths);
        sort_archives(&mut loaded);
        self.files.retain(|f| f.abs.is_file());
        for file in &mut self.files {
            file.origin = match winning_archive(&loaded, &file.rel) {
                Some(archive) => Origin::Override { archive },
                None => Origin::Loose,
            };
            file.source = None;
        }
        let cache = self.cache_dir.clone();
        let loose_jobs = loose_compiled_jobs(&self.game_dir, &loose_compiled);
        let (decompiled, failed) = recover_compiled(&self.game_dir, &loose_jobs, cache.as_deref());
        self.files.extend(decompiled);
        let (archived, extra_compiled, shadowed) =
            take_archived_scripts(&self.game_dir, &loaded, &self.files, cache.as_deref());
        self.files.extend(archived);
        sort_source_files(&mut self.files);
        let mut compiled_only = failed;
        compiled_only.extend(extra_compiled);
        compiled_only.sort();
        compiled_only.dedup();
        fill_archive_stats(&mut loaded, &self.files);
        self.compiled_only = compiled_only;
        self.shadowed = shadowed;
        let stale = stale_entries(&self.game_dir, &loaded);
        apply_stale(&mut loaded, &stale);
        self.stale = stale;
        self.archives = loaded;
        self.compiled_fp = compiled_fp;
        self.has_archives = !archive_paths.is_empty();
        self.rebuild_file_index();
        true
    }

    fn winning(&self, rel: &str) -> Option<String> {
        winning_archive(&self.archives, rel)
    }

    fn compiled_source(&self, rel: &str) -> Option<SourceFile> {
        let rpyc = if let Some(stem) = rel.strip_suffix(".rpy") {
            format!("{stem}.rpyc")
        } else if let Some(stem) = rel.strip_suffix(".rpym") {
            format!("{stem}.rpymc")
        } else {
            return None;
        };
        let loose = loose_path(&self.game_dir, &rpyc);
        let (bytes, archive) = if loose.is_file() {
            (fs::read(&loose).ok()?, None)
        } else {
            let archive = self.winning(&rpyc)?;
            let loaded = self
                .archives
                .iter()
                .find(|a| a.info.path == archive)?
                .archive
                .as_ref()?;
            (
                loaded.read_entry(&rpyc, rpa::SCRIPT_MAX).ok()?,
                Some(archive),
            )
        };
        let job = CompiledJob {
            rpyc_rel: rpyc,
            rpy_rel: rel.to_string(),
            bytes,
            archive,
        };
        let got = cached_decompile(&job.bytes, self.cache_dir.as_deref()).ok()?;
        Some(source_from_decompiled(&self.game_dir, &job, &got))
    }

    fn archived_source(&self, rel: &str) -> Option<SourceFile> {
        let archive = self.winning(rel)?;
        read_archived_script(&self.game_dir, &self.archives, &archive, rel)
    }

    /// Bytes of a script, from the loose file or from its archive.
    pub fn read_script_bytes(&self, rel: &str) -> Result<Vec<u8>, String> {
        let file = self
            .files
            .iter()
            .find(|f| f.rel == rel)
            .ok_or_else(|| format!("`{rel}` is not a script of this project."))?;
        if let Some(text) = &file.source {
            return Ok(text.as_bytes().to_vec());
        }
        match &file.origin {
            Origin::Archived { archive } => {
                let loaded = self
                    .archives
                    .iter()
                    .find(|a| a.info.path == *archive)
                    .and_then(|a| a.archive.as_ref())
                    .ok_or_else(|| format!("Archive `{archive}` is not open."))?;
                loaded
                    .read_entry(rel, rpa::SCRIPT_MAX)
                    .map_err(|e| e.to_string())
            }
            _ => fs::read(&file.abs)
                .map_err(|e| format!("Could not read {}: {e}", file.abs.display())),
        }
    }

    /// CRC32 of an entry in an archive that is already open, if it is still there.
    pub fn archived_crc(&self, archive: &str, name: &str) -> Option<u32> {
        let loaded = self
            .archives
            .iter()
            .find(|a| a.info.path == archive)?
            .archive
            .as_ref()?;
        loaded
            .copy_entry(name, &mut std::io::sink())
            .ok()
            .map(|s| s.crc32)
    }

    pub fn file_index(&self, rel: &str) -> Option<usize> {
        self.file_of.get(&rel_key(rel)).copied()
    }

    /// Where Ren'Py's warp should look. Loose scripts use the editor line.
    /// A decompiled `.rpyc` maps that line back to the compiled filename and linenumber.
    pub fn engine_spec(&self, rel: &str, line: u32) -> (String, u32) {
        let Some(file) = self.files.iter().find(|f| f.rel == rel) else {
            return (crate::rpyc::game_script_rel(rel), line);
        };
        let Some((engine_file, lines)) = &file.engine else {
            return (file.rel.clone(), line);
        };
        if lines.is_empty() || line == 0 {
            return (engine_file.clone(), line);
        }
        let idx = (line as usize).saturating_sub(1).min(lines.len() - 1);
        let mapped = match lines[idx] {
            0 => lines.iter().copied().rev().find(|n| *n > 0).unwrap_or(line),
            n => n,
        };
        (engine_file.clone(), mapped)
    }

    /// Editor file and line for a position the running game reported.
    /// `None` when that file was not recovered from a `.rpyc`.
    pub fn ide_line(&self, engine_file: &str, line: u32) -> Option<(String, u32)> {
        let want = crate::rpyc::game_script_rel(engine_file);
        if want.is_empty() || line == 0 {
            return None;
        }
        for file in &self.files {
            let Some((name, lines)) = &file.engine else {
                continue;
            };
            if name != &want && file.rel != want {
                continue;
            }
            let mut best_i = None;
            let mut best_eng = 0u32;
            for (i, &eng) in lines.iter().enumerate() {
                if eng == 0 || eng > line {
                    continue;
                }
                if best_i.is_none() || eng > best_eng {
                    best_eng = eng;
                    best_i = Some(i);
                }
            }
            let ide = best_i.map(|i| i as u32 + 1)?;
            return Some((file.rel.clone(), ide));
        }
        None
    }

    pub fn display_name(&self) -> String {
        for f in &self.files {
            for (k, v) in &f.meta.config {
                if k == "name" {
                    if let Some(start) = v.find(['"', '\'']) {
                        if let Some((s, _)) = crate::parser::parse_string_at(&v[start..]) {
                            if !s.is_empty() {
                                return s;
                            }
                        }
                    }
                }
            }
        }
        self.root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Ren'Py project".into())
    }

    pub fn config_value(&self, key: &str) -> Option<String> {
        self.files
            .iter()
            .flat_map(|f| f.meta.config.iter())
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
    }

    pub fn game_info(&self) -> GameInfo {
        let (files_oldest, files_newest) = file_date_span(&self.game_dir);
        let mut format_counts: std::collections::BTreeMap<String, u32> =
            std::collections::BTreeMap::new();
        let mut nested = 0u32;
        let mut archives = Vec::new();
        for arch in &self.archives {
            let label = if arch.info.version.is_empty() {
                "unreadable".to_string()
            } else {
                arch.info.version.clone()
            };
            *format_counts.entry(label).or_default() += 1;
            if arch.info.nested {
                nested += 1;
                continue;
            }
            let abs = loose_path(&self.game_dir, &arch.info.path);
            let meta = fs::metadata(&abs).ok();
            archives.push(ArchiveDigest {
                path: arch.info.path.clone(),
                bytes: meta.as_ref().map(|m| m.len()).unwrap_or(0),
                modified: meta.and_then(|m| m.modified().ok()).and_then(format_system_time),
                sha256: None,
            });
        }
        let mut loose = 0u32;
        let mut overrides = 0u32;
        let mut archived = 0u32;
        let mut compiled = 0u32;
        for file in &self.files {
            match file.origin.kind() {
                "loose" => loose += 1,
                "override" => overrides += 1,
                "archived" => archived += 1,
                "compiled" => compiled += 1,
                _ => {}
            }
        }
        let mut notes = Vec::new();
        if version_at_least(self.engine_version.as_deref(), 8, 5) {
            notes.push(
                "Ren'Py 8.5 and later leave the --json-dump label list empty, so the label cross-check is unavailable."
                    .into(),
            );
        }
        GameInfo {
            engine_version: self.engine_version.clone(),
            script_version: self.script_version.clone(),
            name: self.config_value("name").map(|v| plain_config(&v)),
            version: self.config_value("version").map(|v| plain_config(&v)),
            build_name: self.config_value("build.name").map(|v| plain_config(&v)),
            save_directory: self.config_value("save_directory").map(|v| plain_config(&v)),
            files_oldest,
            files_newest,
            layout: GameLayout {
                loose_scripts: loose,
                override_scripts: overrides,
                archived_scripts: archived,
                compiled_scripts: compiled,
                compiled_only: self.compiled_only.len() as u32,
                archives: self.archives.len() as u32,
                nested_archives: nested,
                formats: format_counts
                    .into_iter()
                    .map(|(version, count)| FormatCount { version, count })
                    .collect(),
            },
            archives,
            notes,
        }
    }

    pub fn info(&self) -> ProjectInfo {
        let mut label_counts = vec![0u32; self.files.len()];
        for d in &self.analysis.defs {
            if d.primary && d.kind == "label" {
                label_counts[d.file] += 1;
            }
        }
        ProjectInfo {
            root: self.root.to_string_lossy().into_owned(),
            game_dir: self.game_dir.to_string_lossy().into_owned(),
            name: self.display_name(),
            engine_version: self.engine_version.clone(),
            script_version: self
                .script_version
                .clone()
                .filter(|s| Some(s) != self.engine_version.as_ref()),
            launcher: self
                .launcher
                .as_ref()
                .map(|l| l.exe.to_string_lossy().into_owned()),
            bundled_engine: self
                .launcher
                .as_ref()
                .is_some_and(|l| l.prefix_args.is_empty()),
            developer: self.config_value("developer"),
            has_archives: self.has_archives,
            compiled_only: self.compiled_only.clone(),
            archives: self.archives.iter().map(|a| a.info.clone()).collect(),
            files: self
                .files
                .iter()
                .enumerate()
                .map(|(i, f)| FileInfo {
                    path: f.rel.clone(),
                    lines: f.total_lines,
                    bytes: f.bytes,
                    opaque: f.opaque,
                    labels: label_counts[i],
                    issues: f.issues.len() as u32,
                    origin: f.origin.kind().to_string(),
                    archive: f.origin.archive().map(|s| s.to_string()),
                    editable: f.origin.editable(),
                    decompiled: matches!(f.origin, Origin::Compiled { .. }),
                    reasons: f.decompile_reasons.clone(),
                })
                .collect(),
            stats: self.analysis.stats.clone(),
            parse_ms: self.parse_ms as u64,
            engine: self.engine.as_ref().map(|e| e.summary(&self.source_key())),
            stage_images: self
                .stage_images
                .as_ref()
                .map(|r| r.summary(&self.init_key())),
            lint_count: self.lint.as_ref().map(|l| l.len() as u32),
            game: self.game_info(),
        }
    }

    /// A cached label graph, when one was built since the last analysis.
    pub fn cached_label_graph(&self, name: &str, detail: bool) -> Option<crate::flow::LabelGraph> {
        if !self.analysis_is_current() {
            return None;
        }
        let cache = self.graph_cache.lock().ok()?;
        cache.get(&(name.to_string(), detail)).cloned()
    }

    /// Inputs for building one label graph after the project lock is dropped.
    pub fn graph_request(&self, name: &str, detail: bool) -> Option<GraphRequest> {
        let (fi, line) = self.label_site(name)?;
        let file = self.files.get(fi)?.clone();
        Some(GraphRequest {
            file,
            line,
            detail,
            screens: self.analysis.screens.clone(),
            name: name.to_string(),
            installed_epoch: self.installed_epoch,
            fresh: self.analysis_is_current(),
        })
    }

    /// Cache a graph built from `request`, unless analysis moved on meanwhile.
    pub fn store_label_graph(&self, request: &GraphRequest, graph: &crate::flow::LabelGraph) {
        if !request.fresh
            || !self.analysis_is_current()
            || self.installed_epoch != request.installed_epoch
        {
            return;
        }
        if let Ok(mut cache) = self.graph_cache.lock() {
            cache.insert((request.name.clone(), request.detail), graph.clone());
        }
    }

    /// True when `analysis` was built from the current `files`.
    pub fn analysis_is_current(&self) -> bool {
        self.installed_epoch == self.analysis_epoch
    }

    /// File index and line of a label or named menu. While a forked run is
    /// still going, `analysis.defs` points into an older file list, so the
    /// current files are searched instead.
    fn label_site(&self, name: &str) -> Option<(usize, u32)> {
        if self.analysis_is_current() {
            let idx = *self.analysis.by_name.get(name)?;
            let def = self.analysis.defs.get(idx)?;
            return def.primary.then_some((def.file, def.line));
        }
        for (fi, file) in self.files.iter().enumerate() {
            let mut line = None;
            crate::flow::visit_labels(&file.stmts, None, &mut |s, _| {
                let named = match &s.kind {
                    Kind::Label { name: n, .. } => n == name,
                    Kind::Menu { name: Some(n), .. } => n == name,
                    _ => false,
                };
                if line.is_none() && named {
                    line = Some(s.line);
                }
            });
            if let Some(line) = line {
                return Some((fi, line));
            }
        }
        None
    }

    /// Build the detail graph for a label/menu by name.
    pub fn label_graph(&self, name: &str, detail: bool) -> Option<crate::flow::LabelGraph> {
        if let Some(hit) = self.cached_label_graph(name, detail) {
            return Some(hit);
        }
        let request = self.graph_request(name, detail)?;
        let graph = request.build()?;
        self.store_label_graph(&request, &graph);
        Some(graph)
    }

    /// Says, menus, and choices in one label, in source order.
    pub fn label_lines(&self, name: &str) -> Option<crate::flow::LabelLines> {
        let (fi, line) = self.label_site(name)?;
        let file = self.files.get(fi)?;
        let mut found = None;
        crate::flow::visit_labels(&file.stmts, None, &mut |s, c| {
            if found.is_none() && s.line == line && s.is_label_like() {
                found = Some(crate::flow::collect_lines(s, c));
            }
        });
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_spec_round_trips_a_decompiled_line() {
        let root = std::env::temp_dir().join(format!("vnide-engine-spec-{}", std::process::id()));
        let game = root.join("game");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&game).unwrap();
        fs::write(game.join("script.rpy"), "label start:\n    \"hi\"\n").unwrap();
        let mut project = Project::open(&root).unwrap();
        assert_eq!(
            project.engine_spec("script.rpy", 2),
            ("script.rpy".into(), 2)
        );
        assert!(project.ide_line("script.rpy", 2).is_none());
        project.files[0].engine = Some(("script.rpy".into(), vec![10, 10, 12, 12]));
        assert_eq!(
            project.engine_spec("script.rpy", 1),
            ("script.rpy".into(), 10)
        );
        assert_eq!(
            project.engine_spec("script.rpy", 3),
            ("script.rpy".into(), 12)
        );
        assert_eq!(
            project
                .ide_line("game/script.rpy", 12)
                .as_ref()
                .map(|(f, l)| (f.as_str(), *l)),
            Some(("script.rpy", 3))
        );
        assert_eq!(
            project
                .ide_line(r"C:\proj\game\script.rpy", 10)
                .as_ref()
                .map(|(f, l)| (f.as_str(), *l)),
            Some(("script.rpy", 1))
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn init_key_ignores_dialogue_and_tracks_init_and_images() {
        let root = std::env::temp_dir().join(format!("vnide-init-key-{}", std::process::id()));
        let game = root.join("game");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&game).unwrap();
        let script = game.join("script.rpy");
        let write = |text: &str| fs::write(&script, text).unwrap();
        write("label start:\n    \"hello\"\nimage eileen = \"e.png\"\ninit python:\n    x = 1\n");
        let key = Project::open(&root).unwrap().init_key();
        write("label start:\n    \"world\"\nimage eileen = \"e.png\"\ninit python:\n    x = 1\n");
        assert_eq!(Project::open(&root).unwrap().init_key(), key, "dialogue");
        write("# a comment\nlabel begin:\n    \"world\"\nimage eileen = \"e.png\"\ninit python:\n    x = 1\n");
        assert_eq!(
            Project::open(&root).unwrap().init_key(),
            key,
            "comment and same-size label"
        );
        write("# a comment\nlabel begin:\n    \"world\"\nimage eileen = \"e.png\"\ninit python:\n    x = 2\n");
        let edited = Project::open(&root).unwrap().init_key();
        assert_ne!(edited, key, "init python");
        write("# a comment\nlabel begin:\n    \"world\"\nimage eileen = \"f.png\"\ninit python:\n    x = 2\n");
        let image = Project::open(&root).unwrap().init_key();
        assert_ne!(image, edited, "image statement");
        fs::create_dir_all(game.join("images")).unwrap();
        fs::write(game.join("images").join("e.png"), b"png").unwrap();
        assert_ne!(
            Project::open(&root).unwrap().init_key(),
            image,
            "image file"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn source_key_changes_when_an_edit_keeps_the_same_length() {
        let root = std::env::temp_dir().join(format!("vnide-source-key-{}", std::process::id()));
        let game = root.join("game");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&game).unwrap();
        let script = game.join("script.rpy");
        fs::write(&script, "label start:\n    \"hello\"\n").unwrap();
        let before = Project::open(&root).unwrap().source_key();
        fs::write(&script, "label start:\n    \"world\"\n").unwrap();
        let after = Project::open(&root).unwrap().source_key();
        assert_ne!(before, after);
        assert_eq!(
            fs::metadata(&script).unwrap().len(),
            "label start:\n    \"hello\"\n".len() as u64
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn scripts_sort_shallower_paths_first() {
        let root = std::env::temp_dir().join(format!("vnide-sort-{}", std::process::id()));
        let game = root.join("game");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(game.join("chapter")).unwrap();
        fs::write(game.join("z.rpy"), "label z:\n    return\n").unwrap();
        fs::write(game.join("a.rpy"), "label a:\n    return\n").unwrap();
        fs::write(game.join("chapter").join("b.rpy"), "label b:\n    return\n").unwrap();
        let project = Project::open(&root).unwrap();
        let rels: Vec<_> = project.files.iter().map(|f| f.rel.as_str()).collect();
        assert_eq!(rels, ["a.rpy", "z.rpy", "chapter/b.rpy"]);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_stale_analysis_does_not_replace_a_newer_one() {
        let root = std::env::temp_dir().join(format!("vnide-epoch-{}", std::process::id()));
        let game = root.join("game");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&game).unwrap();
        fs::write(game.join("script.rpy"), "label start:\n    return\n").unwrap();
        let mut project = Project::open(&root).unwrap();
        let first = project.fork_analysis();
        let second = project.fork_analysis();
        let old = first.run();
        let new = second.run();
        assert!(!project.publish_analysis(first.epoch, old));
        assert!(project.publish_analysis(second.epoch, new));
        assert!(project.analysis.by_name.contains_key("start"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn game_info_reads_config_layout_and_warns_on_85() {
        let root = std::env::temp_dir().join(format!("vnide-game-info-{}", std::process::id()));
        let game = root.join("game");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("renpy")).unwrap();
        fs::create_dir_all(&game).unwrap();
        fs::write(
            root.join("renpy").join("vc_version.py"),
            "version = '8.5.2'\n",
        )
        .unwrap();
        fs::write(game.join("script_version.txt"), "(8, 4, 1)\n").unwrap();
        fs::write(
            game.join("options.rpy"),
            "define config.name = _(\"Demo\")\ndefine config.version = \"1.2\"\ndefine config.save_directory = \"Demo-1\"\ndefine build.name = \"demo-pkg\"\n",
        )
        .unwrap();
        fs::write(game.join("script.rpy"), "label start:\n    return\n").unwrap();
        fs::write(game.join("scripts.rpa"), b"not-an-archive").unwrap();
        let info = Project::open(&root).unwrap().game_info();
        assert_eq!(info.engine_version.as_deref(), Some("8.5.2"));
        assert_eq!(info.script_version.as_deref(), Some("8.4.1"));
        assert_eq!(info.name.as_deref(), Some("Demo"));
        assert_eq!(info.version.as_deref(), Some("1.2"));
        assert_eq!(info.build_name.as_deref(), Some("demo-pkg"));
        assert_eq!(info.save_directory.as_deref(), Some("Demo-1"));
        assert_eq!(info.layout.loose_scripts, 2);
        assert_eq!(info.layout.archives, 1);
        assert_eq!(info.archives.len(), 1);
        assert!(info.archives[0].sha256.is_none());
        assert!(info.files_newest.is_some());
        assert!(info.notes.iter().any(|n| n.contains("8.5")));
        fs::write(game.join("odd.rpa"), b"XYZ-9.9 nope\n").unwrap();
        fs::write(
            game.join("script.rpy"),
            "label start:\n    return\ninit python:\n    config.archive_handlers.append(None)\n",
        )
        .unwrap();
        let again = Project::open(&root).unwrap();
        let odd = again
            .archives
            .iter()
            .find(|a| a.info.path == "odd.rpa")
            .unwrap();
        let err = odd.info.error.as_deref().unwrap_or("");
        assert!(err.contains("unsupported"), "{err}");
        assert!(err.contains("script.rpy"), "{err}");
        assert_eq!(odd.info.version, "XYZ-9.9");
        assert!(!version_at_least(Some("8.4.1"), 8, 5));
        assert!(version_at_least(Some("8.5.0"), 8, 5));
        let hashed = sha256_file(&game.join("scripts.rpa")).unwrap();
        assert_eq!(hashed.len(), 64);
        let _ = fs::remove_dir_all(&root);
    }
}
