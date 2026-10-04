//! Start the game (or warp into it at a given script line).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use renpy_core::{Launcher, Project};
use serde::Serialize;

/// Scripts written into `game/` for a single launch. Not part of the project.
pub const ONE_LAUNCH_SHIMS: &[&str] =
    &["vnide_developer.rpy", "vnide_live.rpy", "vnide_images.rpy"];

static LAUNCH_GEN: AtomicU64 = AtomicU64::new(1);

fn shim_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// Extra arguments for [`launch`]. `retire` is true for a normal run: the
/// one-launch scripts are deleted after the process has been gone for a few
/// seconds. Live preview sets it false and deletes them itself, so a restart
/// of the same session cannot race that cleanup.
pub struct LaunchExtra<'a> {
    pub env: Vec<(String, String)>,
    pub shims: Vec<(&'a str, &'a str)>,
    pub need_developer: bool,
    pub retire: bool,
    /// Label to land on when the file:line spec matches nothing.
    pub warp_label: Option<String>,
}

impl Default for LaunchExtra<'static> {
    fn default() -> Self {
        Self {
            env: Vec::new(),
            shims: Vec::new(),
            need_developer: false,
            retire: true,
            warp_label: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchReport {
    pub command: String,
    pub warp: Option<String>,
    /// Things worth telling the user (developer mode, packaged game, ...).
    pub notes: Vec<String>,
    /// Per-launch seeds file. Not part of the IPC payload.
    #[serde(skip)]
    pub seeds: Option<PathBuf>,
}

/// Owned copy of the project fields a launch needs, so the caller can drop
/// the project lock before the process is spawned and waited on.
pub struct LaunchSource {
    pub root: PathBuf,
    pub game_dir: PathBuf,
    pub launcher: Option<Launcher>,
    pub script_version: Option<String>,
    pub developer: Option<String>,
    pub seeds: serde_json::Map<String, serde_json::Value>,
}

impl LaunchSource {
    pub fn capture(project: &Project) -> Self {
        Self {
            root: project.root.clone(),
            game_dir: project.game_dir.clone(),
            launcher: project.launcher.clone(),
            script_version: project.script_version.clone(),
            developer: project.config_value("developer"),
            seeds: collect_seeds(project),
        }
    }
}

pub fn launch(
    project: &LaunchSource,
    launcher_override: Option<String>,
    warp: Option<(String, u32)>,
    extra: LaunchExtra<'_>,
) -> Result<LaunchReport, String> {
    let launcher = match launcher_override.filter(|s| !s.trim().is_empty()) {
        Some(path) => Launcher {
            exe: PathBuf::from(path),
            prefix_args: Vec::new(),
        },
        None => project.launcher.clone().ok_or_else(|| {
            "No launcher found next to the project (looked for a game .exe and an SDK renpy.exe). \
             Choose one with \"Set launcher\"."
                .to_string()
        })?,
    };
    if !launcher.exe.is_file() {
        return Err(format!(
            "Launcher {} does not exist.",
            launcher.exe.display()
        ));
    }

    let mut notes = Vec::new();
    let mut warp_spec = None;
    // Bump before taking the lock so a previous launch's cleanup cannot delete
    // the files this launch is about to write.
    let gen = LAUNCH_GEN.fetch_add(1, Ordering::SeqCst) + 1;
    let mut written: Vec<String> = Vec::new();
    // Live preview and warp both install the replacement warp. A plain Run does not.
    let install_warp = warp.is_some() || extra.need_developer || !extra.shims.is_empty();
    let seeds_path = if install_warp {
        Some(write_warp_seeds(project, gen)?)
    } else {
        None
    };
    {
        let _guard = shim_lock();
        remove_shim_files(&project.game_dir, ONE_LAUNCH_SHIMS);
        if let Some((file, line)) = &warp {
            warp_spec = Some(format!("{file}:{line}"));
        }
        // The replacement warp does not ask for developer mode. Reload still does:
        // renpy.reload_script refuses to run when a release build forced it off.
        if install_warp {
            let force_developer = extra.need_developer && warp_needs_developer_shim(project);
            if let Err(e) = write_warp_shim(&project.game_dir, force_developer) {
                if let Some(path) = &seeds_path {
                    let _ = fs::remove_file(path);
                }
                return Err(e);
            }
            written.push("vnide_developer.rpy".into());
            if force_developer {
                notes.push(
                    "Developer mode is off in this game, so reload would be refused. It was turned on for this launch only; game/vnide_developer.rpy is removed when the game exits."
                        .into(),
                );
            }
            if warp.is_some() {
                notes.push(WARP_NAMES_NOTE.into());
            }
        }
        for (name, body) in &extra.shims {
            let path = project.game_dir.join(name);
            if let Err(e) = fs::write(&path, body) {
                remove_shim_files(&project.game_dir, ONE_LAUNCH_SHIMS);
                if let Some(path) = &seeds_path {
                    let _ = fs::remove_file(path);
                }
                return Err(format!("Could not write {}: {e}", path.display()));
            }
            written.push((*name).to_string());
        }
    }

    // Like the official launcher: the project folder always comes first, so the
    // engine never mistakes an option value (`--warp`'s file:line) for the base folder.
    let mut args: Vec<String> = launcher.prefix_args.clone();
    if args.is_empty() {
        args.push(project.root.to_string_lossy().into_owned());
    }
    if let Some(spec) = &warp_spec {
        args.push(format!("--warp={spec}"));
    }
    let mut cmd = Command::new(&launcher.exe);
    cmd.args(&args).current_dir(&project.root);
    if let Some(path) = &seeds_path {
        cmd.env("VNIDE_WARP_SEEDS", path);
    }
    if let Some(label) = extra.warp_label.as_ref().filter(|s| !s.trim().is_empty()) {
        cmd.env("VNIDE_WARP_LABEL", label);
    }
    if warp_spec.is_some() {
        // The shim arms its missing-name guard when this is set. warp_spec is
        // cleared before the landed line runs, so the env is the durable signal.
        cmd.env("VNIDE_WARP", "1");
    }
    for (key, value) in &extra.env {
        cmd.env(key, value);
    }
    let command_line = {
        let mut parts = vec![launcher.exe.to_string_lossy().into_owned()];
        parts.extend(args.iter().map(|a| {
            if a.contains(' ') {
                format!("\"{a}\"")
            } else {
                a.clone()
            }
        }));
        parts.join(" ")
    };
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) => {
            remove_one_launch_shims(&project.game_dir);
            if let Some(path) = &seeds_path {
                let _ = fs::remove_file(path);
            }
            return Err(format!("Could not start {}: {e}", launcher.exe.display()));
        }
    };

    // Catch launchers that die immediately (bad path, missing runtime).
    let started = Instant::now();
    while started.elapsed() < Duration::from_millis(1200) {
        match child.try_wait() {
            Ok(Some(status)) if !status.success() => {
                remove_one_launch_shims(&project.game_dir);
                if let Some(path) = &seeds_path {
                    let _ = fs::remove_file(path);
                }
                return Err(format!(
                    "The game exited immediately ({status}). Check log.txt / traceback.txt in the project folder."
                ));
            }
            Ok(Some(_)) => break,
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            Err(_) => break,
        }
    }

    if extra.retire && !written.is_empty() {
        let game_dir = project.game_dir.clone();
        let exe_name = launcher
            .exe
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        // The shipped .exe often exits immediately and leaves the real interpreter
        // running. Deleting the shim on that first exit removes it before Ren'Py
        // loads scripts, and --warp then fails with developer mode still off.
        retire_one_launch_shims(game_dir, exe_name, child, gen, seeds_path.clone());
    } else {
        // Live preview removes the shims itself. Keep the handle so later
        // checks can `try_wait` instead of spawning `tasklist` while it lives.
        let name = launcher
            .exe
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if let Ok(mut slot) = GAME_CHILD.lock() {
            if let Some((_, mut previous)) = slot.take() {
                let _ = previous.kill();
                let _ = previous.wait();
            }
            *slot = Some((name, child));
        } else {
            let _ = child.wait();
        }
    }

    Ok(LaunchReport {
        command: command_line,
        warp: warp_spec,
        notes,
        seeds: seeds_path,
    })
}

/// Shown when a launch lands in the middle of a script.
pub const WARP_NAMES_NOTE: &str = "Statements before the landing point did not run. Scenes on the way are rebuilt, simple assignments are applied, and a name that was never set is treated as false, which can take the other branch of an if.";

const WARP_SHIM_HEADER: &str = "\
# Written by Ren'Inspector for this launch. Removed when the game exits.
init 999 python:
";

// No backslash after the opening quote. Rust string continuation strips the
// leading spaces of the next line, and Ren'Py 7.1 then sees an empty python
// block (`python block expects a non-empty block`).
const DEVELOPER_FLAGS: &str = "    config.developer = True
    config.default_developer = True
    # A released game skips the duplicate-label screen. Turning developer mode on
    # brings that screen back, and a long list (backup copies in a `_` folder)
    # is big enough that Ren'Py 7.1 runs out of memory drawing it.
    try:
        config.ignore_duplicate_labels = True
    except Exception:
        pass
";

// Python 2.7 and 3. Replaces renpy.warp.warp, so --warp, warp_to_line and
// full_restart all land through here. `py_eval_bytecode` is wrapped because
// compiled screens keep the original `py_eval` from before this init block;
// that function looks `py_eval_bytecode` up when it runs. An `if` condition
// has no side effects to repeat. `py_exec` is left alone so a statement is
// not run twice.
const WARP_RUNTIME: &str = r##"    import json
    import os

    _vnide_fill_names = False
    _vnide_py_eval_orig = renpy.python.py_eval
    _vnide_py_eval_bytecode_orig = renpy.python.py_eval_bytecode

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
                except Exception:
                    return value
            return value
        try:
            return u"%s" % (value,)
        except Exception:
            return u""

    def _vnide_norm_file(name):
        text = _vnide_text(name).replace(u"\\", u"/")
        mark = u"/game/"
        i = text.rfind(mark)
        if i >= 0:
            return text[i + len(mark):]
        if text.startswith(u"game/"):
            return text[len(u"game/"):]
        return text

    def _vnide_store_name(name):
        if (not name) or name.startswith(u"__") or name[:1].isdigit():
            return False
        for ch in name:
            ok = (ch == u"_") or (u"a" <= ch <= u"z") or (u"A" <= ch <= u"Z") or (u"0" <= ch <= u"9")
            if not ok:
                return False
        return True

    def _vnide_warp_log(msg):
        d = u""
        try:
            d = os.environ.get("VNIDE_LIVE_DIR") or u""
        except Exception:
            d = u""
        if not d:
            return
        try:
            f = open(os.path.join(d, "live.log"), "ab")
            raw = _vnide_text(msg)
            try:
                data = raw.encode("utf-8")
            except Exception:
                data = raw
            f.write(data + b"\n")
            f.close()
        except Exception:
            pass

    def _vnide_load_seeds():
        path = u""
        try:
            path = os.environ.get("VNIDE_WARP_SEEDS") or u""
        except Exception:
            path = u""
        if not path:
            return {}
        try:
            f = open(path, "rb")
            raw = f.read()
            f.close()
            try:
                raw = raw.decode("utf-8")
            except Exception:
                pass
            data = json.loads(raw)
        except Exception:
            return {}
        if not isinstance(data, dict):
            return {}
        return data

    def _vnide_seed():
        data = _vnide_load_seeds()
        for name in data:
            if not _vnide_store_name(name):
                continue
            try:
                if hasattr(renpy.store, name):
                    continue
                setattr(renpy.store, name, data[name])
            except Exception:
                pass

    def _vnide_consider():
        global _vnide_fill_names
        if _vnide_fill_names:
            return True
        spec = None
        try:
            spec = renpy.warp.warp_spec
        except Exception:
            spec = None
        env = u""
        try:
            env = os.environ.get("VNIDE_WARP") or u""
        except Exception:
            env = u""
        if spec or env:
            _vnide_fill_names = True
            _vnide_seed()
        return _vnide_fill_names

    def _vnide_missing_name(exc):
        try:
            text = str(exc)
        except Exception:
            return None
        mark = "name '"
        start = text.find(mark)
        if start < 0:
            return None
        start = start + len(mark)
        end = text.find("'", start)
        if end < 0:
            return None
        name = text[start:end]
        if (not name) or name.startswith("__") or name[0].isdigit():
            return None
        for ch in name:
            ok = (ch == "_") or ("a" <= ch <= "z") or ("A" <= ch <= "Z") or ("0" <= ch <= "9")
            if not ok:
                return None
        return name

    def _vnide_arm():
        _vnide_consider()

    def _vnide_bind_missing(name, target):
        if target is None:
            return False
        try:
            if name in target:
                return False
            target[name] = False
            _vnide_remember_false(name)
            return True
        except Exception:
            return False

    def _vnide_remember_false(name):
        try:
            names = getattr(renpy.warp, "_vnide_false", None)
            if names is None:
                names = []
                renpy.warp._vnide_false = names
            if (name in names) or len(names) >= 40:
                return
            names.append(name)
        except Exception:
            pass

    def _vnide_reset_notes():
        try:
            renpy.warp._vnide_false = []
            renpy.warp._vnide_return = u""
            renpy.warp._vnide_cut = u""
        except Exception:
            pass

    def _vnide_warp_notes():
        notes = []
        ret = u""
        cut = u""
        names = []
        try:
            ret = _vnide_text(getattr(renpy.warp, "_vnide_return", u"") or u"")
            cut = _vnide_text(getattr(renpy.warp, "_vnide_cut", u"") or u"")
            names = list(getattr(renpy.warp, "_vnide_false", []) or [])
        except Exception:
            names = []
        if ret:
            notes.append(ret)
        if cut:
            notes.append(cut)
        hidden = False
        for name in names:
            if len(notes) >= 39:
                hidden = True
                break
            notes.append(u"Treated " + _vnide_text(name) + u" as false.")
        if hidden and len(notes) < 40:
            notes.append(u"More names were treated as false.")
        return notes

    def _vnide_py_eval_bytecode(*args, **kwargs):
        tries = 0
        while True:
            try:
                return _vnide_py_eval_bytecode_orig(*args, **kwargs)
            except NameError as exc:
                if (not _vnide_consider()) or tries >= 16:
                    raise
                name = _vnide_missing_name(exc)
                if not name:
                    raise
                target = None
                if len(args) >= 2 and args[1] is not None:
                    target = args[1]
                elif kwargs.get("globals") is not None:
                    target = kwargs.get("globals")
                else:
                    try:
                        target = renpy.python.store_dicts["store"]
                    except Exception:
                        target = None
                if not _vnide_bind_missing(name, target):
                    raise
                tries = tries + 1

    def _vnide_py_eval(*args, **kwargs):
        return _vnide_py_eval_orig(*args, **kwargs)

    renpy.python.py_eval_bytecode = _vnide_py_eval_bytecode
    renpy.python.py_eval = _vnide_py_eval
    try:
        config.start_callbacks.append(_vnide_arm)
    except Exception:
        _vnide_arm()

    def _vnide_literal(text):
        v = text.strip()
        if len(v) >= 2 and ((v[0] == '"' and v[-1] == '"') or (v[0] == "'" and v[-1] == "'")):
            inner = v[1:-1]
            if ("\\" in inner) or (len(inner) > 200):
                return None
            return ("s", inner)
        if v == "True":
            return ("b", True)
        if v == "False":
            return ("b", False)
        if v == "None":
            return ("n", None)
        if v == "[]":
            return ("l", [])
        if v == "{}":
            return ("d", {})
        digits = v
        neg = False
        if v[:1] == "-" and len(v) > 1:
            neg = True
            digits = v[1:]
        if digits.isdigit():
            n = int(digits)
            if neg:
                n = -n
            return ("i", n)
        if digits.count(".") == 1:
            left, right = digits.split(".", 1)
            if left.isdigit() and right.isdigit() and left and right:
                try:
                    return ("f", float(v))
                except Exception:
                    return None
        return None

    def _vnide_strip_comment(line):
        out = []
        q = ""
        for ch in line:
            if q:
                out.append(ch)
                if ch == q:
                    q = ""
                continue
            if ch == "#":
                break
            if ch == '"' or ch == "'":
                q = ch
            out.append(ch)
        return "".join(out).strip()

    def _vnide_apply_line(line):
        text = _vnide_strip_comment(line)
        if (not text) or ("==" in text) or ("!=" in text) or ("<=" in text) or (">=" in text):
            return
        op = ""
        parts = None
        if "+=" in text:
            op = "+"
            parts = text.split("+=", 1)
        elif "-=" in text:
            op = "-"
            parts = text.split("-=", 1)
        elif "=" in text:
            op = "="
            parts = text.split("=", 1)
        else:
            return
        if parts is None or len(parts) != 2:
            return
        name = parts[0].strip()
        if not _vnide_store_name(name):
            return
        lit = _vnide_literal(parts[1])
        if lit is None:
            return
        value = lit[1]
        try:
            if op == "=":
                setattr(renpy.store, name, value)
            elif op == "+":
                setattr(renpy.store, name, getattr(renpy.store, name) + value)
            else:
                setattr(renpy.store, name, getattr(renpy.store, name) - value)
        except Exception:
            pass

    def _vnide_apply_python(n):
        kind = n.__class__.__name__
        if kind != "Python" and kind != "EarlyPython":
            return
        if getattr(n, "hide", False):
            return
        store = getattr(n, "store", None) or "store"
        if store != "store":
            return
        code = getattr(n, "code", None)
        src = getattr(code, "source", None) if code is not None else None
        if not src:
            return
        for line in _vnide_text(src).splitlines():
            _vnide_apply_line(line)

    def _vnide_enclosing_label(node):
        filename = _vnide_norm_file(getattr(node, "filename", "") or "")
        try:
            line = int(getattr(node, "linenumber", 0) or 0)
        except Exception:
            line = 0
        best = u""
        best_line = -1
        try:
            items = list(renpy.game.script.namemap.items())
        except Exception:
            items = []
        for name, n in items:
            try:
                if n.__class__.__name__ != "Label":
                    continue
                if _vnide_norm_file(getattr(n, "filename", "") or "") != filename:
                    continue
                ln = int(getattr(n, "linenumber", 0) or 0)
                if ln <= line and ln >= best_line:
                    best = _vnide_text(getattr(n, "name", None) or name)
                    best_line = ln
            except Exception:
                continue
        return best

    def _vnide_return_on_chain(run, node):
        label = _vnide_enclosing_label(node)
        if not label:
            return None
        site = None
        for n in run:
            try:
                if n.__class__.__name__ != "Call":
                    continue
                if getattr(n, "expression", False):
                    continue
                target = _vnide_text(getattr(n, "label", None) or getattr(n, "target", None))
                if target != label:
                    continue
                nxt = getattr(n, "next", None)
                if nxt is None or not getattr(nxt, "name", None):
                    continue
                site = nxt.name
            except Exception:
                continue
        return site

    def _vnide_add_prev(prev, node, nxt):
        if nxt is None:
            return
        if nxt not in prev:
            prev[nxt] = node
            return
        old = prev[nxt]

        def prefer(fn):
            if fn(node, old):
                return node
            if fn(old, node):
                return old
            return None

        picked = None
        picked = picked or prefer(lambda a, b: (a.filename == nxt.filename) and (b.filename != nxt.filename))
        picked = picked or prefer(lambda a, b: (a.linenumber <= nxt.linenumber) and (b.linenumber > nxt.linenumber))
        picked = picked or prefer(lambda a, b: a.linenumber >= b.linenumber)
        prev[nxt] = picked or node

    def _vnide_chain(node):
        prev = {}
        try:
            seenset = set(renpy.game.script.namemap.values())
        except Exception:
            return [], None, u""
        for n in seenset:
            try:
                if n.__class__.__name__ == "Translate" and getattr(n, "language", None):
                    continue
                if n.__class__.__name__ == "Menu":
                    for i in getattr(n, "items", ()) or ():
                        if i[2] is not None:
                            _vnide_add_prev(prev, n, i[2][0])
                if n.__class__.__name__ == "Jump":
                    if (not getattr(n, "expression", False)) and getattr(n, "target", None) in renpy.game.script.namemap:
                        _vnide_add_prev(prev, n, renpy.game.script.namemap[n.target])
                        continue
                if n.__class__.__name__ == "Call":
                    if not getattr(n, "expression", False):
                        target = getattr(n, "label", None) or getattr(n, "target", None)
                        if target in renpy.game.script.namemap:
                            _vnide_add_prev(prev, n, renpy.game.script.namemap[target])
                            continue
                if n.__class__.__name__ == "While":
                    block = getattr(n, "block", None) or []
                    if block:
                        _vnide_add_prev(prev, n, block[0])
                if n.__class__.__name__ == "If":
                    seen_true = False
                    for condition, block in getattr(n, "entries", ()) or ():
                        if block:
                            _vnide_add_prev(prev, n, block[0])
                        if condition == "True":
                            seen_true = True
                    if seen_true:
                        continue
                if n.__class__.__name__ == "UserStatement":
                    getter = getattr(n, "get_next", None)
                    _vnide_add_prev(prev, n, getter() if getter else None)
                elif getattr(n, "next", None) is not None:
                    _vnide_add_prev(prev, n, n.next)
            except Exception:
                continue
        run = []
        n = node
        while True:
            n = prev.pop(n, None)
            if not n:
                break
            run.append(n)
        run.reverse()
        site = _vnide_return_on_chain(run, node)
        try:
            limit = int(getattr(renpy.config, "warp_limit", 1000) or 1000)
        except Exception:
            limit = 1000
        if limit < 0:
            limit = 0
        cut = u""
        if limit < len(run):
            cut = u"Only the last %d statements on the way were rebuilt." % limit
            if limit:
                run = run[-limit:]
            else:
                run = []
        return run, site, cut

    def _vnide_candidates(filename, line):
        want = _vnide_norm_file(filename)
        found = []
        try:
            nodes = renpy.game.script.namemap.values()
        except Exception:
            return found
        for n in nodes:
            try:
                if _vnide_norm_file(getattr(n, "filename", "") or "") != want:
                    continue
                ln = int(getattr(n, "linenumber", 0) or 0)
                if ln <= line:
                    found.append((ln, n))
            except Exception:
                continue
        found.sort(key=lambda pair: pair[0])
        return found

    def _vnide_hint_node():
        hint = u""
        try:
            hint = _vnide_text(os.environ.get("VNIDE_WARP_LABEL") or u"")
        except Exception:
            hint = u""
        if not hint:
            return None
        try:
            return renpy.game.script.namemap.get(hint)
        except Exception:
            return None

    def _vnide_play(run):
        try:
            renpy.config.skipping = "fast"
        except Exception:
            pass
        _vnide_seed()
        for n in run:
            try:
                _vnide_apply_python(n)
            except Exception:
                pass
            try:
                gate = getattr(n, "can_warp", None)
                if gate and gate():
                    try:
                        n.execute()
                    except Exception:
                        pass
            except Exception:
                pass
        try:
            renpy.config.skipping = None
        except Exception:
            pass

    def _vnide_note_return(site):
        nxt = None
        try:
            nxt = renpy.game.script.namemap.get(site)
        except Exception:
            nxt = None
        label = _vnide_enclosing_label(nxt) if nxt is not None else u""
        if not label:
            label = _vnide_text(site)
        try:
            renpy.warp._vnide_return = u"A return continues in " + label + u"."
        except Exception:
            pass

    def _vnide_land(node, site):
        try:
            renpy.config.skipping = None
            renpy.game.after_rollback = True
            renpy.exports.block_rollback()
        except Exception:
            pass
        ctx = renpy.game.context()
        name = getattr(node, "name", None)
        landed = False
        if site and name:
            try:
                ctx.call(name, return_site=site)
                landed = True
                _vnide_note_return(site)
            except Exception:
                landed = False
        if not landed:
            try:
                ctx.goto_label(name)
            except Exception as e:
                _vnide_warp_log(e)
                return None
        try:
            ctx.come_from(name, "_after_warp")
        except Exception:
            pass
        exc = getattr(renpy.game, "RestartContext", None)
        if exc is None:
            try:
                import renpy.execution as _vnide_execution
                exc = getattr(_vnide_execution, "RestartContext", None)
            except Exception:
                exc = None
        if exc is None:
            return None
        raise exc()

    def _vnide_warp():
        spec = None
        try:
            spec = renpy.warp.warp_spec
        except Exception:
            spec = None
        try:
            renpy.warp.warp_spec = None
        except Exception:
            pass
        if not spec:
            return None
        text = _vnide_text(spec)
        if u":" not in text:
            _vnide_warp_log("bad warp spec")
            return None
        filename, line_text = text.rsplit(u":", 1)
        try:
            line = int(line_text)
        except Exception:
            _vnide_warp_log("bad warp line")
            return None
        _vnide_reset_notes()
        found = _vnide_candidates(filename, line)
        node = found[-1][1] if found else _vnide_hint_node()
        if node is None:
            _vnide_warp_log(u"no statement for " + text)
            return None
        run = []
        site = None
        if found:
            try:
                run, site, cut = _vnide_chain(node)
                try:
                    renpy.warp._vnide_cut = cut or u""
                except Exception:
                    pass
            except Exception as e:
                _vnide_warp_log(e)
                run = []
                site = None
        _vnide_play(run)
        return _vnide_land(node, site)

    try:
        renpy.warp.warp = _vnide_warp
        renpy.warp._vnide_warp_notes = _vnide_warp_notes
    except Exception:
        pass
"##;

fn warp_shim(force_developer: bool) -> String {
    let mut body = String::from(WARP_SHIM_HEADER);
    if force_developer {
        body.push_str(DEVELOPER_FLAGS);
    }
    body.push_str(WARP_RUNTIME);
    body
}

fn warp_needs_developer_shim(project: &LaunchSource) -> bool {
    let developer = project.developer.as_ref().map(|v| v.to_lowercase());
    if developer.as_deref() == Some("true") {
        return false;
    }
    // script_version.txt makes Ren'Py treat the game as a release and force developer off.
    // An explicit False does the same. Either way --warp raises and writes traceback.txt.
    project.script_version.is_some() || developer.as_deref() == Some("false")
}

fn write_warp_shim(game_dir: &Path, force_developer: bool) -> Result<(), String> {
    let path = game_dir.join("vnide_developer.rpy");
    fs::write(&path, warp_shim(force_developer))
        .map_err(|e| format!("Could not write the warp helper for this launch: {e}"))
}

fn write_warp_seeds(project: &LaunchSource, gen: u64) -> Result<PathBuf, String> {
    // Unique per launch so a stop or a second launch cannot delete the file
    // another game is still reading.
    let path = std::env::temp_dir().join(format!("vnide-seeds-{}-{gen}.json", std::process::id()));
    let bytes = serde_json::to_vec(&serde_json::Value::Object(project.seeds.clone()))
        .map_err(|e| format!("Could not write warp seeds: {e}"))?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("Could not write warp seeds: {e}"))?;
    }
    fs::write(&path, bytes).map_err(|e| format!("Could not write warp seeds: {e}"))?;
    Ok(path)
}

/// First literal assignment of each store name. Later lines do not replace it;
/// the warp applies assignments that sit on the path after this.
fn collect_seeds(project: &Project) -> serde_json::Map<String, serde_json::Value> {
    let mut map = serde_json::Map::new();
    for file in &project.files {
        let lines = source_lines(file);
        renpy_core::analysis::walk_all(&file.stmts, &mut |stmt| match &stmt.kind {
            renpy_core::ast::Kind::Define {
                keyword,
                name,
                value,
                ..
            } => {
                // `default` runs after the seeds, and only when the name is still
                // missing. A constructor such as Character() has to be left for it.
                if *keyword == "default" && !plain_literal(value) {
                    return;
                }
                insert_seed(&mut map, name, value);
            }
            renpy_core::ast::Kind::Python {
                block: false, text, ..
            } => {
                if let Some((name, value)) = assignment_literal(text) {
                    insert_seed(&mut map, &name, &value);
                }
            }
            renpy_core::ast::Kind::Python { block: true, .. } => {
                let start = stmt.line as usize;
                let end = (stmt.end_line as usize).min(lines.len());
                if start < end {
                    for src in &lines[start..end] {
                        if let Some((name, value)) = assignment_literal(src) {
                            insert_seed(&mut map, &name, &value);
                        }
                    }
                }
            }
            _ => {}
        });
    }
    map
}

fn insert_seed(map: &mut serde_json::Map<String, serde_json::Value>, name: &str, value: &str) {
    if !store_name(name) || map.contains_key(name) {
        return;
    }
    map.insert(name.to_string(), seed_literal(value));
}

fn source_lines(file: &renpy_core::project::SourceFile) -> Vec<String> {
    if let Some(text) = &file.source {
        return text.lines().map(|s| s.to_string()).collect();
    }
    fs::read_to_string(&file.abs)
        .map(|text| text.lines().map(|s| s.to_string()).collect())
        .unwrap_or_default()
}

/// `name = literal` only. `+=` is applied later, on the path the warp replays.
fn assignment_literal(line: &str) -> Option<(String, String)> {
    let line = strip_rust_comment(line.trim());
    if line.is_empty()
        || line.contains("==")
        || line.contains("!=")
        || line.contains("<=")
        || line.contains(">=")
        || line.contains("+=")
        || line.contains("-=")
        || line.contains('…')
    {
        return None;
    }
    let (name, value) = line.split_once('=')?;
    let name = name.trim();
    let value = value.trim();
    if value.is_empty() || !store_name(name) {
        return None;
    }
    Some((name.to_string(), value.to_string()))
}

fn strip_rust_comment(line: &str) -> String {
    let mut out = String::new();
    let mut quote = None;
    for ch in line.chars() {
        if let Some(q) = quote {
            out.push(ch);
            if ch == q {
                quote = None;
            }
            continue;
        }
        if ch == '#' {
            break;
        }
        if ch == '"' || ch == '\'' {
            quote = Some(ch);
        }
        out.push(ch);
    }
    out.trim().to_string()
}

fn store_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn plain_literal(value: &str) -> bool {
    let v = value.trim();
    if matches!(v, "True" | "False" | "None" | "[]" | "{}") {
        return true;
    }
    if v.parse::<i64>().is_ok() || v.parse::<f64>().ok().is_some_and(|n| n.is_finite()) {
        return true;
    }
    let quoted = (v.starts_with('"') && v.ends_with('"') && v.len() >= 2)
        || (v.starts_with('\'') && v.ends_with('\'') && v.len() >= 2);
    if !quoted {
        return false;
    }
    let inner = &v[1..v.len() - 1];
    inner.len() <= 80 && inner.chars().all(|c| c.is_ascii() && c != '\\')
}

/// A JSON value the warp guard can drop into the store. Anything that is not a
/// plain literal becomes false: constructors such as `Inventory()` are not run.
fn seed_literal(value: &str) -> serde_json::Value {
    let v = value.trim();
    match v {
        "True" => return serde_json::Value::Bool(true),
        "False" => return serde_json::Value::Bool(false),
        "None" => return serde_json::Value::Null,
        "[]" => return serde_json::json!([]),
        "{}" => return serde_json::json!({}),
        _ => {}
    }
    if let Ok(n) = v.parse::<i64>() {
        return serde_json::Value::from(n);
    }
    if let Ok(n) = v.parse::<f64>() {
        if n.is_finite() {
            if let Some(num) = serde_json::Number::from_f64(n) {
                return serde_json::Value::Number(num);
            }
        }
    }
    let quoted = (v.starts_with('"') && v.ends_with('"') && v.len() >= 2)
        || (v.starts_with('\'') && v.ends_with('\'') && v.len() >= 2);
    if quoted {
        let inner = &v[1..v.len() - 1];
        if inner.len() <= 80 && inner.chars().all(|c| c.is_ascii() && c != '\\') {
            return serde_json::Value::String(inner.to_string());
        }
    }
    serde_json::Value::Bool(false)
}

fn remove_shim_files(game_dir: &Path, names: &[&str]) {
    for name in names {
        let path = game_dir.join(name);
        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(path.with_extension("rpyc"));
    }
}

/// Delete one-launch scripts. Safe to call when a newer launch holds them:
/// callers that are about to write shims bump [`LAUNCH_GEN`] first and take
/// [`shim_lock`], and the retire thread checks the generation under that lock.
pub fn remove_one_launch_shims(game_dir: &Path) {
    let _guard = shim_lock();
    remove_shim_files(game_dir, ONE_LAUNCH_SHIMS);
}

/// Drop leftover one-launch scripts when nothing is running. A crash can leave
/// `vnide_developer.rpy`, `vnide_live.rpy` or `vnide_images.rpy` behind; the next open cleans them
/// up unless the game process is still using them.
pub fn remove_leftover_shims(project: &Project) {
    if let Some(launcher) = &project.launcher {
        let name = launcher
            .exe
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if game_running(&name) {
            return;
        }
    }
    remove_one_launch_shims(&project.game_dir);
}

/// The process `launch` started when it did not hand the handle to the retire thread.
static GAME_CHILD: Mutex<Option<(String, std::process::Child)>> = Mutex::new(None);

struct SeenProcess {
    at: Instant,
    running: bool,
}

static SEEN_PROCESS: Mutex<Option<(String, SeenProcess)>> = Mutex::new(None);

pub fn game_running(exe_name: &str) -> bool {
    if exe_name.is_empty() {
        return false;
    }
    if let Ok(mut slot) = GAME_CHILD.lock() {
        let finished = match slot.as_mut() {
            Some((name, child)) if name.eq_ignore_ascii_case(exe_name) => match child.try_wait() {
                Ok(None) => return true,
                Ok(Some(_)) => true,
                Err(_) => false,
            },
            _ => false,
        };
        if finished {
            *slot = None;
        }
    }
    if let Ok(slot) = SEEN_PROCESS.lock() {
        if let Some((name, seen)) = slot.as_ref() {
            if name.eq_ignore_ascii_case(exe_name) && seen.at.elapsed() < Duration::from_secs(1) {
                return seen.running;
            }
        }
    }
    let running = process_exists(exe_name);
    if let Ok(mut slot) = SEEN_PROCESS.lock() {
        *slot = Some((
            exe_name.to_string(),
            SeenProcess {
                at: Instant::now(),
                running,
            },
        ));
    }
    running
}

fn process_exists(exe_name: &str) -> bool {
    #[cfg(windows)]
    {
        let Ok(output) = crate::process::background("tasklist")
            .args([
                "/FI",
                &format!("IMAGENAME eq {exe_name}"),
                "/FO",
                "CSV",
                "/NH",
            ])
            .output()
        else {
            return false;
        };
        return String::from_utf8_lossy(&output.stdout)
            .to_ascii_lowercase()
            .contains(&exe_name.to_ascii_lowercase());
    }
    #[cfg(not(windows))]
    {
        // `pgrep -x` matches the process name. A `.app` or `renpy.sh` launch often
        // shows up as `python` or `renpy`, so a failed lookup just means "not sure".
        let stem = std::path::Path::new(exe_name)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| exe_name.to_string());
        let Ok(output) = crate::process::background("pgrep")
            .args(["-x", &stem])
            .output()
        else {
            return false;
        };
        !output.stdout.is_empty()
    }
}

/// Drop the one-launch scripts only once the game process has been gone for a
/// few seconds, and only if no newer launch has taken them over.
/// A stub exe that exits while the real process is still starting must not win that race.
fn retire_one_launch_shims(
    game_dir: PathBuf,
    exe_name: String,
    mut child: std::process::Child,
    gen: u64,
    seeds: Option<PathBuf>,
) {
    std::thread::spawn(move || {
        let _ = child.wait();
        let mut gone_for: Option<Instant> = None;
        loop {
            if LAUNCH_GEN.load(Ordering::SeqCst) != gen {
                return;
            }
            if game_running(&exe_name) {
                gone_for = None;
            } else {
                let started = gone_for.get_or_insert_with(Instant::now);
                if started.elapsed() >= Duration::from_secs(8) {
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(400));
        }
        let _guard = shim_lock();
        if LAUNCH_GEN.load(Ordering::SeqCst) != gen {
            return;
        }
        remove_shim_files(&game_dir, ONE_LAUNCH_SHIMS);
        if let Some(path) = &seeds {
            let _ = fs::remove_file(path);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use renpy_core::Project;

    fn temp_project(name: &str, files: &[(&str, &str)]) -> (PathBuf, Project) {
        let root = std::env::temp_dir().join(format!("vn-ide-warp-{}-{name}", std::process::id()));
        let game = root.join("game");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&game).unwrap();
        for (rel, text) in files {
            fs::write(game.join(rel), text).unwrap();
        }
        let project = Project::open(&root).unwrap();
        (root, project)
    }

    #[test]
    fn release_marker_requests_a_shim_and_the_shim_is_not_a_script() {
        let (root, project) = temp_project(
            "release",
            &[
                ("script.rpy", "label start:\n    return\n"),
                ("script_version.txt", "(7, 1, 3)"),
                ("vnide_developer.rpy", "# placeholder\n"),
                ("vnide_live.rpy", "init 999 python:\n    pass\n"),
            ],
        );
        let full = warp_shim(true);
        let mut in_python = false;
        for line in full.lines() {
            if line == "init 999 python:" {
                in_python = true;
                continue;
            }
            if in_python && !line.is_empty() {
                assert!(
                    line.starts_with("    "),
                    "Ren'Py 7.1 rejects an unindented python block line: {line}"
                );
            }
        }
        assert!(in_python);
        assert!(full.contains("ignore_duplicate_labels"));
        assert!(full.contains("_vnide_py_eval_bytecode"));
        assert!(full.contains("_vnide_consider"));
        assert!(full.contains("_vnide_seed"));
        assert!(full.contains("_vnide_warp"));
        assert!(full.contains("_vnide_return_on_chain"));
        assert!(full.contains("n.__class__.__name__ == \"Call\""));
        assert!(full.contains("_vnide_warp_notes"));
        assert!(full.contains("Treated "));
        assert!(full.contains("VNIDE_WARP"));
        assert!(full.contains("VNIDE_WARP_SEEDS"));
        assert!(full.contains("VNIDE_WARP_LABEL"));
        assert!(full.contains("RestartContext"));
        assert!(!full.contains("full_restart"));
        assert!(!full.contains("print("));
        assert_eq!(seed_literal("0"), serde_json::json!(0));
        assert_eq!(seed_literal("\"hi\""), serde_json::json!("hi"));
        assert_eq!(seed_literal("Inventory()"), serde_json::json!(false));
        let guard_only = warp_shim(false);
        assert!(!guard_only.contains("ignore_duplicate_labels"));
        assert!(guard_only.contains("target[name] = False"));
        assert!(warp_needs_developer_shim(&LaunchSource::capture(&project)));
        assert!(project.files.iter().all(|f| f.rel != "vnide_developer.rpy"));
        assert!(project.files.iter().all(|f| f.rel != "vnide_live.rpy"));
        fs::write(project.game_dir.join("vnide_live.rpyc"), b"rpyc").unwrap();
        remove_one_launch_shims(&project.game_dir);
        assert!(!project.game_dir.join("vnide_developer.rpy").exists());
        assert!(!project.game_dir.join("vnide_live.rpy").exists());
        assert!(!project.game_dir.join("vnide_live.rpyc").exists());
        assert!(project.game_dir.join("script.rpy").exists());
        let (root2, explicit) = temp_project(
            "explicit",
            &[
                (
                    "script.rpy",
                    "define config.developer = True\nlabel start:\n    return\n",
                ),
                ("script_version.txt", "(7, 1, 3)"),
            ],
        );
        assert!(!warp_needs_developer_shim(&LaunchSource::capture(
            &explicit
        )));
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(root2);
    }

    #[test]
    fn seeds_keep_the_first_literal_assignment() {
        let (root, project) = temp_project("seeds", &[(
            "script.rpy",
            "default points = 0\ndefine hero = \"Ada\"\ndefault who = Character(\"A\")\nlabel start:\n    $ flag = True\n    python:\n        name = \"Eve\"\n        skipped = Hero()\n    $ points = 9\n    \"hi\"\n",
        )]);
        let seeds = collect_seeds(&project);
        assert_eq!(seeds.get("points"), Some(&serde_json::json!(0)));
        assert_eq!(seeds.get("hero"), Some(&serde_json::json!("Ada")));
        assert_eq!(seeds.get("flag"), Some(&serde_json::json!(true)));
        assert_eq!(seeds.get("name"), Some(&serde_json::json!("Eve")));
        assert_eq!(seeds.get("skipped"), Some(&serde_json::json!(false)));
        assert!(
            seeds.get("who").is_none(),
            "a default constructor is left for Ren'Py"
        );
        assert!(assignment_literal("points += 1").is_none());
        assert!(assignment_literal("a == 1").is_none());
        let _ = fs::remove_dir_all(root);
    }
}
