//! Read Ren'Py save slots. Nothing here writes a save, and unpickling never
//! imports or calls the classes stored in the file.

use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use flate2::read::ZlibDecoder;
use serde::Serialize;
use zip::ZipArchive;

use crate::pickle::{load, Policy, Value};
use crate::rpa::format_utc;

const META_MAX: u64 = 1_048_576;
const IMAGE_MAX: u64 = 4 * 1024 * 1024;
const PICKLE_MAX: u64 = 32 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveSlot {
    pub path: String,
    pub name: String,
    pub extra: String,
    pub version: String,
    pub modified: Option<String>,
    pub persistent: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveNode {
    pub name: String,
    pub kind: String,
    pub repr: String,
    pub children: Vec<SaveNode>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveDetail {
    pub slot: SaveSlot,
    pub json: String,
    pub screenshot_base64: Option<String>,
    pub tree: Option<SaveNode>,
    pub note: Option<String>,
}

/// `game/saves` plus the per-user folder Ren'Py uses for `config.save_directory`.
pub fn save_roots(game_dir: &Path, save_directory: Option<&str>) -> Vec<PathBuf> {
    let mut roots = vec![game_dir.join("saves")];
    let Some(name) = save_directory else {
        return roots;
    };
    if name.is_empty()
        || name.contains("..")
        || name.contains('/')
        || name.contains('\\')
        || name.contains('\0')
    {
        return roots;
    }
    if let Ok(appdata) = std::env::var("APPDATA") {
        roots.push(PathBuf::from(appdata).join("RenPy").join(name));
    }
    if let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) {
        let home = PathBuf::from(home);
        roots.push(home.join(".renpy").join(name));
        roots.push(home.join("Library").join("RenPy").join(name));
    }
    roots
}

pub fn list_saves(game_dir: &Path, save_directory: Option<&str>) -> Vec<SaveSlot> {
    let mut slots = Vec::new();
    for root in save_roots(game_dir, save_directory) {
        walk(&root, 0, &mut slots);
    }
    slots.sort_by(|a, b| a.path.cmp(&b.path));
    slots
}

pub fn within_roots(path: &Path, roots: &[PathBuf]) -> bool {
    let Ok(path) = path.canonicalize() else {
        return false;
    };
    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
    if !name.ends_with(".save") && name != "persistent" {
        return false;
    }
    roots.iter().any(|root| {
        root.canonicalize()
            .ok()
            .is_some_and(|root| path.starts_with(root))
    })
}

pub fn inspect_save(path: &Path, deep: bool) -> Result<SaveDetail, String> {
    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("save");
    if name == "persistent" {
        return inspect_persistent(path, deep);
    }
    if !name.ends_with(".save") {
        return Err("That is not a Ren'Py save.".into());
    }
    let file = File::open(path).map_err(|e| format!("Could not open {}: {e}", path.display()))?;
    let mut zip = ZipArchive::new(file).map_err(|e| format!("Could not read the save: {e}"))?;
    let json = named(&mut zip, "json")
        .map(|b| String::from_utf8_lossy(&b).to_string())
        .unwrap_or_default();
    let extra = named(&mut zip, "extra_info")
        .map(|b| String::from_utf8_lossy(&b).trim().to_string())
        .unwrap_or_default();
    let version = named(&mut zip, "renpy_version")
        .map(|b| String::from_utf8_lossy(&b).trim().to_string())
        .unwrap_or_default();
    let screenshot = named(&mut zip, "screenshot.png").map(|b| base64(&b));
    let (tree, note) = if deep {
        match named(&mut zip, "log") {
            Some(bytes) => match load(&bytes, &Policy::SAVE) {
                Ok(value) => (Some(node_of("log", &value, 0)), None),
                Err(e) => (None, Some(format!("The save log could not be read: {e}"))),
            },
            None => (None, Some("This save has no log entry.".into())),
        }
    } else {
        (None, None)
    };
    Ok(SaveDetail {
        slot: SaveSlot {
            path: path.display().to_string(),
            name: save_name(&json, name),
            extra,
            version,
            modified: modified_stamp(path),
            persistent: false,
        },
        json,
        screenshot_base64: screenshot,
        tree,
        note,
    })
}

fn inspect_persistent(path: &Path, deep: bool) -> Result<SaveDetail, String> {
    let file = File::open(path).map_err(|e| format!("Could not read persistent data: {e}"))?;
    // Bound both the file and the inflated size so a crafted stream cannot fill memory.
    let mut decoded = Vec::new();
    ZlibDecoder::new(file.take(PICKLE_MAX))
        .take(PICKLE_MAX + 1)
        .read_to_end(&mut decoded)
        .map_err(|e| format!("Persistent data is not a zlib stream: {e}"))?;
    if decoded.len() as u64 > PICKLE_MAX {
        return Err("Persistent data is too large.".into());
    }
    let (tree, note) = if deep {
        match load(&decoded, &Policy::SAVE) {
            Ok(value) => (Some(node_of("persistent", &value, 0)), None),
            Err(e) => (
                None,
                Some(format!("Persistent data could not be read: {e}")),
            ),
        }
    } else {
        (None, None)
    };
    Ok(SaveDetail {
        slot: SaveSlot {
            path: path.display().to_string(),
            name: "persistent".into(),
            extra: String::new(),
            version: String::new(),
            modified: modified_stamp(path),
            persistent: true,
        },
        json: String::new(),
        screenshot_base64: None,
        tree,
        note,
    })
}

fn walk(dir: &Path, depth: u32, out: &mut Vec<SaveSlot>) {
    if depth > 4 {
        return;
    }
    let Ok(rd) = fs::read_dir(dir) else {
        return;
    };
    for entry in rd.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name == "cache" {
            continue;
        }
        if path.is_dir() {
            walk(&path, depth + 1, out);
            continue;
        }
        if name.ends_with(".save") {
            out.push(slot_zip(&path, &name));
        } else if depth == 0 && name == "persistent" {
            out.push(SaveSlot {
                path: path.display().to_string(),
                name: "persistent".into(),
                extra: String::new(),
                version: String::new(),
                modified: modified_stamp(&path),
                persistent: true,
            });
        }
    }
}

fn slot_zip(path: &Path, fallback: &str) -> SaveSlot {
    let opened = File::open(path)
        .ok()
        .and_then(|file| ZipArchive::new(file).ok());
    let (name, extra, version) = if let Some(mut zip) = opened {
        let json = named(&mut zip, "json")
            .map(|b| String::from_utf8_lossy(&b).to_string())
            .unwrap_or_default();
        let extra = named(&mut zip, "extra_info")
            .map(|b| String::from_utf8_lossy(&b).trim().to_string())
            .unwrap_or_default();
        let version = named(&mut zip, "renpy_version")
            .map(|b| String::from_utf8_lossy(&b).trim().to_string())
            .unwrap_or_default();
        (save_name(&json, fallback), extra, version)
    } else {
        (
            fallback.to_string(),
            "Could not read this save.".into(),
            String::new(),
        )
    };
    SaveSlot {
        path: path.display().to_string(),
        name,
        extra,
        version,
        modified: modified_stamp(path),
        persistent: false,
    }
}

fn named(zip: &mut ZipArchive<File>, want: &str) -> Option<Vec<u8>> {
    let mut found = None;
    for i in 0..zip.len() {
        let Ok(file) = zip.by_index(i) else {
            continue;
        };
        let raw = file.name().to_string();
        let name = raw.rsplit(['/', '\\']).next().unwrap_or("");
        if name.eq_ignore_ascii_case(want) {
            found = Some(i);
            break;
        }
    }
    let i = found?;
    let mut file = zip.by_index(i).ok()?;
    let cap = match want {
        "screenshot.png" => IMAGE_MAX,
        "log" | "persistent" => PICKLE_MAX,
        _ => META_MAX,
    };
    if file.size() > cap {
        return None;
    }
    // The declared size can lie; never read more than the cap.
    let mut buf = Vec::new();
    (&mut file).take(cap + 1).read_to_end(&mut buf).ok()?;
    if buf.len() as u64 > cap {
        return None;
    }
    Some(buf)
}

fn save_name(json: &str, fallback: &str) -> String {
    serde_json::from_str::<serde_json::Value>(json)
        .ok()
        .and_then(|v| {
            v.get("_save_name")
                .and_then(|n| n.as_str())
                .map(|s| s.to_string())
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| fallback.to_string())
}

fn modified_stamp(path: &Path) -> Option<String> {
    let modified = fs::metadata(path).ok()?.modified().ok()?;
    let secs = modified.duration_since(UNIX_EPOCH).ok()?.as_secs();
    Some(format_utc(secs))
}

fn node_of(name: &str, value: &Value, depth: usize) -> SaveNode {
    if depth > 8 {
        return SaveNode {
            name: name.into(),
            kind: "…".into(),
            repr: "nested too deep".into(),
            children: Vec::new(),
        };
    }
    match value {
        Value::None => leaf(name, "None", "None"),
        Value::Bool(v) => leaf(name, "bool", if *v { "True" } else { "False" }),
        Value::Int(v) => leaf(name, "int", &v.to_string()),
        Value::Float(v) => leaf(name, "float", &v.to_string()),
        Value::Str(v) => leaf(name, "str", &short(v)),
        Value::Bytes(v) => leaf(name, "bytes", &format!("{} bytes", v.len())),
        Value::List(items) => seq_node(name, "list", items, depth),
        Value::Tuple(items) => seq_node(name, "tuple", items, depth),
        Value::Set(items) => seq_node(name, "set", items, depth),
        Value::Dict(items) => kids(
            name,
            "dict",
            &format!("{} keys", items.len()),
            items
                .iter()
                .take(40)
                .map(|(k, v)| node_of(&short_value(k), v, depth + 1)),
        ),
        Value::Object { class, args, state } => {
            let mut children: Vec<SaveNode> = args
                .iter()
                .enumerate()
                .take(40)
                .map(|(i, v)| node_of(&format!("arg {i}"), v, depth + 1))
                .collect();
            if let Some(state) = state {
                children.push(node_of("state", state, depth + 1));
            }
            SaveNode {
                name: name.into(),
                kind: class.clone(),
                repr: format!("{} fields", children.len()),
                children,
            }
        }
    }
}

fn seq_node(name: &str, kind: &str, items: &[Value], depth: usize) -> SaveNode {
    kids(
        name,
        kind,
        &format!("{} items", items.len()),
        items
            .iter()
            .enumerate()
            .take(40)
            .map(|(i, v)| node_of(&i.to_string(), v, depth + 1)),
    )
}

fn kids(name: &str, kind: &str, repr: &str, children: impl Iterator<Item = SaveNode>) -> SaveNode {
    SaveNode {
        name: name.into(),
        kind: kind.into(),
        repr: repr.into(),
        children: children.collect(),
    }
}

fn leaf(name: &str, kind: &str, repr: &str) -> SaveNode {
    SaveNode {
        name: name.into(),
        kind: kind.into(),
        repr: short(repr),
        children: Vec::new(),
    }
}

fn short_value(value: &Value) -> String {
    match value {
        Value::Str(s) => short(s),
        Value::Int(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::None => "None".into(),
        other => format!("{other:?}"),
    }
}

fn short(text: &str) -> String {
    let mut out: String = text.chars().take(80).collect();
    if text.chars().count() > 80 {
        out.push('…');
    }
    out
}

fn base64(data: &[u8]) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
        if chunk.len() > 1 {
            out.push(T[((n >> 6) & 63) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(T[(n & 63) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    fn pickle_dict() -> Vec<u8> {
        let mut p = vec![0x80, 2, b'}', b'X'];
        p.extend(1u32.to_le_bytes());
        p.extend(b"a");
        p.push(b'K');
        p.push(1);
        p.push(b's');
        p.push(b'.');
        p
    }

    #[test]
    fn lists_a_slot_and_reads_its_metadata_without_unpickling() {
        let dir = std::env::temp_dir().join(format!("vn-ide-saves-{}", std::process::id()));
        let saves = dir.join("saves");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&saves).unwrap();
        let path = saves.join("1-1-LT1.save");
        let file = File::create(&path).unwrap();
        let mut zip = ZipWriter::new(file);
        let opts = SimpleFileOptions::default();
        zip.start_file("json", opts).unwrap();
        zip.write_all(br#"{"_save_name":"Morning"}"#).unwrap();
        zip.start_file("extra_info", opts).unwrap();
        zip.write_all(b"Chapter 1").unwrap();
        zip.start_file("renpy_version", opts).unwrap();
        zip.write_all(b"8.3.7").unwrap();
        zip.start_file("log", opts).unwrap();
        zip.write_all(&pickle_dict()).unwrap();
        zip.finish().unwrap();

        let slots = list_saves(&dir, None);
        assert_eq!(slots.len(), 1);
        assert_eq!(slots[0].name, "Morning");
        assert_eq!(slots[0].version, "8.3.7");
        let shallow = inspect_save(&path, false).unwrap();
        assert!(shallow.tree.is_none());
        let deep = inspect_save(&path, true).unwrap();
        let tree = deep.tree.unwrap();
        assert_eq!(tree.kind, "dict");
        assert!(tree.children.iter().any(|c| c.name == "a"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_truncated_save_is_an_error() {
        let dir = std::env::temp_dir().join(format!("vn-ide-saves-cut-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("broken.save");
        fs::write(&path, b"PK\x03\x04not-a-zip").unwrap();
        let err = inspect_save(&path, false).unwrap_err();
        assert!(err.contains("Could not read the save"), "{err}");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn persistent_unpickles_only_when_asked() {
        let dir = std::env::temp_dir().join(format!("vn-ide-persist-{}", std::process::id()));
        let saves = dir.join("saves");
        fs::create_dir_all(&saves).unwrap();
        let path = saves.join("persistent");
        let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(&pickle_dict()).unwrap();
        fs::write(&path, enc.finish().unwrap()).unwrap();
        let shallow = inspect_save(&path, false).unwrap();
        assert!(shallow.slot.persistent);
        assert!(shallow.tree.is_none());
        let deep = inspect_save(&path, true).unwrap();
        assert_eq!(deep.tree.unwrap().kind, "dict");
        let _ = fs::remove_dir_all(dir);
    }
}
