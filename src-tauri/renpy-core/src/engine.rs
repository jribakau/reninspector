//! Ground truth from the game's own bundled Ren'Py engine.
//!
//! `--json-dump` writes the labels, screens and definitions the engine itself
//! found, and `lint` writes the engine's own problem report. Both start the
//! game's engine headless (no window) and exit again. Starting an engine can
//! touch the project folder (`log.txt`, `game/cache`, `game/saves/persistent`),
//! so every run is wrapped in a guard that puts those files back and reports
//! anything it could not undo.
//!
//! Known engine limits (found by running 7 games from Ren'Py 7.1 to 8.5):
//! - labels whose source `.rpy` file is not on disk are left out of the dump
//!   (the engine filters by `os.path.exists`), so compiled-only and archived
//!   scripts stay invisible;
//! - Ren'Py 8.5 dumps no labels at all (its `namemap` keys are nodes, and the
//!   dump skips keys that are not `str`).

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};

use crate::project::Launcher;

pub type Location = (String, u32);

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineDump {
    pub name: String,
    pub version: String,
    /// Label name to (path relative to `game/`, line).
    pub labels: BTreeMap<String, Location>,
    pub screens: BTreeMap<String, Location>,
    pub defines: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineRun {
    pub dump: EngineDump,
    pub notes: Vec<String>,
    pub duration_ms: u64,
    /// Fingerprint of the project's script files when the dump was taken.
    pub key: String,
    #[serde(default)]
    pub from_cache: bool,
}

/// Compact description sent to the UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineSummary {
    pub labels: u32,
    pub screens: u32,
    pub defines: u32,
    pub labels_available: bool,
    pub notes: Vec<String>,
    pub duration_ms: u64,
    pub from_cache: bool,
    /// The scripts changed after the dump was taken.
    pub stale: bool,
}

impl EngineRun {
    pub fn summary(&self, current_key: &str) -> EngineSummary {
        EngineSummary {
            labels: self.dump.labels.len() as u32,
            screens: self.dump.screens.len() as u32,
            defines: self.dump.defines,
            labels_available: !self.dump.labels.is_empty(),
            notes: self.notes.clone(),
            duration_ms: self.duration_ms,
            from_cache: self.from_cache,
            stale: self.key != current_key,
        }
    }
}

/// Bumped when the image dump's shape changes, so a cached list is fetched again.
pub const IMAGE_DUMP_VERSION: u64 = 4;

/// One child of a composite, placed at a pixel offset inside the box.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct EngineLayer {
    pub x: f32,
    pub y: f32,
    pub node: EngineImage,
}

/// One case of a condition switch. `when` is the condition, or `"True"`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct EngineCase {
    pub when: String,
    pub node: EngineImage,
}

/// One step of an ATL frame animation. `seconds` is 0 when no pause follows.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct EngineFrame {
    pub seconds: f32,
    pub node: EngineImage,
}

/// One image from `renpy.display.image.images`, with wrappers flattened.
/// Composites, condition switches and dynamic patterns keep their node tree.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct EngineImage {
    pub name: String,
    /// `image`, `solid`, `ref`, `atl`, `layered`, `dynamic`, `composite`,
    /// `switch`, `pattern`, `frames`, `null`, or another class name.
    pub kind: String,
    pub file: Option<String>,
    pub color: Option<String>,
    #[serde(rename = "ref")]
    pub reference: Option<String>,
    pub w: Option<f32>,
    pub h: Option<f32>,
    pub fit: Option<String>,
    pub zoom: f32,
    pub xzoom: f32,
    pub yzoom: f32,
    pub layers: Vec<EngineLayer>,
    pub cases: Vec<EngineCase>,
    pub pattern: Option<String>,
    pub frames: Vec<EngineFrame>,
    pub repeat: bool,
}

impl Default for EngineImage {
    fn default() -> Self {
        Self {
            name: String::new(),
            kind: "unknown".into(),
            file: None,
            color: None,
            reference: None,
            w: None,
            h: None,
            fit: None,
            zoom: 1.0,
            xzoom: 1.0,
            yzoom: 1.0,
            layers: Vec::new(),
            cases: Vec::new(),
            pattern: None,
            frames: Vec::new(),
            repeat: false,
        }
    }
}

/// A character the engine built during init.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct EngineCharacter {
    pub var: String,
    pub name: Option<String>,
    pub color: Option<String>,
}

/// Images, screen size and characters after the game's init has finished.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct ImageDump {
    pub images: Vec<EngineImage>,
    pub screen_width: Option<u32>,
    pub screen_height: Option<u32>,
    pub characters: Vec<EngineCharacter>,
    /// The game sets `adjust_attributes` or `default_attribute_callbacks`.
    pub callbacks: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageRun {
    pub dump: ImageDump,
    pub notes: Vec<String>,
    pub duration_ms: u64,
    pub key: String,
    #[serde(default)]
    pub from_cache: bool,
}

/// Compact description sent to the UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StageImagesSummary {
    pub count: u32,
    pub stale: bool,
    pub notes: Vec<String>,
    pub duration_ms: u64,
}

impl ImageRun {
    pub fn summary(&self, current_key: &str) -> StageImagesSummary {
        StageImagesSummary {
            count: self.dump.images.len() as u32,
            stale: self.key != current_key,
            notes: self.notes.clone(),
            duration_ms: self.duration_ms,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LintItem {
    pub path: String,
    pub line: u32,
    pub message: String,
}

// ------------------------------------------------------------------ dump

pub fn parse_dump(json: &str) -> Result<EngineDump, String> {
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("Invalid dump: {e}"))?;
    let loc = v
        .get("location")
        .ok_or("The dump has no `location` section.")?;
    let table = |key: &str| -> BTreeMap<String, Location> {
        let mut out = BTreeMap::new();
        if let Some(map) = loc.get(key).and_then(|m| m.as_object()) {
            for (name, val) in map {
                if let Some(arr) = val.as_array() {
                    let file = arr.first().and_then(|f| f.as_str()).unwrap_or("");
                    let line = arr.get(1).and_then(|l| l.as_u64()).unwrap_or(0) as u32;
                    let file = file.replace('\\', "/");
                    let rel = file.strip_prefix("game/").unwrap_or(&file).to_string();
                    out.insert(name.clone(), (rel, line));
                }
            }
        }
        out
    };
    Ok(EngineDump {
        name: v
            .get("name")
            .and_then(|n| n.as_str())
            .unwrap_or("")
            .to_string(),
        version: v
            .get("version")
            .and_then(|n| n.as_str())
            .unwrap_or("")
            .to_string(),
        labels: table("label"),
        screens: table("screen"),
        defines: loc
            .get("define")
            .and_then(|d| d.as_object())
            .map(|d| d.len())
            .unwrap_or(0) as u32,
    })
}

/// Start the bundled engine with `--json-dump` and read the result.
pub fn run_json_dump(
    root: &Path,
    game_dir: &Path,
    launcher: &Launcher,
    key: String,
) -> Result<EngineRun, String> {
    let started = Instant::now();
    let tmp = TempDir::new("dump")?;
    let out = tmp.path().join("dump.json");
    let mut args = base_args(root, launcher);
    args.push("--json-dump".into());
    args.push(out.to_string_lossy().into_owned());
    args.push("quit".into());
    let notes = run_engine(
        root,
        game_dir,
        launcher,
        &args,
        &out,
        Completion::Exists,
        Duration::from_secs(150),
        &EngineExtras::none(),
    )?;
    let text = fs::read_to_string(&out).map_err(|e| format!("The engine produced no dump: {e}"))?;
    let dump = parse_dump(&text)?;
    let mut notes = notes;
    if dump.labels.is_empty() {
        notes.push(
            "This engine version reported no labels in its dump (Ren'Py 8.5 skips them), so the label cross-check is unavailable."
                .into(),
        );
    }
    Ok(EngineRun {
        dump,
        notes,
        duration_ms: started.elapsed().as_millis() as u64,
        key,
        from_cache: false,
    })
}

// ------------------------------------------------------------------ lint

/// Run the engine's `lint` and parse the problems it reports.
pub fn run_lint(
    root: &Path,
    game_dir: &Path,
    launcher: &Launcher,
) -> Result<(Vec<LintItem>, Vec<String>), String> {
    let tmp = TempDir::new("lint")?;
    let out = tmp.path().join("lint.txt");
    let mut args = base_args(root, launcher);
    args.push("lint".into());
    args.push(out.to_string_lossy().into_owned());
    let notes = run_engine(
        root,
        game_dir,
        launcher,
        &args,
        &out,
        Completion::Contains("Statistics:"),
        Duration::from_secs(300),
        &EngineExtras::none(),
    )?;
    let bytes = fs::read(&out).map_err(|e| format!("The engine produced no lint report: {e}"))?;
    let text = String::from_utf8_lossy(&bytes);
    Ok((parse_lint(&text), notes))
}

pub fn parse_lint(text: &str) -> Vec<LintItem> {
    let mut items = Vec::new();
    let mut current: Option<String> = None;
    for raw in text.lines() {
        let line = raw.trim_start_matches('\u{feff}');
        let t = line.trim_end();
        // `game/file.rpy:` header followed by `    * line   12 message`.
        if !t.starts_with(char::is_whitespace) && t.ends_with(':') {
            let name = t.trim_end_matches(':');
            if is_script_name(name) {
                current = Some(norm_path(name));
                continue;
            }
        }
        let body = t.trim_start();
        if let Some(rest) = body.strip_prefix("* line") {
            if let Some(file) = &current {
                let rest = rest.trim_start();
                let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
                if let Ok(n) = digits.parse::<u32>() {
                    items.push(LintItem {
                        path: file.clone(),
                        line: n,
                        message: rest[digits.len()..].trim().to_string(),
                    });
                    continue;
                }
            }
        }
        // `game/file.rpy:12 message` (older engines).
        if let Some(colon) = body.find(':') {
            let (file, tail) = body.split_at(colon);
            if is_script_name(file) {
                let tail = tail[1..].trim_start();
                let digits: String = tail.chars().take_while(|c| c.is_ascii_digit()).collect();
                if let Ok(n) = digits.parse::<u32>() {
                    items.push(LintItem {
                        path: norm_path(file),
                        line: n,
                        message: tail[digits.len()..].trim().to_string(),
                    });
                }
            }
        }
    }
    items
}

fn is_script_name(s: &str) -> bool {
    let l = s.to_ascii_lowercase();
    !s.contains(' ') && (l.ends_with(".rpy") || l.ends_with(".rpym"))
}

fn norm_path(s: &str) -> String {
    let s = s.replace('\\', "/");
    s.strip_prefix("game/").unwrap_or(&s).to_string()
}

// -------------------------------------------------------------- images

/// Image walker shared by the headless dump and the live preview.
/// It only defines functions; nothing runs until one of them is called.
pub const IMAGE_DUMP_PY: &str = r#"# Written by Ren'Inspector. Defines the image list. Does nothing until called.
# Runs on Python 2.7 and Python 3.
init -1999 python:
    import io
    import json
    import os

    def _vnide_text(value):
        if value is None:
            return u""
        try:
            bases = (str, unicode)
        except NameError:
            bases = (str,)
        if isinstance(value, bases):
            if isinstance(value, str):
                try:
                    return value.decode("utf-8", "replace")
                except AttributeError:
                    return value
            return value
        try:
            return u"%s" % (value,)
        except Exception:
            return u""

    def _vnide_bases():
        try:
            return (str, unicode)
        except NameError:
            return (str,)

    def _vnide_num(value):
        if value is None or isinstance(value, bool):
            return None
        try:
            return float(value)
        except Exception:
            return None

    def _vnide_rel(path):
        text = _vnide_text(path).replace(u"\\", u"/")
        try:
            base = _vnide_text(renpy.config.gamedir).replace(u"\\", u"/").rstrip(u"/")
        except Exception:
            base = u""
        if base and text.lower().startswith(base.lower() + u"/"):
            text = text[len(base) + 1:]
        if text.lower().startswith(u"game/"):
            text = text[5:]
        return text

    def _vnide_color(value):
        if value is None:
            return None
        hexcode = getattr(value, "hexcode", None)
        if hexcode:
            return _vnide_text(hexcode)
        text = _vnide_text(value)
        if not text:
            return None
        return text

    _vnide_node_count = {"n": 0}
    _vnide_prefix_cache = {"v": None}
    _vnide_files_cache = {"v": None}

    def _vnide_prefixes():
        if _vnide_prefix_cache["v"] is not None:
            return _vnide_prefix_cache["v"]
        got = None
        try:
            got = renpy.loader.get_prefixes(False, u"images")
        except Exception:
            got = None
        if not got:
            try:
                got = list(renpy.config.search_prefixes)
            except Exception:
                got = None
        prefixes = [_vnide_text(p) for p in (got or [u""])]
        if u"images/" not in prefixes:
            prefixes.append(u"images/")
        _vnide_prefix_cache["v"] = prefixes
        return prefixes

    # Every loose and archived file, lowercased. loadable() cannot be used to
    # test one prefix: older engines search their own prefixes inside it.
    def _vnide_files():
        if _vnide_files_cache["v"] is not None:
            return _vnide_files_cache["v"]
        files = {}
        try:
            for _dir, fn in renpy.loader.listdirfiles():
                text = _vnide_text(fn).replace(u"\\", u"/")
                files[text.lower()] = text
        except Exception:
            files = {}
        _vnide_files_cache["v"] = files
        return files

    def _vnide_resolve(name):
        text = _vnide_text(name).replace(u"\\", u"/")
        if not text:
            return text
        if u"/" in text:
            return _vnide_rel(text)
        files = _vnide_files()
        for prefix in _vnide_prefixes():
            candidate = prefix + text
            if files:
                found = files.get(candidate.lower())
                if found:
                    return found
                continue
            try:
                if renpy.loader.loadable_core(candidate):
                    return candidate
            except Exception:
                continue
        return u"images/" + text

    def _vnide_style_num(d, key):
        style = getattr(d, "style", None)
        if style is None:
            return None
        return _vnide_num(getattr(style, key, None))

    def _vnide_limited(depth, budget):
        if depth > 16:
            return True
        if budget["n"] <= 0:
            return True
        if _vnide_node_count["n"] >= 300000:
            return True
        return False

    def _vnide_spend(budget):
        budget["n"] = budget["n"] - 1
        _vnide_node_count["n"] = _vnide_node_count["n"] + 1

    def _vnide_child(d):
        child = getattr(d, "child", None)
        if child is not None:
            return child
        kids = getattr(d, "children", None) or []
        if kids:
            return kids[0]
        return None

    def _vnide_atl_seconds(st):
        src = getattr(st, "duration", None)
        if src is None:
            return 0.0
        try:
            if isinstance(src, _vnide_bases()):
                src = renpy.python.py_eval(src)
            return float(src)
        except Exception:
            return 0.0

    # Quoted pictures, pauses and repeat of an ATL block, in order.
    def _vnide_atl_collect(block, frames, flags, depth, budget):
        statements = getattr(block, "statements", None) or []
        for st in statements:
            if len(frames) >= 64 or _vnide_limited(depth, budget):
                return
            kind = type(st).__name__
            if kind == "RawBlock":
                _vnide_atl_collect(st, frames, flags, depth + 1, budget)
            elif kind == "RawParallel":
                blocks = getattr(st, "blocks", None) or []
                if blocks:
                    _vnide_atl_collect(blocks[0], frames, flags, depth + 1, budget)
            elif kind == "RawChoice":
                choices = getattr(st, "choices", None) or []
                if choices:
                    try:
                        _vnide_atl_collect(choices[0][1], frames, flags, depth + 1, budget)
                    except Exception:
                        pass
            elif kind == "RawRepeat":
                flags["repeat"] = True
            elif kind == "RawMultipurpose":
                exprs = getattr(st, "expressions", None) or []
                if exprs:
                    for item in exprs:
                        try:
                            value = renpy.python.py_eval(item[0])
                            disp = renpy.easy.displayable(value)
                        except Exception:
                            continue
                        node = {}
                        _vnide_walk(disp, node, depth + 1, budget)
                        if not node.get("kind"):
                            node["kind"] = "unknown"
                        frames.append({"node": node, "seconds": 0.0})
                elif frames:
                    frames[-1]["seconds"] = frames[-1]["seconds"] + _vnide_atl_seconds(st)

    def _vnide_atl_frames(d, acc, depth, budget):
        block = getattr(d, "atl", None)
        if block is None:
            return False
        frames = []
        flags = {"repeat": False}
        try:
            _vnide_atl_collect(block, frames, flags, depth + 1, budget)
        except Exception:
            return False
        if not frames:
            return False
        acc["kind"] = "frames"
        acc["frames"] = frames
        acc["repeat"] = flags["repeat"]
        return True

    def _vnide_walk(d, acc, depth, budget):
        if d is None:
            return
        if _vnide_limited(depth, budget):
            acc["kind"] = "unknown"
            return
        _vnide_spend(budget)
        bases = _vnide_bases()
        if isinstance(d, bases):
            acc["kind"] = "image"
            acc["file"] = _vnide_resolve(d)
            return
        name = type(d).__name__
        if name == "Position":
            _vnide_walk(_vnide_child(d), acc, depth + 1, budget)
            return
        if name == "Null":
            acc["kind"] = "null"
            return
        if name in ("Transform", "ATLTransform") or (name.endswith("Transform") and hasattr(d, "state")):
            state = getattr(d, "state", None)
            if state is not None:
                for key in ("zoom", "xzoom", "yzoom"):
                    num = _vnide_num(getattr(state, key, None))
                    if num is not None:
                        acc[key] = acc.get(key, 1.0) * num
                if acc.get("w") is None:
                    num = _vnide_num(getattr(state, "xsize", None))
                    if num:
                        acc["w"] = num
                if acc.get("h") is None:
                    num = _vnide_num(getattr(state, "ysize", None))
                    if num:
                        acc["h"] = num
                if not acc.get("fit"):
                    fit = getattr(state, "fit", None)
                    if isinstance(fit, bases):
                        acc["fit"] = _vnide_text(fit)
            child = getattr(d, "child", None)
            if child is None:
                child = getattr(d, "original_child", None)
            if child is None:
                if _vnide_atl_frames(d, acc, depth, budget):
                    return
                acc["kind"] = "atl"
                return
            _vnide_walk(child, acc, depth + 1, budget)
            return
        if name == "Scale":
            if acc.get("w") is None:
                num = _vnide_num(getattr(d, "width", None))
                if num:
                    acc["w"] = num
            if acc.get("h") is None:
                num = _vnide_num(getattr(d, "height", None))
                if num:
                    acc["h"] = num
            if not acc.get("fit"):
                acc["fit"] = "fill"
            _vnide_walk(getattr(d, "image", None), acc, depth + 1, budget)
            return
        inner = getattr(d, "image", None)
        module = _vnide_text(getattr(type(d), "__module__", u""))
        im_module = u"display.im"
        if inner is not None and not isinstance(inner, bases) and im_module in module and name != "Image":
            _vnide_walk(inner, acc, depth + 1, budget)
            return
        if name == "Image" or isinstance(getattr(d, "filename", None), bases):
            acc["kind"] = "image"
            acc["file"] = _vnide_resolve(getattr(d, "filename", u""))
            return
        if name == "Solid":
            acc["kind"] = "solid"
            acc["color"] = _vnide_color(getattr(d, "color", None))
            return
        if name == "ImageReference":
            acc["kind"] = "ref"
            n = getattr(d, "name", None)
            if isinstance(n, tuple):
                acc["ref"] = u" ".join([_vnide_text(p) for p in n])
            else:
                acc["ref"] = _vnide_text(n)
            return
        if name in ("Fixed", "MultiBox"):
            acc["kind"] = "composite"
            if acc.get("w") is None:
                num = _vnide_style_num(d, "xminimum")
                if num:
                    acc["w"] = num
            if acc.get("h") is None:
                num = _vnide_style_num(d, "yminimum")
                if num:
                    acc["h"] = num
            layers = []
            children = getattr(d, "children", None) or []
            for child in children:
                layer = {"x": 0, "y": 0}
                node_src = child
                if type(child).__name__ == "Position":
                    x = _vnide_style_num(child, "xpos")
                    y = _vnide_style_num(child, "ypos")
                    if x is not None:
                        layer["x"] = x
                    if y is not None:
                        layer["y"] = y
                    node_src = _vnide_child(child)
                node = {}
                _vnide_walk(node_src, node, depth + 1, budget)
                if not node.get("kind"):
                    node["kind"] = "unknown"
                layer["node"] = node
                layers.append(layer)
            acc["layers"] = layers
            return
        if name == "DynamicDisplayable":
            fn = getattr(d, "function", None)
            fname = _vnide_text(getattr(fn, "__name__", u""))
            if fname == u"condition_switch_show":
                acc["kind"] = "switch"
                args = getattr(d, "args", None) or ()
                switch = args[0] if len(args) > 0 else []
                cases = []
                try:
                    pairs = list(switch)
                except Exception:
                    pairs = []
                for pair in pairs:
                    try:
                        cond = pair[0]
                        disp = pair[1]
                    except Exception:
                        continue
                    if cond is True:
                        when = u"True"
                    elif cond is False:
                        when = u"False"
                    elif cond is None:
                        when = u"None"
                    else:
                        when = _vnide_text(cond)
                    node = {}
                    _vnide_walk(disp, node, depth + 1, budget)
                    if not node.get("kind"):
                        node["kind"] = "unknown"
                    cases.append({"when": when, "node": node})
                acc["cases"] = cases
                return
            acc["kind"] = "dynamic"
            return
        if name == "DynamicImage":
            acc["kind"] = "pattern"
            raw = getattr(d, "name", u"")
            if isinstance(raw, list):
                acc["pattern"] = u" ".join([_vnide_text(p) for p in raw])
            else:
                acc["pattern"] = _vnide_text(raw)
            return
        low = name.lower()
        if "layered" in low:
            acc["kind"] = "layered"
        elif "condition" in low:
            acc["kind"] = "condition"
        elif "dynamic" in low:
            acc["kind"] = "dynamic"
        elif "composite" in low:
            acc["kind"] = "composite"
        else:
            acc["kind"] = low

    def _vnide_write_json(path, payload):
        try:
            raw = json.dumps(payload)
            data = raw.encode("utf-8")
        except Exception:
            data = b'{"error":"could not encode the image list"}'
        partial = path + ".partial"
        f = io.open(partial, "wb")
        try:
            f.write(data)
            f.flush()
            try:
                os.fsync(f.fileno())
            except Exception:
                pass
        finally:
            f.close()
        try:
            os.remove(path)
        except Exception:
            pass
        os.rename(partial, path)

    def _vnide_image_payload():
        try:
            images = []
            try:
                table = renpy.display.image.images
            except Exception:
                table = {}
            for name, d in table.items():
                try:
                    if isinstance(name, tuple):
                        label = u" ".join([_vnide_text(p) for p in name])
                    else:
                        label = _vnide_text(name)
                    acc = {"zoom": 1.0, "xzoom": 1.0, "yzoom": 1.0}
                    budget = {"n": 4000}
                    _vnide_walk(d, acc, 0, budget)
                    item = {
                        "name": label,
                        "kind": acc.get("kind") or "unknown",
                        "file": acc.get("file"),
                        "color": acc.get("color"),
                        "ref": acc.get("ref"),
                        "w": acc.get("w"),
                        "h": acc.get("h"),
                        "fit": acc.get("fit"),
                        "zoom": acc.get("zoom", 1.0),
                        "xzoom": acc.get("xzoom", 1.0),
                        "yzoom": acc.get("yzoom", 1.0),
                    }
                    if acc.get("layers"):
                        item["layers"] = acc["layers"]
                    if acc.get("cases"):
                        item["cases"] = acc["cases"]
                    if acc.get("pattern"):
                        item["pattern"] = acc["pattern"]
                    if acc.get("frames"):
                        item["frames"] = acc["frames"]
                        item["repeat"] = bool(acc.get("repeat"))
                    images.append(item)
                except Exception:
                    continue
            characters = []
            try:
                char_type = renpy.character.ADVCharacter
            except Exception:
                char_type = None
            if char_type is not None:
                try:
                    names = dir(renpy.store)
                except Exception:
                    names = []
                for var in names:
                    if not var or var[:1] == "_":
                        continue
                    try:
                        value = getattr(renpy.store, var)
                    except Exception:
                        continue
                    try:
                        if not isinstance(value, char_type):
                            continue
                    except Exception:
                        continue
                    who = getattr(value, "name", None)
                    if who is not None and not isinstance(who, _vnide_bases()):
                        who = _vnide_text(who)
                    color = None
                    args = getattr(value, "who_args", None)
                    if isinstance(args, dict):
                        color = _vnide_color(args.get("color"))
                    who_text = None if who is None else _vnide_text(who)
                    characters.append({"var": _vnide_text(var), "name": who_text, "color": color})
                    if len(characters) >= 400:
                        break
            width = _vnide_num(getattr(renpy.config, "screen_width", None))
            height = _vnide_num(getattr(renpy.config, "screen_height", None))
            callbacks = False
            try:
                if renpy.config.adjust_attributes or renpy.config.default_attribute_callbacks:
                    callbacks = True
            except Exception:
                callbacks = False
            payload = {
                "screenWidth": None if width is None else int(width),
                "screenHeight": None if height is None else int(height),
                "callbacks": callbacks,
                "characters": characters,
                "images": images,
            }
            return payload
        except Exception as e:
            return {"error": _vnide_text(e)}

"#;

/// Headless command. Registered only when `VNIDE_IMAGE_DUMP` is set.
/// Persistent saving is turned off before the engine quits.
pub const IMAGE_COMMAND_PY: &str = r#"# Registers vnide_images only when VNIDE_IMAGE_DUMP is set.

init python:
    import os
    _vnide_out = os.environ.get("VNIDE_IMAGE_DUMP")
    if _vnide_out:
        def _vnide_dump_images():
            take = getattr(renpy.arguments, "takes_no_arguments", None)
            if take is not None:
                take("Writes the image list for Ren'Inspector.")
            try:
                _vnide_write_json(_vnide_out, _vnide_image_payload())
            except Exception:
                pass
            persistent = getattr(renpy, "persistent", None)
            if persistent is not None:
                try:
                    persistent.should_save_persistent = False
                except Exception:
                    pass
            return False

        renpy.arguments.register_command("vnide_images", _vnide_dump_images)
"#;

fn headless_image_shim() -> &'static str {
    static SHIM: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    SHIM.get_or_init(|| format!("{IMAGE_DUMP_PY}\n{IMAGE_COMMAND_PY}"))
        .as_str()
}

pub fn parse_image_dump(json: &str) -> Result<ImageDump, String> {
    let json = json.trim().trim_start_matches('\u{feff}');
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("Invalid image list: {e}"))?;
    if let Some(err) = v.get("error").and_then(|e| e.as_str()) {
        if !err.is_empty() {
            return Err(format!("The engine could not list images: {err}"));
        }
    }
    serde_json::from_value(v).map_err(|e| format!("Invalid image list: {e}"))
}

/// Start the bundled engine, let init register every image, and read that list.
/// `game_running` refuses the run: a second engine must not touch a live game.
pub fn run_image_dump(
    root: &Path,
    game_dir: &Path,
    launcher: &Launcher,
    key: String,
    game_running: bool,
) -> Result<ImageRun, String> {
    if game_running {
        return Err(
            "The game is running. Stop it before asking the engine for a new image list.".into(),
        );
    }
    let started = Instant::now();
    let tmp = TempDir::new("images")?;
    let out = tmp.path().join("images.json");
    let mut args = base_args(root, launcher);
    args.push("vnide_images".into());
    let dump_path = out.to_string_lossy().into_owned();
    let env = [("VNIDE_IMAGE_DUMP", dump_path.as_str())];
    let shim = headless_image_shim();
    let notes = run_engine(
        root,
        game_dir,
        launcher,
        &args,
        &out,
        Completion::Exists,
        Duration::from_secs(150),
        &EngineExtras {
            shim: Some(("vnide_images.rpy", shim)),
            env: &env,
            // Ren'Py adds save signatures during init and keeps the old file
            // time, so the copies taken before the process have to be written back.
            restore_saves: true,
        },
    )?;
    let text =
        fs::read_to_string(&out).map_err(|e| format!("The engine produced no image list: {e}"))?;
    let dump = parse_image_dump(&text)?;
    Ok(ImageRun {
        dump,
        notes: report_image_notes(notes),
        duration_ms: started.elapsed().as_millis() as u64,
        key,
        from_cache: false,
    })
}

/// Housekeeping from the guard is not a problem with the image list.
fn report_image_notes(notes: Vec<String>) -> Vec<String> {
    notes
        .into_iter()
        .filter(|n| {
            if n.starts_with("Put back ") {
                return false;
            }
            if let Some(list) = n.split("left in place: ").nth(1) {
                let files: Vec<&str> = list
                    .trim()
                    .trim_end_matches('.')
                    .split(", ")
                    .filter(|s| !s.is_empty() && *s != "...")
                    .collect();
                if !files.is_empty()
                    && files
                        .iter()
                        .all(|f| f.to_ascii_lowercase().ends_with(".rpyc"))
                {
                    return false;
                }
            }
            true
        })
        .collect()
}

// --------------------------------------------------------------- running

enum Completion<'a> {
    /// The output file exists.
    Exists,
    /// The output file contains this text.
    Contains(&'a str),
}

struct EngineExtras<'a> {
    shim: Option<(&'a str, &'a str)>,
    env: &'a [(&'a str, &'a str)],
    /// When false, files under `game/saves` are left exactly as the run found them.
    restore_saves: bool,
}

impl EngineExtras<'static> {
    fn none() -> Self {
        Self {
            shim: None,
            env: &[],
            restore_saves: true,
        }
    }
}

/// Removes the shim (and the `.rpyc` the engine may compile next to it) even
/// when the run returns early or panics. Refuses to replace a file that is
/// already in the game.
struct ShimGuard {
    path: Option<PathBuf>,
}

impl ShimGuard {
    fn install(game_dir: &Path, shim: Option<(&str, &str)>) -> Result<Self, String> {
        let Some((name, body)) = shim else {
            return Ok(Self { path: None });
        };
        if Path::new(name).file_name().and_then(|n| n.to_str()) != Some(name) {
            return Err(format!("Refusing to write a shim named {name}."));
        }
        let path = game_dir.join(name);
        if path.exists() {
            // A crash can leave our own shim behind. Identical bytes are ours.
            let ours = fs::read(&path)
                .map(|b| b == body.as_bytes())
                .unwrap_or(false);
            if !ours {
                return Err(format!(
                    "{} already exists and is not the IDE's helper. Rename or remove it before asking the engine to run.",
                    path.display()
                ));
            }
            let _ = fs::remove_file(path.with_extension("rpyc"));
            return Ok(Self { path: Some(path) });
        }
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|e| format!("Could not write {}: {e}", path.display()))?;
        use std::io::Write as _;
        file.write_all(body.as_bytes())
            .map_err(|e| format!("Could not write {}: {e}", path.display()))?;
        Ok(Self { path: Some(path) })
    }
}

impl Drop for ShimGuard {
    fn drop(&mut self) {
        if let Some(path) = self.path.take() {
            let _ = fs::remove_file(&path);
            let _ = fs::remove_file(path.with_extension("rpyc"));
        }
    }
}

/// Temp folder removed on drop, including when the engine run fails.
struct TempDir(PathBuf);

impl TempDir {
    fn new(what: &str) -> Result<Self, String> {
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir =
            std::env::temp_dir().join(format!("vn-ide-{what}-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&dir).map_err(|e| format!("Could not create a temp folder: {e}"))?;
        Ok(Self(dir))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn base_args(root: &Path, launcher: &Launcher) -> Vec<String> {
    // The project folder always comes first, exactly like the official launcher.
    let mut args = launcher.prefix_args.clone();
    if args.is_empty() {
        args.push(root.to_string_lossy().into_owned());
    }
    args
}

/// True once `out` has a non-zero size that has stopped changing. A file that
/// merely exists may still be mid-write, so the engine is not killed for that.
fn file_settled(
    out: &Path,
    last_size: &mut u64,
    last_change: &mut Instant,
    quiet: Duration,
) -> bool {
    let Ok(meta) = fs::metadata(out) else {
        return false;
    };
    let len = meta.len();
    if len == 0 {
        return false;
    }
    if len != *last_size {
        *last_size = len;
        *last_change = Instant::now();
        return false;
    }
    last_change.elapsed() >= quiet
}

fn is_complete(
    out: &Path,
    how: &Completion,
    last_size: &mut u64,
    last_change: &mut Instant,
) -> bool {
    match how {
        Completion::Exists => file_settled(out, last_size, last_change, Duration::from_millis(400)),
        Completion::Contains(needle) => fs::read(out)
            .map(|b| String::from_utf8_lossy(&b).contains(needle))
            .unwrap_or(false),
    }
}

fn kill(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn run_engine(
    root: &Path,
    game_dir: &Path,
    launcher: &Launcher,
    args: &[String],
    out: &Path,
    how: Completion,
    timeout: Duration,
    extras: &EngineExtras<'_>,
) -> Result<Vec<String>, String> {
    if !launcher.exe.is_file() {
        return Err(format!(
            "Launcher {} does not exist.",
            launcher.exe.display()
        ));
    }
    let guard = Guard::new(root, game_dir, extras.restore_saves);
    let shim = ShimGuard::install(game_dir, extras.shim)?;
    let mut cmd = Command::new(&launcher.exe);
    cmd.args(args)
        .current_dir(root)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    for (key, value) in extras.env {
        cmd.env(key, value);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) => {
            return Err(format!("Could not start {}: {e}", launcher.exe.display()));
        }
    };

    let started = Instant::now();
    let mut exited: Option<(Instant, bool)> = None;
    let mut last_size = 0u64;
    let mut last_change = Instant::now();
    let result = loop {
        if is_complete(out, &how, &mut last_size, &mut last_change) {
            // Let the engine finish quitting by itself; kill it if it lingers.
            let grace = Instant::now();
            while grace.elapsed() < Duration::from_secs(5) {
                if matches!(child.try_wait(), Ok(Some(_))) {
                    break;
                }
                sleep(Duration::from_millis(100));
            }
            kill(&mut child);
            break Ok(());
        }
        if exited.is_none() {
            if let Ok(Some(status)) = child.try_wait() {
                exited = Some((Instant::now(), status.success()));
            }
        }
        // Some older launchers hand over to a second process and exit at once;
        // keep waiting for the file in that case.
        if let Some((at, ok)) = exited {
            if !ok && at.elapsed() > Duration::from_secs(10) && !out.exists() {
                break Err("The game's engine exited without writing a result. Check log.txt in the project folder.".to_string());
            }
            if file_settled(
                out,
                &mut last_size,
                &mut last_change,
                Duration::from_secs(3),
            ) {
                break Ok(());
            }
        }
        if started.elapsed() > timeout {
            kill(&mut child);
            break Err(format!(
                "The engine did not finish within {} s.",
                timeout.as_secs()
            ));
        }
        sleep(Duration::from_millis(150));
    };
    // Remove the shim before the guard looks, so it is never reported as a kept file.
    drop(shim);
    let notes = guard.restore();
    result.map(|_| notes)
}

// ----------------------------------------------------------------- guard

type Snapshot = HashMap<PathBuf, (u64, Option<SystemTime>)>;

const BACKUP_CAP: u64 = 128 * 1024 * 1024;

/// Remembers the project folder before an engine run and undoes what the run
/// writes to the places the engine is known to touch.
struct Guard {
    root: PathBuf,
    game_dir: PathBuf,
    before: Snapshot,
    backups: HashMap<PathBuf, (Vec<u8>, Option<SystemTime>)>,
    had_cache: bool,
    had_saves: bool,
    restore_saves: bool,
    /// False after `restore` has run, so `Drop` does not put the snapshot back twice.
    pending: bool,
}

fn snapshot(root: &Path, game_dir: &Path) -> Snapshot {
    fn walk(dir: &Path, out: &mut Snapshot) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for e in entries.flatten() {
            let p = e.path();
            let Ok(ty) = e.file_type() else { continue };
            if ty.is_dir() {
                walk(&p, out);
            } else if let Ok(m) = e.metadata() {
                out.insert(p, (m.len(), m.modified().ok()));
            }
        }
    }
    let mut out = Snapshot::new();
    if let Ok(entries) = fs::read_dir(root) {
        for e in entries.flatten() {
            if let (Ok(ty), Ok(m)) = (e.file_type(), e.metadata()) {
                if ty.is_file() {
                    out.insert(e.path(), (m.len(), m.modified().ok()));
                }
            }
        }
    }
    walk(game_dir, &mut out);
    out
}

impl Guard {
    fn new(root: &Path, game_dir: &Path, restore_saves: bool) -> Guard {
        let before = snapshot(root, game_dir);
        let mut guard = Guard {
            root: root.to_path_buf(),
            game_dir: game_dir.to_path_buf(),
            had_cache: game_dir.join("cache").is_dir(),
            had_saves: game_dir.join("saves").is_dir(),
            restore_saves,
            before,
            backups: HashMap::new(),
            pending: true,
        };
        let mut total = 0u64;
        let paths: Vec<PathBuf> = guard.before.keys().cloned().collect();
        for p in paths {
            if !guard.is_volatile(&p) {
                continue;
            }
            let size = guard.before[&p].0;
            if total + size > BACKUP_CAP {
                continue;
            }
            if let Ok(bytes) = fs::read(&p) {
                total += size;
                let mtime = guard.before[&p].1;
                guard.backups.insert(p, (bytes, mtime));
            }
        }
        guard
    }

    /// Files an engine run is known to write and that are safe to put back.
    fn is_volatile(&self, p: &Path) -> bool {
        if p.parent() == Some(self.root.as_path()) {
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            return name == "log.txt" || name == "traceback.txt" || name == "errors.txt";
        }
        if p.starts_with(self.game_dir.join("cache")) {
            return true;
        }
        self.restore_saves && p.starts_with(self.game_dir.join("saves"))
    }

    /// A save written while this run was going must stay, even if it is new.
    fn leave_saves(&self, p: &Path) -> bool {
        !self.restore_saves && p.starts_with(self.game_dir.join("saves"))
    }

    fn restore(mut self) -> Vec<String> {
        self.restore_now()
    }

    fn restore_now(&mut self) -> Vec<String> {
        if !self.pending {
            return Vec::new();
        }
        self.pending = false;
        let after = snapshot(&self.root, &self.game_dir);
        let mut restored = 0u32;
        let mut removed = 0u32;
        let mut kept: Vec<String> = Vec::new();
        let rel = |p: &Path| -> String {
            p.strip_prefix(&self.root)
                .unwrap_or(p)
                .to_string_lossy()
                .replace('\\', "/")
        };
        for (p, meta) in &after {
            match self.before.get(p) {
                None => {
                    if self.leave_saves(p) {
                    } else if self.is_volatile(p) {
                        if fs::remove_file(p).is_ok() {
                            removed += 1;
                        }
                    } else {
                        kept.push(rel(p));
                    }
                }
                Some(old) if old != meta => {
                    if self.leave_saves(p) {
                    } else if let Some((bytes, mtime)) = self.backups.get(p) {
                        if fs::write(p, bytes).is_ok() {
                            if let (Some(t), Ok(f)) =
                                (mtime, fs::OpenOptions::new().write(true).open(p))
                            {
                                let _ = f.set_modified(*t);
                            }
                            restored += 1;
                        }
                    } else {
                        kept.push(rel(p));
                    }
                }
                _ => {}
            }
        }
        // Volatile files the engine deleted.
        for (p, (bytes, mtime)) in &self.backups {
            if !after.contains_key(p) && fs::write(p, bytes).is_ok() {
                if let (Some(t), Ok(f)) = (mtime, fs::OpenOptions::new().write(true).open(p)) {
                    let _ = f.set_modified(*t);
                }
                restored += 1;
            }
        }
        for (name, had) in [("cache", self.had_cache), ("saves", self.had_saves)] {
            if name == "saves" && !self.restore_saves {
                continue;
            }
            if !had {
                let _ = fs::remove_dir(self.game_dir.join(name));
            }
        }
        let mut notes = Vec::new();
        if restored + removed > 0 {
            notes.push(format!(
                "Put back {restored} and removed {removed} log, cache or save file(s) the engine touched."
            ));
        }
        if !kept.is_empty() {
            kept.sort();
            let shown: Vec<&str> = kept.iter().take(5).map(|s| s.as_str()).collect();
            notes.push(format!(
                "The engine also wrote {} file(s) into the project that were left in place: {}{}.",
                kept.len(),
                shown.join(", "),
                if kept.len() > 5 { ", ..." } else { "" }
            ));
        }
        notes
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        let _ = self.restore_now();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn has_fstring(src: &str) -> bool {
        let bytes = src.as_bytes();
        bytes.windows(2).enumerate().any(|(i, pair)| {
            let marker = pair == b"f\"" || pair == b"f'";
            marker && (i == 0 || !bytes[i - 1].is_ascii_alphanumeric())
        })
    }

    #[test]
    fn parses_dump() {
        let json = r#"{"error":false,"name":"X","version":"1","location":{"label":{"start":["game/script.rpy",3],"b":["game\\sub/b.rpy",9]},"screen":{"s":["game/screens.rpy",1]},"define":{"a":["game/a.rpy",1]}}}"#;
        let d = parse_dump(json).unwrap();
        assert_eq!(d.labels["start"], ("script.rpy".to_string(), 3));
        assert_eq!(d.labels["b"], ("sub/b.rpy".to_string(), 9));
        assert_eq!(d.screens.len(), 1);
        assert_eq!(d.defines, 1);
    }

    #[test]
    fn parses_image_dump() {
        let json = r##"{"screenWidth":1280,"screenHeight":720,"callbacks":true,"characters":[{"var":"e","name":"Eileen","color":"#ffcc00ff"}],"images":[{"name":"rae school default","kind":"image","file":"images/rae.png","fit":"contain","zoom":1.0,"xzoom":-1.0,"yzoom":1.0},{"name":"rae default","kind":"ref","ref":"rae school default"}]}"##;
        let d = parse_image_dump(json).unwrap();
        assert_eq!(
            (d.screen_width, d.screen_height, d.callbacks),
            (Some(1280), Some(720), true)
        );
        assert_eq!(d.characters[0].var, "e");
        assert_eq!(d.images[0].file.as_deref(), Some("images/rae.png"));
        assert_eq!(d.images[0].xzoom, -1.0);
        assert_eq!(d.images[0].fit.as_deref(), Some("contain"));
        assert_eq!(d.images[1].reference.as_deref(), Some("rae school default"));
        assert_eq!(d.images[1].zoom, 1.0);
    }

    #[test]
    fn parses_a_composite_dump() {
        let json = r##"{"images":[{"name":"ian","kind":"composite","w":640,"h":1080,"layers":[{"x":0,"y":12,"node":{"kind":"switch","cases":[{"when":"fian == 'smile'","node":{"kind":"image","file":"images/iansmile.webp"}},{"when":"True","node":{"kind":"pattern","pattern":"ian_[fian].webp"}}]}},{"x":1,"y":2,"node":{"kind":"null"}}]}]}"##;
        let d = parse_image_dump(json).unwrap();
        let ian = &d.images[0];
        assert_eq!(ian.kind, "composite");
        assert_eq!((ian.w, ian.h), (Some(640.0), Some(1080.0)));
        assert_eq!(ian.layers.len(), 2);
        assert_eq!((ian.layers[0].x, ian.layers[0].y), (0.0, 12.0));
        assert_eq!(ian.layers[0].node.kind, "switch");
        assert_eq!(ian.layers[0].node.cases[0].when, "fian == 'smile'");
        assert_eq!(
            ian.layers[0].node.cases[0].node.file.as_deref(),
            Some("images/iansmile.webp")
        );
        assert_eq!(
            ian.layers[0].node.cases[1].node.pattern.as_deref(),
            Some("ian_[fian].webp")
        );
        assert_eq!(ian.layers[1].node.kind, "null");
    }

    #[test]
    fn parses_an_animation_dump() {
        let json = r##"{"images":[{"name":"anim","kind":"frames","repeat":true,"frames":[{"seconds":0.25,"node":{"kind":"ref","ref":"fa"}},{"seconds":0.2,"node":{"kind":"image","file":"images/b.webp"}}]}]}"##;
        let d = parse_image_dump(json).unwrap();
        let anim = &d.images[0];
        assert_eq!(anim.kind, "frames");
        assert!(anim.repeat);
        assert_eq!(anim.frames.len(), 2);
        assert_eq!(anim.frames[0].node.reference.as_deref(), Some("fa"));
        assert_eq!(anim.frames[1].seconds, 0.2);
        assert!(IMAGE_DUMP_PY.contains("_vnide_atl_frames"));
    }

    #[test]
    fn image_dump_error_is_reported() {
        let err = parse_image_dump(r#"{"error":"init failed"}"#).unwrap_err();
        assert!(err.contains("init failed"));
    }

    #[test]
    fn image_shims_stay_on_the_python2_subset() {
        let headless = headless_image_shim();
        for src in [IMAGE_DUMP_PY, IMAGE_COMMAND_PY, headless] {
            assert!(!has_fstring(src), "f-strings need Python 3");
            assert!(!src.contains("print("));
        }
        assert!(IMAGE_DUMP_PY.contains("init -1999 python:"));
        assert!(IMAGE_DUMP_PY.contains("_vnide_image_payload"));
        assert!(IMAGE_DUMP_PY.contains("condition_switch_show"));
        assert!(IMAGE_DUMP_PY.contains("get_prefixes"));
        assert!(IMAGE_DUMP_PY.contains("_vnide_write_json"));
        assert!(IMAGE_DUMP_PY.contains("io.open"));
        assert!(!IMAGE_DUMP_PY.contains("register_command"));
        assert!(IMAGE_COMMAND_PY.contains("VNIDE_IMAGE_DUMP"));
        assert!(IMAGE_COMMAND_PY.contains("should_save_persistent"));
        assert!(IMAGE_COMMAND_PY.contains("register_command(\"vnide_images\""));
        assert!(IMAGE_COMMAND_PY.contains("return False"));
        assert!(headless.contains("_vnide_image_payload"));
    }

    #[test]
    fn image_notes_drop_housekeeping() {
        let notes = report_image_notes(vec![
            "Put back 4 and removed 0 log, cache or save file(s) the engine touched.".into(),
            "The engine also wrote 2 file(s) into the project that were left in place: game/script.rpyc, game/a.rpyc.".into(),
            "The engine also wrote 1 file(s) into the project that were left in place: game/oops.txt.".into(),
        ]);
        let expected = vec![
            "The engine also wrote 1 file(s) into the project that were left in place: game/oops.txt.".to_string(),
        ];
        assert_eq!(notes, expected);
    }

    #[test]
    fn shim_guard_keeps_a_user_file_and_reuses_its_own_leftover() {
        let dir = std::env::temp_dir().join(format!("vn-ide-shim-guard-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let shim = dir.join("vnide_images.rpy");

        fs::write(&shim, "# mine\n").unwrap();
        let err = ShimGuard::install(&dir, Some(("vnide_images.rpy", "# helper\n")))
            .err()
            .unwrap();
        assert!(err.contains("not the IDE's helper"), "{err}");
        assert_eq!(fs::read_to_string(&shim).unwrap(), "# mine\n");

        // A crash left our own copy behind: take it over and clean it up.
        fs::write(&shim, "# helper\n").unwrap();
        let guard = ShimGuard::install(&dir, Some(("vnide_images.rpy", "# helper\n"))).unwrap();
        drop(guard);
        assert!(!shim.exists());

        let guard = ShimGuard::install(&dir, Some(("vnide_images.rpy", "# helper\n"))).unwrap();
        assert!(shim.exists());
        drop(guard);
        assert!(!shim.exists());
        assert!(ShimGuard::install(&dir, Some(("sub/x.rpy", "x"))).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn guard_leaves_saves_when_restore_is_off() {
        let root = std::env::temp_dir().join(format!("vnide-guard-saves-{}", std::process::id()));
        let game = root.join("game");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(game.join("saves")).unwrap();
        fs::create_dir_all(game.join("cache")).unwrap();
        let old = game.join("saves").join("old.save");
        let fresh = game.join("saves").join("new.save");
        let cache = game.join("cache").join("x");
        fs::write(&old, b"old").unwrap();
        fs::write(&cache, b"before").unwrap();
        let guard = Guard::new(&root, &game, false);
        fs::write(&old, b"changed").unwrap();
        fs::write(&fresh, b"new").unwrap();
        fs::write(&cache, b"after").unwrap();
        let _notes = guard.restore();
        assert_eq!(fs::read(&old).unwrap(), b"changed");
        assert_eq!(fs::read(&fresh).unwrap(), b"new");
        assert_eq!(fs::read(&cache).unwrap(), b"before");
        let guard = Guard::new(&root, &game, true);
        fs::write(&old, b"again").unwrap();
        fs::write(game.join("saves").join("later.save"), b"later").unwrap();
        let _notes = guard.restore();
        assert_eq!(fs::read(&old).unwrap(), b"changed");
        assert!(!game.join("saves").join("later.save").exists());
        assert_eq!(fs::read(&fresh).unwrap(), b"new");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn parses_both_lint_formats() {
        let text = "\u{feff}8.2 lint report\n\ngame/script.rpy:\n    * line    12 'x' is not an image.\n    * and 3 more.\n\ngame/b.rpy:30 The label 'q' was not found.\nStatistics:\n";
        let items = parse_lint(text);
        assert_eq!(items.len(), 2);
        assert_eq!((items[0].path.as_str(), items[0].line), ("script.rpy", 12));
        assert_eq!((items[1].path.as_str(), items[1].line), ("b.rpy", 30));
    }
}
