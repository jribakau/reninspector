//! Application event log.
//!
//! One ordered stream for the process: `log` macros, panics, and lines the UI
//! sends in. Entries stay in a ring buffer, are appended to `logs/app.log`,
//! and are emitted to the window as `applog:entry`.

use std::cell::Cell;
use std::collections::VecDeque;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, Once, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

const CAP: usize = 2000;
const ROTATE_AT: u64 = 1024 * 1024;
const EVENT: &str = "applog:entry";

static STATE: Mutex<LogState> = Mutex::new(LogState {
    entries: VecDeque::new(),
    next_seq: 1,
    path: None,
    pending: VecDeque::new(),
});
static APP: OnceLock<AppHandle> = OnceLock::new();
static LOGGER: Logger = Logger;
/// Panics that happen while the log lock is held, drained on the next record.
static PANIC_SIDE: Mutex<Vec<String>> = Mutex::new(Vec::new());

thread_local! {
    static BUSY: Cell<bool> = const { Cell::new(false) };
    static HOLDING: Cell<bool> = const { Cell::new(false) };
}

struct LogState {
    entries: VecDeque<Entry>,
    next_seq: u64,
    path: Option<PathBuf>,
    pending: VecDeque<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub seq: u64,
    pub ts_ms: u64,
    pub level: String,
    pub source: String,
    pub message: String,
}

struct Logger;

const OURS: &[&str] = &[
    "app", "project", "git", "sdk", "live", "watch", "ui", "panic", "edit", "patch", "pylsp",
];

fn is_ours(target: &str) -> bool {
    target == "ty"
        || target.starts_with("vn_ide_lib")
        || target.starts_with("renpy_core")
        || OURS.contains(&target)
}

fn accepts(target: &str, level: log::Level) -> bool {
    if is_ours(target) {
        level <= log::Level::Info
    } else {
        level <= log::Level::Warn
    }
}

fn source_of(target: &str) -> String {
    let rest = target
        .strip_prefix("vn_ide_lib::")
        .or_else(|| target.strip_prefix("renpy_core::"))
        .unwrap_or(target);
    let head = rest.split("::").next().unwrap_or("app");
    clean_source(head)
}

fn clean_source(source: &str) -> String {
    let mut out = String::new();
    for c in source.chars() {
        if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
            out.push(c);
        }
        if out.len() >= 16 {
            break;
        }
    }
    if out.is_empty() {
        "app".to_string()
    } else {
        out
    }
}

fn clean_message(message: &str) -> String {
    let trimmed = message.trim();
    let count = trimmed.chars().count();
    if count <= 4000 {
        return trimmed.to_string();
    }
    trimmed.chars().take(4000).collect()
}

fn level_name(level: log::Level) -> &'static str {
    match level {
        log::Level::Error => "error",
        log::Level::Warn => "warn",
        _ => "info",
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// `days` is the Unix day number (1970-01-01 is 0). Civil date, UTC.
fn ymd_from_epoch_days(days: i64) -> (i32, u32, u32) {
    let days = days + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let doe = (days - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m as u32, d as u32)
}

fn format_ts(ts_ms: u64) -> String {
    let secs = ts_ms / 1000;
    let days = (secs / 86_400) as i64;
    let tod = secs % 86_400;
    let (y, m, d) = ymd_from_epoch_days(days);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}",
        tod / 3600,
        (tod % 3600) / 60,
        tod % 60
    )
}

fn file_line(entry: &Entry) -> String {
    let message = entry.message.replace(['\n', '\r'], " ");
    format!(
        "{} {:<5} {:<8} {message}",
        format_ts(entry.ts_ms),
        entry.level.to_ascii_uppercase(),
        entry.source
    )
}

fn panic_text(info: &std::panic::PanicHookInfo<'_>) -> String {
    let msg = if let Some(s) = info.payload().downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = info.payload().downcast_ref::<String>() {
        s.clone()
    } else {
        "panic".to_string()
    };
    let loc = info
        .location()
        .map(|l| format!("{}:{}", l.file(), l.line()))
        .unwrap_or_else(|| "unknown".to_string());
    let current = std::thread::current();
    let thread = current.name().unwrap_or("unnamed");
    format!("{msg} at {loc} ({thread})")
}

fn lock_state() -> std::sync::MutexGuard<'static, LogState> {
    STATE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn append_rotated(path: &Path, line: &str, limit: u64) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    if let Ok(meta) = fs::metadata(path) {
        if meta.len() >= limit {
            let old = path.with_file_name("app.log.1");
            let _ = fs::remove_file(&old);
            let _ = fs::rename(path, &old);
        }
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(line.as_bytes())?;
    file.write_all(b"\n")?;
    Ok(())
}

fn flush_pending(state: &mut LogState) {
    let Some(path) = state.path.clone() else {
        return;
    };
    while let Some(line) = state.pending.pop_front() {
        if append_rotated(&path, &line, ROTATE_AT).is_err() {
            state.pending.push_front(line);
            break;
        }
    }
}

fn drain_side(state: &mut LogState) -> Vec<Entry> {
    let side = match PANIC_SIDE.lock() {
        Ok(mut guard) => std::mem::take(&mut *guard),
        Err(poison) => std::mem::take(&mut *poison.into_inner()),
    };
    let mut made = Vec::new();
    for message in side {
        if let Some(entry) = push(state, "error", "panic", &message) {
            made.push(entry);
        }
    }
    made
}

fn push(state: &mut LogState, level: &str, source: &str, message: &str) -> Option<Entry> {
    let message = clean_message(message);
    if message.is_empty() {
        return None;
    }
    let entry = Entry {
        seq: state.next_seq,
        ts_ms: now_ms(),
        level: level.to_string(),
        source: clean_source(source),
        message,
    };
    state.next_seq = state.next_seq.saturating_add(1);
    state.pending.push_back(file_line(&entry));
    if state.pending.len() > CAP {
        state.pending.pop_front();
    }
    state.entries.push_back(entry.clone());
    if state.entries.len() > CAP {
        state.entries.pop_front();
    }
    Some(entry)
}

fn record_entry(level: &str, source: &str, message: &str) {
    if HOLDING.with(|h| h.get()) {
        if let Ok(mut side) = PANIC_SIDE.lock() {
            side.push(clean_message(message));
        }
        return;
    }
    if BUSY.with(|b| b.replace(true)) {
        return;
    }
    struct Guard;
    impl Drop for Guard {
        fn drop(&mut self) {
            BUSY.with(|b| b.set(false));
        }
    }
    let _guard = Guard;

    let made = {
        struct Hold;
        impl Drop for Hold {
            fn drop(&mut self) {
                HOLDING.with(|h| h.set(false));
            }
        }
        HOLDING.with(|h| h.set(true));
        let _hold = Hold;
        let mut state = lock_state();
        let mut made = drain_side(&mut state);
        if let Some(entry) = push(&mut state, level, source, message) {
            made.push(entry);
        }
        flush_pending(&mut state);
        made
    };
    if let Some(app) = APP.get() {
        for entry in made {
            let _ = app.emit(EVENT, &entry);
        }
    }
}

impl log::Log for Logger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        accepts(metadata.target(), metadata.level())
    }

    fn log(&self, record: &log::Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        record_entry(
            level_name(record.level()),
            &source_of(record.target()),
            &record.args().to_string(),
        );
    }

    fn flush(&self) {}
}

/// Install the global logger and a panic hook. Safe to call more than once.
pub fn install() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let _ = log::set_logger(&LOGGER);
        log::set_max_level(log::LevelFilter::Info);
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            record_entry("error", "panic", &panic_text(info));
            prev(info);
        }));
    });
}

/// Open `logs/app.log` under the app data folder and emit later entries.
pub fn attach(app: &AppHandle) {
    if APP.set(app.clone()).is_err() {
        return;
    }
    if let Ok(dir) = crate::util::data_dir(app) {
        let mut state = lock_state();
        state.path = Some(dir.join("logs").join("app.log"));
        flush_pending(&mut state);
    }
    log::info!(
        target: "app",
        "Ren'Inspector {} started on {} {}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH
    );
}

/// Last `max_lines` of the log file, or of the ring buffer when the file is not open yet.
pub fn recent_text(max_lines: usize) -> String {
    let state = lock_state();
    if let Some(path) = &state.path {
        if let Some(text) = tail_lines(path, max_lines) {
            return text;
        }
    }
    let skip = state.entries.len().saturating_sub(max_lines);
    state
        .entries
        .iter()
        .skip(skip)
        .map(file_line)
        .collect::<Vec<_>>()
        .join("\n")
}

fn tail_lines(path: &Path, max_lines: usize) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    let lines: Vec<&str> = text.lines().rev().take(max_lines).collect();
    if lines.is_empty() {
        return None;
    }
    let mut out = String::new();
    for line in lines.into_iter().rev() {
        out.push_str(line);
        out.push('\n');
    }
    Some(out)
}

#[tauri::command]
pub fn applog_read() -> Vec<Entry> {
    lock_state().entries.iter().cloned().collect()
}

#[tauri::command]
pub fn applog_write(level: String, source: String, message: String) {
    let level = match level.as_str() {
        "error" => "error",
        "warn" => "warn",
        _ => "info",
    };
    record_entry(level, &source, &message);
}

#[tauri::command]
pub fn applog_clear() {
    let mut state = lock_state();
    state.entries.clear();
}

#[tauri::command]
pub fn applog_path() -> Option<String> {
    lock_state()
        .path
        .as_ref()
        .and_then(|path| path.parent())
        .map(|dir| dir.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    static TEST: Mutex<()> = Mutex::new(());

    fn reset() {
        let mut state = lock_state();
        state.entries.clear();
        state.pending.clear();
        state.path = None;
    }

    #[test]
    fn ring_drops_the_oldest() {
        let _guard = TEST.lock().unwrap();
        reset();
        let start = lock_state().next_seq;
        for i in 0..CAP + 5 {
            record_entry("info", "app", &format!("n{i}"));
        }
        let rows = lock_state().entries.iter().cloned().collect::<Vec<_>>();
        assert_eq!(rows.len(), CAP);
        assert_eq!(rows[0].message, "n5");
        assert_eq!(rows[0].seq, start + 5);
        assert_eq!(rows.last().unwrap().message, format!("n{}", CAP + 4));
    }

    #[test]
    fn rotates_the_log_file_past_the_limit() {
        let dir =
            std::env::temp_dir().join(format!("vnide-applog-{}-{}", std::process::id(), now_ms()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("app.log");
        append_rotated(&path, "one", 10).unwrap();
        append_rotated(&path, "two", 10).unwrap();
        append_rotated(&path, "three", 10).unwrap();
        append_rotated(&path, "four", 10).unwrap();
        let current = fs::read_to_string(&path).unwrap();
        let old = fs::read_to_string(path.with_file_name("app.log.1")).unwrap();
        assert!(current.contains("four"), "{current}");
        assert!(!current.contains("one"), "{current}");
        assert!(old.contains("one"), "{old}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn filters_by_level_and_target() {
        assert!(accepts("vn_ide_lib::git", log::Level::Info));
        assert!(accepts("renpy_core::parser", log::Level::Warn));
        assert!(accepts("ty", log::Level::Info));
        assert!(accepts("project", log::Level::Info));
        assert!(!accepts("vn_ide_lib::git", log::Level::Debug));
        assert!(!accepts("wry::webview", log::Level::Info));
        assert!(accepts("wry::webview", log::Level::Warn));
        assert!(accepts("tao::event", log::Level::Error));
    }

    #[test]
    fn formats_utc_timestamps() {
        assert_eq!(format_ts(0), "1970-01-01 00:00:00");
        assert_eq!(format_ts(1_791_310_323_000), "2026-10-06 18:12:03");
    }

    #[test]
    fn panic_is_recorded_with_its_location() {
        let _guard = TEST.lock().unwrap();
        reset();
        struct Restore(Option<Box<dyn Fn(&std::panic::PanicHookInfo<'_>) + Send + Sync>>);
        impl Drop for Restore {
            fn drop(&mut self) {
                if let Some(prev) = self.0.take() {
                    std::panic::set_hook(prev);
                }
            }
        }
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(|info| {
            record_entry("error", "panic", &panic_text(info));
        }));
        let _restore = Restore(Some(prev));
        let caught = std::panic::catch_unwind(|| panic!("sliced through a character"));
        assert!(caught.is_err());
        let rows = lock_state().entries.iter().cloned().collect::<Vec<_>>();
        let hit = rows
            .iter()
            .find(|e| e.message.contains("sliced through a character"))
            .expect("panic entry");
        assert_eq!(hit.level, "error");
        assert_eq!(hit.source, "panic");
        assert!(hit.message.contains("applog.rs"), "{}", hit.message);
        assert!(hit.message.contains('('), "{}", hit.message);
    }
}
