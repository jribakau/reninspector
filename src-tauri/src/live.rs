//! Live preview: the real game runs in its own window and reports where it is.
//!
//! A one-launch script, `game/vnide_live.rpy`, polls `cmd.json` and writes
//! `state.json` in a temp folder. The folder path is passed in `VNIDE_LIVE_DIR`.
//! Files are used instead of a socket so the same script runs on Python 2
//! (Ren'Py 7) and Python 3 (Ren'Py 8), and so nothing asks for a firewall prompt.
//!
//! A jump sets `renpy.warp.warp_spec` and calls `renpy.full_restart`, which works
//! on Ren'Py 7.1 as well as 8. The one-launch developer shim replaces `renpy.warp.warp`,
//! so the restart lands on the line without developer mode. `renpy.reload_script`
//! is not on every build. When it is missing the script writes `restart.json` and
//! quits, and the watcher starts the game again.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant, SystemTime};

use renpy_core::Project;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::commands::AppState;
use crate::launch::{self, LaunchExtra};

pub const LIVE_EVENT: &str = "live-state";
pub const LIVE_SHOT_EVENT: &str = "live-shot";

const WARP_NOTE: &str = crate::launch::WARP_NAMES_NOTE;

static SESSION_IDS: AtomicU64 = AtomicU64::new(1);

/// Injected for one live launch. Kept here so the IDE and the game share one copy.
pub const LIVE_SHIM: &str = r#"# Written by Ren'Inspector for one live-preview launch. Removed when that launch ends.
# Does nothing unless VNIDE_LIVE_DIR is set. Runs on Python 2.7 and Python 3.

init 9999 python:
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

    def _vnide_dir():
        return os.environ.get("VNIDE_LIVE_DIR") or u""

    def _vnide_bytes(value):
        if value is None:
            return b""
        if isinstance(value, str):
            try:
                return value.encode("utf-8")
            except AttributeError:
                return value
        try:
            return value.encode("utf-8")
        except Exception:
            try:
                return _vnide_text(value).encode("utf-8")
            except Exception:
                return b"?"

    def _vnide_log(msg):
        d = _vnide_dir()
        if not d:
            return
        try:
            path = os.path.join(d, "live.log")
            try:
                if os.path.getsize(path) > 262144:
                    return
            except Exception:
                pass
            f = open(path, "ab")
            f.write(_vnide_bytes(_vnide_text(msg)) + b"\n")
            f.close()
        except Exception:
            pass

    def _vnide_atomic(path, data):
        tmp = path + ".tmp"
        f = open(tmp, "wb")
        f.write(_vnide_bytes(data))
        f.close()
        try:
            os.remove(path)
        except Exception:
            pass
        os.rename(tmp, path)

    def _vnide_ack():
        d = _vnide_dir()
        try:
            f = open(os.path.join(d, "ack.txt"), "rb")
            raw = f.read().strip()
            f.close()
            return int(raw)
        except Exception:
            return 0

    _vnide_seen = {"id": 0}
    _vnide_poll_at = {"t": 0}
    _vnide_label_cache = {"key": None, "label": u""}

    def _vnide_save_ack(n):
        d = _vnide_dir()
        _vnide_atomic(os.path.join(d, "ack.txt"), str(int(n)))
        _vnide_seen["id"] = int(n)

    def _vnide_field(obj, key):
        if not isinstance(obj, dict):
            return None
        if key in obj:
            return obj[key]
        try:
            ukey = unicode(key)
        except NameError:
            ukey = key
        if ukey in obj:
            return obj[ukey]
        return None

    def _vnide_jsonable(value):
        if value is None or isinstance(value, (bool, int, float)):
            return value
        try:
            if isinstance(value, long):
                return int(value)
        except NameError:
            pass
        text = _vnide_text(value)
        if len(text) > 200:
            text = text[:200]
        return text

    def _vnide_watch_names():
        d = _vnide_dir()
        try:
            f = open(os.path.join(d, "watch.json"), "rb")
            raw = f.read()
            f.close()
            names = json.loads(raw.decode("utf-8"))
        except Exception:
            return []
        out = []
        if isinstance(names, list):
            for n in names:
                t = _vnide_text(n).strip()
                if t:
                    out.append(t)
                if len(out) >= 40:
                    break
        return out

    def _vnide_vars():
        out = {}
        for name in _vnide_watch_names():
            if not hasattr(renpy.store, name):
                out[name] = None
                continue
            try:
                out[name] = _vnide_jsonable(getattr(renpy.store, name))
            except Exception:
                out[name] = None
        return out

    def _vnide_speaker():
        who = getattr(renpy.store, "_last_say_who", None)
        if who is None:
            return u""
        name = getattr(who, "name", None)
        if name:
            return _vnide_text(name)
        return _vnide_text(who)

    def _vnide_showing():
        fn = getattr(renpy, "get_showing_tags", None)
        if not callable(fn):
            return []
        try:
            tags = []
            for t in fn():
                tags.append(_vnide_text(t))
                if len(tags) >= 24:
                    break
            tags.sort()
            return tags
        except Exception:
            return []

    def _vnide_label(filename, line):
        key = (filename, line)
        if _vnide_label_cache.get("key") == key:
            return _vnide_label_cache.get("label") or u""
        best = u""
        best_line = -1
        try:
            items = renpy.game.script.namemap.items()
        except Exception:
            items = []
        for name, node in items:
            try:
                if node.__class__.__name__ != "Label":
                    continue
                node_file = _vnide_text(getattr(node, "filename", None)).replace(u"\\", u"/")
                if node_file != filename:
                    continue
                ln = getattr(node, "linenumber", 0) or 0
                if ln <= line and ln >= best_line:
                    best = _vnide_text(getattr(node, "name", name))
                    best_line = ln
            except Exception:
                continue
        _vnide_label_cache["key"] = key
        _vnide_label_cache["label"] = best
        return best

    def _vnide_fn(name):
        fn = getattr(renpy, name, None)
        if callable(fn):
            return fn
        if name == "warp_to_line":
            mod = getattr(renpy, "warp", None)
            fn = getattr(mod, "warp_to_line", None) if mod is not None else None
            if callable(fn):
                return fn
        return None

    def _vnide_control(exc):
        return exc.__class__.__name__ in ("JumpException", "JumpOutException", "CallException", "FullRestartException", "QuitException", "RestartException")

    def _vnide_request_restart(spec, reason):
        d = _vnide_dir()
        _vnide_atomic(os.path.join(d, "restart.json"), json.dumps({"warp": spec, "reason": reason}))
        renpy.quit()

    def _vnide_warp_note_lines():
        try:
            warp = getattr(renpy, "warp", None)
            fn = getattr(warp, "_vnide_warp_notes", None) if warp is not None else None
            if fn:
                return list(fn())
        except Exception:
            pass
        return []

    def _vnide_report():
        d = _vnide_dir()
        if not d:
            return
        try:
            if renpy.predicting():
                return
        except Exception:
            pass
        try:
            _vnide_replay_tick()
        except Exception:
            pass
        try:
            filename, line = renpy.get_filename_line()
        except Exception:
            filename, line = u"", 0
        filename = _vnide_text(filename).replace(u"\\", u"/")
        try:
            line = int(line)
        except Exception:
            line = 0
        payload = {
            "file": filename,
            "line": line,
            "label": _vnide_label(filename, line) if line else u"",
            "showing": _vnide_showing(),
            "speaker": _vnide_speaker(),
            "vars": _vnide_vars(),
            "canWarp": getattr(renpy, "full_restart", None) is not None and getattr(renpy, "warp", None) is not None,
            "canReload": _vnide_fn("reload_script") is not None,
            "replay": _vnide_text(_vnide_replay.get("status") or u""),
            "replayReason": _vnide_text(_vnide_replay.get("reason") or u""),
            "warpNotes": _vnide_warp_note_lines(),
        }
        try:
            _vnide_atomic(os.path.join(d, "state.json"), json.dumps(payload))
        except Exception as e:
            _vnide_log(e)
        try:
            if os.path.exists(os.path.join(d, "shots.on")):
                import time
                _vnide_shot["want"] = True
                _vnide_shot["at"] = time.time()
                _vnide_shot["file"] = filename
                _vnide_shot["line"] = line
        except Exception:
            pass

    def _vnide_replay_cancel():
        if _vnide_replay.get("status") == u"running":
            _vnide_replay_finish(u"", u"")
        _vnide_replay["on"] = False
        _vnide_replay["status"] = u""
        _vnide_replay["booted"] = True
        _vnide_replay["reason"] = u""

    def _vnide_do_jump(spec):
        _vnide_replay_cancel()
        # full_restart keeps this process, so the warp guard can see the env.
        try:
            os.environ["VNIDE_WARP"] = "1"
        except Exception:
            pass
        try:
            renpy.warp.warp_spec = spec
        except Exception as e:
            _vnide_log(e)
            return
        renpy.full_restart()

    def _vnide_do_reload():
        fn = _vnide_fn("reload_script")
        if fn is None:
            try:
                filename, line = renpy.get_filename_line()
            except Exception:
                filename, line = u"", 1
            spec = _vnide_text(filename).replace(u"\\", u"/") + u":" + _vnide_text(line)
            _vnide_request_restart(spec, "reload")
            return
        fn()

    _vnide_replay = {"on": False, "status": u"", "cursor": 0, "booted": False, "reason": u""}
    _vnide_shot = {"want": False, "at": 0, "file": u"", "line": 0, "seq": 0}
    _vnide_saved = {}

    def _vnide_norm_file(name):
        text = _vnide_text(name).replace(u"\\", u"/")
        mark = u"/game/"
        i = text.rfind(mark)
        if i >= 0:
            return text[i + len(mark):]
        if text.startswith(u"game/"):
            return text[len(u"game/"):]
        return text

    def _vnide_load_replay():
        d = _vnide_dir()
        if not d:
            return
        path = os.path.join(d, "replay.json")
        if not os.path.exists(path):
            return
        try:
            f = open(path, "rb")
            raw = f.read()
            f.close()
            plan = json.loads(raw.decode("utf-8"))
        except Exception as e:
            _vnide_log(e)
            return
        if not isinstance(plan, dict):
            return
        decs = _vnide_field(plan, "decisions")
        _vnide_replay["on"] = True
        _vnide_replay["status"] = u"pending"
        _vnide_replay["file"] = _vnide_norm_file(_vnide_field(plan, "file") or u"")
        try:
            _vnide_replay["stopLine"] = int(_vnide_field(plan, "stopLine") or 0)
        except Exception:
            _vnide_replay["stopLine"] = 0
        _vnide_replay["label"] = _vnide_text(_vnide_field(plan, "label") or u"")
        _vnide_replay["decompiled"] = bool(_vnide_field(plan, "decompiled"))
        _vnide_replay["entry"] = _vnide_text(_vnide_field(plan, "entry") or u"start") or u"start"
        _vnide_replay["decisions"] = decs if isinstance(decs, list) else []
        _vnide_replay["cursor"] = 0
        _vnide_replay["booted"] = False
        _vnide_replay["reason"] = u""

    def _vnide_replay_tick():
        if _vnide_replay.get("status") != u"running":
            return
        try:
            config.allow_skipping = True
            config.skipping = "fast"
        except Exception:
            pass
        try:
            _preferences.skip_unseen = True
        except Exception:
            pass
        try:
            preferences.skip_unseen = True
        except Exception:
            pass
        for ch in ("music", "sound", "voice"):
            try:
                preferences.set_volume(ch, 0.0)
            except Exception:
                pass

    def _vnide_replay_arm():
        if _vnide_replay.get("status") == u"running":
            _vnide_replay_tick()
            return
        try:
            _vnide_saved["skipping"] = config.skipping
        except Exception:
            _vnide_saved["skipping"] = None
        try:
            _vnide_saved["allow"] = config.allow_skipping
        except Exception:
            _vnide_saved["allow"] = True
        try:
            _vnide_saved["skip_unseen"] = _preferences.skip_unseen
        except Exception:
            pass
        for ch in ("music", "sound", "voice"):
            try:
                _vnide_saved["vol_" + ch] = preferences.get_volume(ch)
            except Exception:
                pass
        _vnide_replay["status"] = u"running"
        _vnide_replay["reason"] = u""
        _vnide_replay_tick()

    def _vnide_replay_finish(status, reason):
        if _vnide_replay.get("status") != u"running":
            return
        _vnide_replay["status"] = status
        _vnide_replay["reason"] = _vnide_text(reason)
        try:
            config.skipping = _vnide_saved.get("skipping", None)
        except Exception:
            pass
        try:
            config.allow_skipping = _vnide_saved.get("allow", True)
        except Exception:
            pass
        try:
            if "skip_unseen" in _vnide_saved:
                _preferences.skip_unseen = _vnide_saved["skip_unseen"]
        except Exception:
            pass
        for ch in ("music", "sound", "voice"):
            key = "vol_" + ch
            if key in _vnide_saved:
                try:
                    preferences.set_volume(ch, _vnide_saved[key])
                except Exception:
                    pass
        try:
            _vnide_report()
        except Exception:
            pass

    def _vnide_is_target(node):
        if _vnide_replay.get("decompiled"):
            if node.__class__.__name__ != "Label":
                return False
            return _vnide_text(getattr(node, "name", u"")) == _vnide_text(_vnide_replay.get("label") or u"")
        try:
            fn = _vnide_norm_file(getattr(node, "filename", u""))
            ln = int(getattr(node, "linenumber", 0) or 0)
        except Exception:
            return False
        try:
            stop = int(_vnide_replay.get("stopLine") or 0)
        except Exception:
            stop = 0
        return fn == _vnide_text(_vnide_replay.get("file") or u"") and ln == stop and stop != 0

    def _vnide_replay_on_node(node):
        if _vnide_replay.get("status") != u"running":
            return
        _vnide_replay_tick()
        if _vnide_is_target(node):
            _vnide_replay_finish(u"done", u"")

    def _vnide_wrap_nodes():
        try:
            import renpy.ast as ast
        except Exception:
            return
        for name in dir(ast):
            cls = getattr(ast, name, None)
            try:
                if not isinstance(cls, type):
                    continue
                if "execute" not in cls.__dict__:
                    continue
            except Exception:
                continue
            orig = cls.__dict__.get("execute")
            if not callable(orig) or getattr(orig, "_vnide", False):
                continue
            def _make(fn):
                def _run(self, *a, **k):
                    try:
                        _vnide_replay_on_node(self)
                    except Exception as e:
                        if _vnide_control(e):
                            raise
                    return fn(self, *a, **k)
                try:
                    _run._vnide = True
                except Exception:
                    pass
                return _run
            try:
                setattr(cls, "execute", _make(orig))
            except Exception:
                pass

    def _vnide_item_label(it):
        try:
            return _vnide_text(it[0])
        except Exception:
            return u""

    def _vnide_item_index(it):
        try:
            if len(it) >= 3:
                return int(it[2])
        except Exception:
            return None
        return None

    def _vnide_next_decision():
        decs = _vnide_replay.get("decisions") or []
        try:
            i = int(_vnide_replay.get("cursor") or 0)
        except Exception:
            i = 0
        if i < 0 or i >= len(decs):
            return None
        item = decs[i]
        if not isinstance(item, dict):
            return None
        return item

    def _vnide_decision_here(decision, filename, line, items):
        if _vnide_replay.get("decompiled"):
            caption = _vnide_text(_vnide_field(decision, "caption") or u"")
            for it in items:
                if _vnide_item_label(it) == caption:
                    return True
            return False
        try:
            want_line = int(_vnide_field(decision, "menuLine") or 0)
        except Exception:
            want_line = 0
        want_file = _vnide_norm_file(_vnide_field(decision, "file") or u"")
        return filename == want_file and line == want_line

    def _vnide_menu(*args, **kwargs):
        orig = _vnide_saved.get("menu")
        if orig is None:
            return None
        if _vnide_replay.get("status") != u"running":
            return orig(*args, **kwargs)
        items = args[0] if args else kwargs.get("items")
        if items is None:
            return orig(*args, **kwargs)
        decision = _vnide_next_decision()
        filename, line = u"", 0
        try:
            filename, line = renpy.get_filename_line()
        except Exception:
            filename, line = u"", 0
        filename = _vnide_norm_file(filename)
        try:
            line = int(line)
        except Exception:
            line = 0
        if decision is None:
            _vnide_replay_finish(u"diverged", u"The story reached a menu that was not on the planned path.")
            return orig(*args, **kwargs)
        if not _vnide_decision_here(decision, filename, line, items):
            _vnide_replay_finish(u"diverged", u"The story left the planned path at " + filename + u":" + _vnide_text(line) + u".")
            return orig(*args, **kwargs)
        try:
            want = int(_vnide_field(decision, "index") or 0)
        except Exception:
            want = 0
        caption = _vnide_text(_vnide_field(decision, "caption") or u"")
        for it in items:
            idx = _vnide_item_index(it)
            lab = _vnide_item_label(it)
            if _vnide_replay.get("decompiled"):
                take = lab == caption
            else:
                take = idx == want
            if not take:
                continue
            try:
                _vnide_replay["cursor"] = int(_vnide_replay.get("cursor") or 0) + 1
            except Exception:
                _vnide_replay["cursor"] = 1
            if idx is None:
                _vnide_replay_finish(u"diverged", u"This Ren'Py menu did not report a choice index.")
                return orig(*args, **kwargs)
            return idx
        _vnide_replay_finish(u"diverged", u"The planned choice was not available: " + caption)
        return orig(*args, **kwargs)

    def _vnide_pause(*args, **kwargs):
        if _vnide_replay.get("status") == u"running":
            return None
        orig = _vnide_saved.get("pause")
        if orig is None:
            return None
        return orig(*args, **kwargs)

    def _vnide_input(*args, **kwargs):
        if _vnide_replay.get("status") == u"running":
            if "default" in kwargs and kwargs["default"] is not None:
                return kwargs["default"]
            if len(args) >= 2 and args[1] is not None:
                return args[1]
            return u""
        orig = _vnide_saved.get("input")
        if orig is None:
            return u""
        return orig(*args, **kwargs)

    def _vnide_install_replay():
        if _vnide_saved.get("installed"):
            return
        _vnide_saved["installed"] = True
        _vnide_saved["menu"] = renpy.exports.menu
        _vnide_saved["pause"] = renpy.pause
        _vnide_saved["input"] = renpy.input
        renpy.exports.menu = _vnide_menu
        try:
            renpy.menu = _vnide_menu
        except Exception:
            pass
        renpy.pause = _vnide_pause
        try:
            renpy.exports.pause = _vnide_pause
        except Exception:
            pass
        renpy.input = _vnide_input
        try:
            renpy.exports.input = _vnide_input
        except Exception:
            pass
        _vnide_wrap_nodes()

    def _vnide_replay_boot():
        if not _vnide_replay.get("on") or _vnide_replay.get("booted"):
            return
        mm = False
        try:
            mm = bool(getattr(renpy.store, "main_menu", False))
        except Exception:
            mm = False
        if not mm:
            try:
                mm = bool(getattr(renpy.context(), "_main_menu", False))
            except Exception:
                pass
        entry = _vnide_text(_vnide_replay.get("entry") or u"start") or u"start"
        if mm:
            _vnide_replay["booted"] = True
            _vnide_replay_arm()
            renpy.jump_out_of_context(entry)
            return
        cur = u""
        try:
            cur = _vnide_text(getattr(renpy.game.context(), "current", None))
        except Exception:
            cur = u""
        if cur == entry:
            _vnide_replay["booted"] = True
            _vnide_replay_arm()

    def _vnide_on_start():
        # Games with no main menu are already at the entry label before the
        # first poll. Arm there so the opening lines are not skipped past.
        if not _vnide_replay.get("on") or _vnide_replay.get("booted"):
            return
        cur = u""
        try:
            cur = _vnide_text(getattr(renpy.game.context(), "current", None))
        except Exception:
            cur = u""
        entry = _vnide_text(_vnide_replay.get("entry") or u"start") or u"start"
        if cur != entry:
            return
        _vnide_replay["booted"] = True
        _vnide_replay_arm()

    def _vnide_jump_label(name):
        _vnide_replay_cancel()
        mm = False
        try:
            mm = bool(getattr(renpy.store, "main_menu", False))
        except Exception:
            mm = False
        nested = 0
        try:
            nested = int(renpy.context_nesting_level())
        except Exception:
            nested = 0
        if mm or nested > 0:
            renpy.jump_out_of_context(name)
        else:
            renpy.jump(name)

    def _vnide_skipping():
        try:
            fn = getattr(renpy, "is_skipping", None)
            if callable(fn):
                return bool(fn())
        except Exception:
            pass
        try:
            return bool(getattr(renpy.config, "skipping", None))
        except Exception:
            return False

    def _vnide_take_shot():
        # interact_callbacks run before the frame is drawn, so the shot waits
        # until a later poll. shots.on is only there while the stage pane is open.
        d = _vnide_dir()
        if not d or not _vnide_shot.get("want"):
            return
        if not os.path.exists(os.path.join(d, "shots.on")):
            _vnide_shot["want"] = False
            return
        if _vnide_replay.get("status") == u"running" or _vnide_skipping():
            return
        now = 0
        try:
            import time
            now = time.time()
        except Exception:
            now = 0
        at = _vnide_shot.get("at") or 0
        if now and (now - at) < 0.15:
            return
        _vnide_shot["want"] = False
        seq = int(_vnide_shot.get("seq") or 0) + 1
        _vnide_shot["seq"] = seq
        tmp = os.path.join(d, "shot.tmp.png")
        final = os.path.join(d, "shot.png")
        try:
            fn = getattr(renpy, "screenshot", None)
            if not callable(fn):
                return
            fn(tmp)
            if os.path.exists(final):
                os.remove(final)
            os.rename(tmp, final)
            payload = {
                "seq": seq,
                "file": _vnide_shot.get("file") or u"",
                "line": _vnide_shot.get("line") or 0,
            }
            _vnide_atomic(os.path.join(d, "shot.json"), json.dumps(payload))
        except Exception as e:
            _vnide_log(e)

    def vnide_live_poll():
        d = _vnide_dir()
        if not d:
            return
        try:
            if renpy.predicting():
                return
        except Exception:
            pass
        try:
            _vnide_take_shot()
        except Exception as e:
            if _vnide_control(e):
                raise
            _vnide_log(e)
        try:
            _vnide_replay_boot()
            _vnide_replay_tick()
        except Exception as e:
            if _vnide_control(e):
                raise
            _vnide_log(e)
        path = os.path.join(d, "cmd.json")
        if not os.path.exists(path):
            return
        try:
            f = open(path, "rb")
            raw = f.read()
            f.close()
            cmd = json.loads(raw.decode("utf-8"))
        except Exception as e:
            _vnide_log(e)
            return
        try:
            cid = int(_vnide_field(cmd, "id") or 0)
        except Exception:
            return
        if cid == 0 or cid <= _vnide_seen.get("id", 0) or cid <= _vnide_ack():
            _vnide_seen["id"] = max(cid, _vnide_seen.get("id", 0))
            return
        _vnide_save_ack(cid)
        op = _vnide_text(_vnide_field(cmd, "op") or u"")
        try:
            if op == u"jump":
                spec = _vnide_text(_vnide_field(cmd, "spec") or u"")
                label = _vnide_text(_vnide_field(cmd, "label") or u"")
                try:
                    if label:
                        try:
                            os.environ["VNIDE_WARP_LABEL"] = label
                        except Exception:
                            os.environ["VNIDE_WARP_LABEL"] = label.encode("utf-8")
                    else:
                        try:
                            del os.environ["VNIDE_WARP_LABEL"]
                        except Exception:
                            pass
                except Exception:
                    pass
                if spec:
                    _vnide_do_jump(spec)
            elif op == u"label":
                name = _vnide_text(_vnide_field(cmd, "name") or u"")
                if name:
                    _vnide_jump_label(name)
            elif op == u"reload":
                _vnide_do_reload()
            elif op == u"images":
                d = _vnide_dir()
                if d:
                    try:
                        raw = json.dumps(_vnide_image_payload())
                        _vnide_atomic(os.path.join(d, "images.json"), raw)
                    except Exception as e:
                        _vnide_log(e)
            elif op == u"stop":
                renpy.quit()
        except Exception as e:
            if _vnide_control(e):
                raise
            _vnide_log(e)

    def _vnide_periodic():
        # The overlay timer does not run while suppress_overlay is set, which
        # is the splash screen and the main menu. Poll from the frame tick then.
        try:
            if not getattr(renpy.store, "suppress_overlay", False):
                return
        except Exception:
            return
        now = 0
        try:
            import time
            now = time.time()
        except Exception:
            now = 0
        last = _vnide_poll_at.get("t") or 0
        if now and (now - last) < 0.25:
            return
        _vnide_poll_at["t"] = now
        vnide_live_poll()

    if _vnide_dir():
        _vnide_seen["id"] = _vnide_ack()
        _vnide_load_replay()
        if _vnide_replay.get("on"):
            _vnide_install_replay()
            try:
                config.start_callbacks.append(_vnide_on_start)
            except Exception:
                pass
        try:
            config.interact_callbacks.append(_vnide_report)
        except Exception:
            config.interact_callbacks = [_vnide_report]
        try:
            if u"vnide_live" not in config.overlay_screens:
                config.overlay_screens.append("vnide_live")
        except Exception:
            config.overlay_screens = ["vnide_live"]
        try:
            config.periodic_callbacks.append(_vnide_periodic)
        except Exception:
            pass

screen vnide_live():
    zorder 1000
    timer 0.25 action Function(vnide_live_poll) repeat True
"#;

fn live_shim() -> &'static str {
    static SHIM: OnceLock<String> = OnceLock::new();
    SHIM.get_or_init(|| format!("{LIVE_SHIM}\n{}", renpy_core::engine::IMAGE_DUMP_PY))
        .as_str()
}

pub const STAGE_IMAGES_EVENT: &str = "stage-images";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LiveShot {
    pub seq: u64,
    pub file: String,
    pub line: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LiveState {
    #[serde(default)]
    pub running: bool,
    #[serde(default)]
    pub file: String,
    #[serde(default)]
    pub line: u32,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub showing: Vec<String>,
    #[serde(default)]
    pub speaker: String,
    #[serde(default)]
    pub vars: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    pub can_warp: bool,
    #[serde(default)]
    pub can_reload: bool,
    /// `pending`, `running`, `done`, `diverged` or `stalled`. Empty when this launch is not a replay.
    #[serde(default)]
    pub replay: String,
    #[serde(default)]
    pub replay_reason: String,
    #[serde(default)]
    pub note: String,
    /// What this warp filled in: a return label, a cut path, names treated as false.
    #[serde(default)]
    pub warp_notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveReport {
    pub notes: Vec<String>,
    pub warp: Option<String>,
    pub label: Option<String>,
    /// Conditions the replay path assumes. Empty for a jump.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assumptions: Vec<String>,
}

pub struct Session {
    pub id: u64,
    pub dir: PathBuf,
    pub game_dir: PathBuf,
    pub exe_name: String,
    pub launcher: Option<String>,
    pub cmd_id: u64,
    pub stop: Arc<AtomicBool>,
    /// Set once the game has written a state file, so capability flags are real.
    pub heard: bool,
    pub can_warp: bool,
    pub can_reload: bool,
    /// This launch is replaying from the start, not warping.
    pub replaying: bool,
    /// Names the warp fills in. Deleted when the session ends.
    pub seeds: Option<PathBuf>,
    /// `init_key` of the scripts this process loaded. Reset on reload and relaunch.
    pub loaded_key: String,
}

#[derive(Serialize)]
struct LiveCmd<'a> {
    id: u64,
    op: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    spec: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    label: Option<String>,
}

#[derive(Deserialize)]
struct RestartReq {
    warp: String,
    #[serde(default)]
    reason: String,
}

fn no_project() -> String {
    "No project is open.".into()
}

fn clean_watch(names: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for name in names {
        let name = name.trim();
        if name.is_empty() || name.len() > 80 {
            continue;
        }
        if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            continue;
        }
        if out.iter().any(|e: &String| e == name) {
            continue;
        }
        out.push(name.to_string());
        if out.len() == 40 {
            break;
        }
    }
    out
}

fn write_json(dir: &Path, name: &str, value: &impl Serialize) -> Result<(), String> {
    let bytes = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    let dest = dir.join(name);
    let tmp = dir.join(format!("{name}.tmp"));
    fs::write(&tmp, bytes).map_err(|e| format!("Could not write {name}: {e}"))?;
    let _ = fs::remove_file(&dest);
    fs::rename(&tmp, &dest).map_err(|e| format!("Could not write {name}: {e}"))
}

fn norm_script(file: &str) -> String {
    let s = file.replace('\\', "/");
    let s = s.trim();
    if s.is_empty() || s == "None" || s == "?" {
        return String::new();
    }
    if let Some(i) = s.rfind("/game/") {
        return s[i + "/game/".len()..].to_string();
    }
    s.strip_prefix("game/").unwrap_or(s).to_string()
}

/// Innermost label or named menu whose source span contains `line`.
pub fn enclosing_label(project: &Project, file: &str, line: u32) -> Option<String> {
    let idx = project.file_index(file)? as i32;
    let mut best: Option<&renpy_core::MapNode> = None;
    for n in &project.analysis.map.nodes {
        if n.file != idx || n.line > line || line > n.end_line {
            continue;
        }
        if n.kind != "label" && n.kind != "menu" {
            continue;
        }
        if best.map(|b| n.line >= b.line).unwrap_or(true) {
            best = Some(n);
        }
    }
    best.map(|n| n.id.clone())
}

fn resolve_exe(project: &Project, launcher: &Option<String>) -> Result<PathBuf, String> {
    if let Some(path) = launcher.as_ref().filter(|s| !s.trim().is_empty()) {
        return Ok(PathBuf::from(path));
    }
    project
        .launcher
        .as_ref()
        .map(|l| l.exe.clone())
        .ok_or_else(|| {
            "No launcher found next to the project (looked for a game .exe and an SDK renpy.exe). \
             Choose one with \"Set launcher\"."
                .into()
        })
}

fn exe_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn live_dir() -> Result<PathBuf, String> {
    static N: AtomicU64 = AtomicU64::new(1);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("vnide-live-{}-{n}", std::process::id()));
    fs::create_dir_all(&dir).map_err(|e| format!("Could not create the live folder: {e}"))?;
    Ok(dir)
}

fn emit(app: &AppHandle, state: &LiveState) {
    let _ = app.emit(LIVE_EVENT, state);
}

fn wait_for_exit(exe: &str, wait: Duration) {
    let started = Instant::now();
    while started.elapsed() < wait {
        if !exe.is_empty()
            && !launch::game_running(exe)
            && started.elapsed() >= Duration::from_millis(200)
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Ask the running game to quit, then delete the one-launch scripts.
pub fn stop_session(app: &AppHandle, wait: Duration) {
    let state = app.state::<AppState>();
    let session = {
        let mut slot = match state.live.lock() {
            Ok(guard) => guard,
            Err(poison) => poison.into_inner(),
        };
        slot.take()
    };
    let Some(session) = session else {
        return;
    };
    session.stop.store(true, Ordering::SeqCst);
    let _ = write_json(
        &session.dir,
        "cmd.json",
        &LiveCmd {
            id: session.cmd_id.saturating_add(1),
            op: "stop",
            spec: None,
            name: None,
            label: None,
        },
    );
    wait_for_exit(&session.exe_name, wait);
    launch::remove_one_launch_shims(&session.game_dir);
    if let Some(path) = &session.seeds {
        let _ = fs::remove_file(path);
    }
    let _ = fs::remove_dir_all(&session.dir);
    emit(
        app,
        &LiveState {
            running: false,
            note: "Live preview stopped.".into(),
            ..LiveState::default()
        },
    );
}

pub fn on_exit(app: &AppHandle) {
    stop_session(app, Duration::from_millis(300));
}

fn cancel_replay(state: &State<'_, AppState>) {
    let Ok(mut slot) = state.live.lock() else {
        return;
    };
    let Some(session) = slot.as_mut() else {
        return;
    };
    session.replaying = false;
    let _ = fs::remove_file(session.dir.join("replay.json"));
}

fn send_cmd(
    state: &State<'_, AppState>,
    op: &str,
    spec: Option<String>,
    name: Option<String>,
    label: Option<String>,
) -> Result<(), String> {
    let (dir, id) = {
        let mut slot = state.live.lock().map_err(|e| e.to_string())?;
        let session = slot
            .as_mut()
            .ok_or_else(|| "Live preview is not running.".to_string())?;
        if session.stop.load(Ordering::SeqCst) {
            return Err("Live preview is stopping.".into());
        }
        session.cmd_id += 1;
        (session.dir.clone(), session.cmd_id)
    };
    write_json(
        &dir,
        "cmd.json",
        &LiveCmd {
            id,
            op,
            spec,
            name,
            label,
        },
    )
}

fn split_warp(spec: &str) -> Option<(String, u32)> {
    let (file, line) = spec.rsplit_once(':')?;
    let line: u32 = line.trim().parse().ok()?;
    let file = norm_script(file);
    if file.is_empty() || line == 0 {
        return None;
    }
    Some((file, line))
}

fn session_still_live(state: &AppState, id: u64, dir: &Path) -> Result<(), String> {
    let slot = state.live.lock().map_err(|e| e.to_string())?;
    match slot.as_ref() {
        Some(session)
            if session.id == id
                && !session.stop.load(Ordering::SeqCst)
                && session.dir == dir
                && dir.is_dir() =>
        {
            Ok(())
        }
        _ => Err("Live preview is stopping.".into()),
    }
}

fn relaunch(app: &AppHandle, spec: &str, reason: &str) -> Result<(), String> {
    let state = app.state::<AppState>();
    let (source, loaded_key) = {
        let guard = state.project.lock().map_err(|e| e.to_string())?;
        let project = guard.as_ref().ok_or_else(no_project)?;
        (launch::LaunchSource::capture(project), project.init_key())
    };
    let (id, dir, launcher, stop, replaying) = {
        let slot = state.live.lock().map_err(|e| e.to_string())?;
        let session = slot
            .as_ref()
            .ok_or_else(|| "Live preview is not running.".to_string())?;
        if session.stop.load(Ordering::SeqCst) {
            return Err("Live preview is stopping.".into());
        }
        (
            session.id,
            session.dir.clone(),
            session.launcher.clone(),
            Arc::clone(&session.stop),
            session.replaying,
        )
    };
    if stop.load(Ordering::SeqCst) {
        return Err("Live preview is stopping.".into());
    }
    if replaying {
        let extra = LaunchExtra {
            env: vec![("VNIDE_LIVE_DIR".into(), dir.to_string_lossy().into_owned())],
            shims: vec![("vnide_live.rpy", live_shim())],
            need_developer: false,
            retire: false,
            warp_label: None,
        };
        session_still_live(&state, id, &dir)?;
        launch::launch(&source, launcher, None, extra)?;
        stamp_loaded_key(&state, &loaded_key);
        emit(
            app,
            &LiveState {
                running: true,
                replay: "pending".into(),
                note: "The game restarted. Replay is running from the start again.".into(),
                ..LiveState::default()
            },
        );
        return Ok(());
    }
    let (file, line) = split_warp(spec).ok_or_else(|| format!("Could not restart at `{spec}`."))?;
    let extra = LaunchExtra {
        env: vec![("VNIDE_LIVE_DIR".into(), dir.to_string_lossy().into_owned())],
        shims: vec![("vnide_live.rpy", live_shim())],
        need_developer: reason == "reload",
        retire: false,
        warp_label: None,
    };
    session_still_live(&state, id, &dir)?;
    launch::launch(&source, launcher, Some((file.clone(), line)), extra)?;
    stamp_loaded_key(&state, &loaded_key);
    let why = if reason == "reload" {
        format!("This Ren'Py has no reload_script, so the game was restarted at {file}:{line}. {WARP_NOTE}")
    } else {
        format!(
            "This Ren'Py has no warp_to_line (that call arrived in 7.5 and 8.0), so the game was restarted at {file}:{line}. {WARP_NOTE}"
        )
    };
    emit(
        app,
        &LiveState {
            running: true,
            file,
            line,
            note: why,
            ..LiveState::default()
        },
    );
    Ok(())
}

fn stamp_loaded_key(state: &AppState, key: &str) {
    if let Ok(mut slot) = state.live.lock() {
        if let Some(session) = slot.as_mut() {
            session.loaded_key = key.to_string();
        }
    }
}

fn traceback_stamp(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|m| m.modified()).ok()
}

fn traceback_text(path: &Path) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    if lines.is_empty() {
        return None;
    }
    let start = lines.len().saturating_sub(8);
    let mut body = lines[start..].join(" ");
    if body.chars().count() > 420 {
        body = body.chars().take(420).collect();
        body.push('…');
    }
    Some(body)
}

fn read_capped(path: &Path, max: u64) -> Option<Vec<u8>> {
    let meta = fs::metadata(path).ok()?;
    if meta.len() > max {
        return None;
    }
    let bytes = fs::read(path).ok()?;
    if bytes.len() as u64 > max {
        None
    } else {
        Some(bytes)
    }
}

fn store_live_images(app: &AppHandle, text: &str, session_id: u64) -> Result<(), String> {
    let dump = renpy_core::engine::parse_image_dump(text)?;
    let state = app.state::<AppState>();
    let key = {
        let slot = state.live.lock().map_err(|e| e.to_string())?;
        let session = slot
            .as_ref()
            .ok_or_else(|| "Live preview is not running.".to_string())?;
        if session.id != session_id {
            return Ok(());
        }
        session.loaded_key.clone()
    };
    let mut guard = state.project.lock().map_err(|e| e.to_string())?;
    let project = guard
        .as_mut()
        .ok_or_else(|| "No project is open.".to_string())?;
    let root = project.root.clone();
    let run = renpy_core::engine::ImageRun {
        dump,
        notes: vec!["From the running game.".into()],
        duration_ms: 0,
        key,
        from_cache: false,
    };
    project.set_stage_images(run.clone());
    let info = project.info();
    drop(guard);
    crate::commands::save_cached_images(app, &root, &run);
    app.emit(STAGE_IMAGES_EVENT, &info)
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn watch_session(
    app: AppHandle,
    dir: PathBuf,
    exe: String,
    id: u64,
    stop: Arc<AtomicBool>,
    root: PathBuf,
    replaying: bool,
) {
    let mut last_text = String::new();
    let mut last_shot = String::new();
    let mut last_images = String::new();
    let mut saw_process = false;
    let mut gone: Option<Instant> = None;
    let tb_path = root.join("traceback.txt");
    let mut tb_seen = traceback_stamp(&tb_path);
    let mut last_body: Option<LiveState> = None;
    let mut stuck_at: Option<(String, u32, Instant)> = None;
    let mut stalled_sent = false;
    while !stop.load(Ordering::SeqCst) {
        if let Some(bytes) = read_capped(&dir.join("restart.json"), 64 * 1024) {
            let _ = fs::remove_file(dir.join("restart.json"));
            if stop.load(Ordering::SeqCst) {
                break;
            }
            if let Ok(req) = serde_json::from_slice::<RestartReq>(&bytes) {
                if let Err(e) = relaunch(&app, &req.warp, &req.reason) {
                    emit(
                        &app,
                        &LiveState {
                            running: false,
                            note: e.clone(),
                            ..LiveState::default()
                        },
                    );
                    finish_stopped(&app, id, &e);
                    break;
                }
                saw_process = false;
                gone = None;
                last_text.clear();
                last_shot.clear();
                last_images.clear();
                stuck_at = None;
                stalled_sent = false;
            }
        }
        if let Some(bytes) = read_capped(&dir.join("state.json"), 1024 * 1024) {
            let text = String::from_utf8_lossy(&bytes);
            if text != last_text {
                last_text = text.to_string();
                if let Ok(mut body) = serde_json::from_str::<LiveState>(&text) {
                    body.running = true;
                    body.file = norm_script(&body.file);
                    if let Ok(guard) = app.state::<AppState>().project.lock() {
                        if let Some(project) = guard.as_ref() {
                            if let Some((file, line)) = project.ide_line(&body.file, body.line) {
                                body.file = file;
                                body.line = line;
                            }
                        }
                    }
                    body.note.clear();
                    if let Ok(mut slot) = app.state::<AppState>().live.lock() {
                        if let Some(session) = slot.as_mut() {
                            if session.id == id {
                                session.heard = true;
                                session.can_warp = body.can_warp;
                                session.can_reload = body.can_reload;
                            }
                        }
                    }
                    let pos = (body.file.clone(), body.line);
                    match &stuck_at {
                        Some((file, line, _)) if file == &pos.0 && *line == pos.1 => {}
                        _ => {
                            stuck_at = Some((pos.0, pos.1, Instant::now()));
                            stalled_sent = false;
                        }
                    }
                    if replaying && stalled_sent && body.replay == "running" {
                        body.replay = "stalled".into();
                        body.replay_reason =
                            "Replay is waiting. A screen or prompt may need you.".into();
                        body.note = body.replay_reason.clone();
                    }
                    last_body = Some(body.clone());
                    emit(&app, &body);
                }
            }
        }
        if replaying {
            if let Some(body) = last_body.as_ref() {
                if body.replay == "running" && !stalled_sent {
                    if let Some((_, _, at)) = &stuck_at {
                        if at.elapsed() >= Duration::from_secs(4) {
                            let mut stalled = body.clone();
                            stalled.replay = "stalled".into();
                            stalled.replay_reason =
                                "Replay is waiting. A screen or prompt may need you.".into();
                            stalled.note = stalled.replay_reason.clone();
                            emit(&app, &stalled);
                            stalled_sent = true;
                        }
                    }
                }
            }
        }
        if let Ok(text) = fs::read_to_string(dir.join("shot.json")) {
            if text != last_shot {
                last_shot = text.clone();
                if let Ok(mut shot) = serde_json::from_str::<LiveShot>(&text) {
                    shot.file = norm_script(&shot.file);
                    if let Ok(guard) = app.state::<AppState>().project.lock() {
                        if let Some(project) = guard.as_ref() {
                            if let Some((file, line)) = project.ide_line(&shot.file, shot.line) {
                                shot.file = file;
                                shot.line = line;
                            }
                        }
                    }
                    let _ = app.emit(LIVE_SHOT_EVENT, &shot);
                }
            }
        }
        if let Some(bytes) = read_capped(&dir.join("images.json"), 8 * 1024 * 1024) {
            let text = String::from_utf8_lossy(&bytes);
            if text != last_images {
                last_images = text.into_owned();
                let _ = store_live_images(&app, &last_images, id);
            }
        }
        let stamp = traceback_stamp(&tb_path);
        if stamp != tb_seen {
            tb_seen = stamp;
            if let Some(text) = stamp.and_then(|_| traceback_text(&tb_path)) {
                let note = if replaying {
                    format!("The game hit an error: {text}")
                } else {
                    format!("The game hit an error: {text} Warp skipped earlier statements. Use Replay to line.")
                };
                let mut body = last_body.clone().unwrap_or_default();
                body.running = true;
                body.note = note;
                emit(&app, &body);
            }
        }
        if launch::game_running(&exe) {
            saw_process = true;
            gone = None;
        } else if saw_process {
            let started = gone.get_or_insert_with(Instant::now);
            if started.elapsed() >= Duration::from_secs(8) {
                finish_stopped(&app, id, "The game exited.");
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn finish_stopped(app: &AppHandle, id: u64, note: &str) {
    let state = app.state::<AppState>();
    let session = {
        let mut slot = match state.live.lock() {
            Ok(guard) => guard,
            Err(poison) => poison.into_inner(),
        };
        if slot.as_ref().map(|s| s.id) != Some(id) {
            return;
        }
        slot.take()
    };
    let Some(session) = session else {
        return;
    };
    session.stop.store(true, Ordering::SeqCst);
    launch::remove_one_launch_shims(&session.game_dir);
    if let Some(path) = &session.seeds {
        let _ = fs::remove_file(path);
    }
    let _ = fs::remove_dir_all(&session.dir);
    emit(
        app,
        &LiveState {
            running: false,
            note: note.into(),
            ..LiveState::default()
        },
    );
}

fn start_gate() -> std::sync::MutexGuard<'static, ()> {
    LIVE_GATE
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
}

static LIVE_GATE: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[tauri::command(async)]
pub fn live_start(
    app: AppHandle,
    state: State<'_, AppState>,
    launcher: Option<String>,
    file: Option<String>,
    line: Option<u32>,
    watch: Vec<String>,
) -> Result<LiveReport, String> {
    let _gate = start_gate();
    stop_session(&app, Duration::from_millis(1200));
    let watched = clean_watch(&watch);
    let dir = live_dir()?;
    write_json(&dir, "watch.json", &watched)?;

    let guard = match state.project.lock() {
        Ok(guard) => guard,
        Err(e) => {
            let _ = fs::remove_dir_all(&dir);
            return Err(e.to_string());
        }
    };
    let Some(project) = guard.as_ref() else {
        let _ = fs::remove_dir_all(&dir);
        return Err(no_project());
    };
    let exe = match resolve_exe(project, &launcher) {
        Ok(exe) => exe,
        Err(e) => {
            let _ = fs::remove_dir_all(&dir);
            return Err(e);
        }
    };
    let name = exe_name(&exe);
    let game_dir = project.game_dir.clone();
    let root = project.root.clone();

    let mut notes = Vec::new();
    let mut warp = None;
    let mut warp_label = None;
    if let (Some(file), Some(line)) = (
        file.filter(|s| !s.trim().is_empty()),
        line.filter(|n| *n > 0),
    ) {
        if project.file_index(&file).is_none() {
            let _ = fs::remove_dir_all(&dir);
            return Err(format!("`{file}` is not a script of this project."));
        }
        let (engine_file, engine_line) = project.engine_spec(&file, line);
        warp_label = enclosing_label(project, &file, line);
        if engine_file != file || engine_line != line {
            notes.push(format!(
                "`{file}` was decompiled from a .rpyc, so the game opens at {engine_file}:{engine_line}. {WARP_NOTE}"
            ));
        } else {
            notes.push(format!("Starting at {file}:{line}. {WARP_NOTE}"));
        }
        warp = Some((engine_file, engine_line));
    }
    notes.push(
        "The game is running in its own window. This editor follows its current line.".into(),
    );

    let extra = LaunchExtra {
        env: vec![("VNIDE_LIVE_DIR".into(), dir.to_string_lossy().into_owned())],
        shims: vec![("vnide_live.rpy", live_shim())],
        need_developer: false,
        retire: false,
        warp_label: warp_label.clone(),
    };
    let source = launch::LaunchSource::capture(project);
    let loaded_key = project.init_key();
    drop(guard);
    let launched = match launch::launch(&source, launcher.clone(), warp.clone(), extra) {
        Ok(report) => report,
        Err(e) => {
            let _ = fs::remove_dir_all(&dir);
            return Err(e);
        }
    };
    notes.splice(0..0, launched.notes);

    let id = SESSION_IDS.fetch_add(1, Ordering::Relaxed);
    let stop = Arc::new(AtomicBool::new(false));
    {
        let mut slot = state.live.lock().map_err(|e| e.to_string())?;
        *slot = Some(Session {
            id,
            dir: dir.clone(),
            game_dir,
            exe_name: name.clone(),
            launcher,
            cmd_id: 0,
            stop: Arc::clone(&stop),
            heard: false,
            can_warp: false,
            can_reload: false,
            replaying: false,
            seeds: launched.seeds.clone(),
            loaded_key,
        });
    }
    let app2 = app.clone();
    std::thread::spawn(move || watch_session(app2, dir, name, id, stop, root, false));
    emit(
        &app,
        &LiveState {
            running: true,
            note: notes.join(" "),
            ..LiveState::default()
        },
    );
    Ok(LiveReport {
        notes,
        warp: warp.map(|(f, l)| format!("{f}:{l}")),
        label: warp_label,
        assumptions: Vec::new(),
    })
}

#[tauri::command(async)]
pub fn live_replay(
    app: AppHandle,
    state: State<'_, AppState>,
    launcher: Option<String>,
    file: String,
    line: u32,
    watch: Vec<String>,
) -> Result<LiveReport, String> {
    if line == 0 {
        return Err("Pick a line in the script first.".into());
    }
    let _gate = start_gate();
    let plan = {
        let guard = state.project.lock().map_err(|e| e.to_string())?;
        let project = guard.as_ref().ok_or_else(no_project)?;
        renpy_core::replay::plan(project, &file, line)?
    };
    stop_session(&app, Duration::from_millis(1200));
    let watched = clean_watch(&watch);
    let dir = live_dir()?;
    let fail = |dir: PathBuf, e: String| {
        let _ = fs::remove_dir_all(&dir);
        e
    };
    if let Err(e) = write_json(&dir, "watch.json", &watched) {
        return Err(fail(dir, e));
    }
    if let Err(e) = write_json(&dir, "replay.json", &plan) {
        return Err(fail(dir, e));
    }

    let guard = match state.project.lock() {
        Ok(guard) => guard,
        Err(e) => return Err(fail(dir, e.to_string())),
    };
    let Some(project) = guard.as_ref() else {
        return Err(fail(dir, no_project()));
    };
    let exe = match resolve_exe(project, &launcher) {
        Ok(exe) => exe,
        Err(e) => return Err(fail(dir, e)),
    };
    let name = exe_name(&exe);
    let game_dir = project.game_dir.clone();
    let root = project.root.clone();
    let extra = LaunchExtra {
        env: vec![("VNIDE_LIVE_DIR".into(), dir.to_string_lossy().into_owned())],
        shims: vec![("vnide_live.rpy", live_shim())],
        need_developer: false,
        retire: false,
        warp_label: None,
    };
    let source = launch::LaunchSource::capture(project);
    let loaded_key = project.init_key();
    drop(guard);
    let launched = match launch::launch(&source, launcher.clone(), None, extra) {
        Ok(report) => report,
        Err(e) => return Err(fail(dir, e)),
    };

    let choices = plan.decisions.len();
    let choice_word = if choices == 1 { "choice" } else { "choices" };
    let mut notes = launched.notes;
    notes.push(format!(
        "Replaying from the start to {}:{} ({choices} menu {choice_word}). The game runs the real script, so persistent data and seen flags can change.",
        plan.file, plan.line
    ));
    if plan.decompiled {
        notes.push(format!(
            "`{}` was decompiled, so replay stops at label {} instead of a line.",
            plan.file,
            plan.label.as_deref().unwrap_or("start")
        ));
    }
    if !plan.assumptions.is_empty() {
        let lead = if plan.assumptions.len() == 1 {
            "This path assumes one condition"
        } else {
            "This path assumes these conditions"
        };
        notes.push(format!("{lead}: {}.", plan.assumptions.join("; ")));
    }
    notes.push(
        "The game is running in its own window. This editor follows its current line.".into(),
    );

    let id = SESSION_IDS.fetch_add(1, Ordering::Relaxed);
    let stop = Arc::new(AtomicBool::new(false));
    {
        let mut slot = state.live.lock().map_err(|e| e.to_string())?;
        *slot = Some(Session {
            id,
            dir: dir.clone(),
            game_dir,
            exe_name: name.clone(),
            launcher,
            cmd_id: 0,
            stop: Arc::clone(&stop),
            heard: false,
            can_warp: false,
            can_reload: false,
            replaying: true,
            seeds: launched.seeds.clone(),
            loaded_key,
        });
    }
    let app2 = app.clone();
    std::thread::spawn(move || watch_session(app2, dir, name, id, stop, root, true));
    emit(
        &app,
        &LiveState {
            running: true,
            replay: "pending".into(),
            note: notes.join(" "),
            ..LiveState::default()
        },
    );
    Ok(LiveReport {
        notes,
        warp: None,
        label: plan.label,
        assumptions: plan.assumptions,
    })
}

#[tauri::command]
pub fn live_jump(
    state: State<'_, AppState>,
    file: String,
    line: u32,
) -> Result<LiveReport, String> {
    if line == 0 {
        return Err("Pick a line in the script first.".into());
    }
    let guard = state.project.lock().map_err(|e| e.to_string())?;
    let project = guard.as_ref().ok_or_else(no_project)?;
    if project.file_index(&file).is_none() {
        return Err(format!("`{file}` is not a script of this project."));
    }
    let (engine_file, engine_line) = project.engine_spec(&file, line);
    let label = enclosing_label(project, &file, line);
    let mapped = engine_file != file || engine_line != line;
    drop(guard);
    cancel_replay(&state);
    let spec = format!("{engine_file}:{engine_line}");
    send_cmd(&state, "jump", Some(spec.clone()), None, label.clone())?;
    let note = if mapped {
        format!("`{file}` was decompiled from a .rpyc, so the game opens at {spec}. {WARP_NOTE}")
    } else {
        format!("Running the game from {spec}. {WARP_NOTE}")
    };
    Ok(LiveReport {
        notes: vec![note],
        warp: Some(spec),
        label,
        assumptions: Vec::new(),
    })
}

#[tauri::command]
pub fn live_jump_label(state: State<'_, AppState>, name: String) -> Result<LiveReport, String> {
    let name = name.trim().to_string();
    if name.is_empty() || name.contains(['\n', '\r']) {
        return Err("That is not a label name.".into());
    }
    cancel_replay(&state);
    send_cmd(&state, "label", None, Some(name.clone()), None)?;
    Ok(LiveReport {
        notes: vec![format!("Jumped the game to label {name}. {WARP_NOTE}")],
        warp: None,
        label: Some(name),
        assumptions: Vec::new(),
    })
}

#[tauri::command]
pub fn live_reload(state: State<'_, AppState>) -> Result<String, String> {
    let older = {
        let slot = state.live.lock().map_err(|e| e.to_string())?;
        slot.as_ref()
            .map(|s| s.heard && !s.can_reload)
            .unwrap_or(false)
    };
    send_cmd(&state, "reload", None, None, None)?;
    if let Ok(guard) = state.project.lock() {
        if let Some(key) = guard.as_ref().map(|p| p.init_key()) {
            drop(guard);
            stamp_loaded_key(&state, &key);
        }
    }
    if older {
        Ok(
            "This Ren'Py has no reload_script, so the game will restart at the current line."
                .into(),
        )
    } else {
        Ok("Asked the game to reload scripts.".into())
    }
}

#[tauri::command]
pub fn live_images(state: State<'_, AppState>) -> Result<(), String> {
    let current = {
        let guard = state.project.lock().map_err(|e| e.to_string())?;
        let project = guard.as_ref().ok_or_else(no_project)?;
        project.init_key()
    };
    {
        let slot = state.live.lock().map_err(|e| e.to_string())?;
        let session = slot
            .as_ref()
            .ok_or_else(|| "Live preview is not running.".to_string())?;
        if session.stop.load(Ordering::SeqCst) {
            return Err("Live preview is stopping.".into());
        }
        if session.loaded_key != current {
            return Err(
                "Scripts changed since the game loaded them. Reload the game to refresh images."
                    .into(),
            );
        }
    }
    send_cmd(&state, "images", None, None, None)?;
    Ok(())
}

#[tauri::command]
pub fn live_stop(app: AppHandle) -> Result<(), String> {
    stop_session(&app, Duration::from_millis(1200));
    Ok(())
}

#[tauri::command]
pub fn live_shots(state: State<'_, AppState>, on: bool) -> Result<(), String> {
    let path = {
        let slot = state.live.lock().map_err(|e| e.to_string())?;
        let Some(session) = slot.as_ref() else {
            return Ok(());
        };
        session.dir.join("shots.on")
    };
    if on {
        fs::write(&path, b"1").map_err(|e| format!("Could not enable screenshots: {e}"))
    } else {
        let _ = fs::remove_file(path);
        Ok(())
    }
}

#[tauri::command]
pub fn live_shot(state: State<'_, AppState>) -> Result<tauri::ipc::Response, String> {
    let path = {
        let slot = state.live.lock().map_err(|e| e.to_string())?;
        let Some(session) = slot.as_ref() else {
            return Err("The game is not running.".into());
        };
        session.dir.join("shot.png")
    };
    let bytes = fs::read(&path).map_err(|_| "No screenshot yet.".to_string())?;
    if bytes.len() > 8_000_000 {
        return Err("The screenshot is too large.".into());
    }
    Ok(tauri::ipc::Response::new(bytes))
}

#[tauri::command]
pub fn live_set_watch(state: State<'_, AppState>, names: Vec<String>) -> Result<(), String> {
    let names = clean_watch(&names);
    let dir = {
        let slot = state.live.lock().map_err(|e| e.to_string())?;
        let Some(session) = slot.as_ref() else {
            return Ok(());
        };
        session.dir.clone()
    };
    write_json(&dir, "watch.json", &names)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shim_stays_on_the_python2_subset_and_has_a_restart_fallback() {
        assert!(!LIVE_SHIM.contains("f\""));
        assert!(!LIVE_SHIM.contains("print("));
        assert!(LIVE_SHIM.contains("full_restart"));
        assert!(LIVE_SHIM.contains("periodic_callbacks"));
        assert!(LIVE_SHIM.contains("suppress_overlay"));
        assert!(LIVE_SHIM.contains("VNIDE_WARP_LABEL"));
        assert!(LIVE_SHIM.contains("reload_script"));
        assert!(LIVE_SHIM.contains("restart.json"));
        assert!(LIVE_SHIM.contains("warpNotes"));
        assert!(LIVE_SHIM.contains("_vnide_warp_notes"));
        assert!(LIVE_SHIM.contains("VNIDE_LIVE_DIR"));
        assert!(LIVE_SHIM.contains("shots.on"));
        assert!(LIVE_SHIM.contains("renpy.screenshot") || LIVE_SHIM.contains("\"screenshot\""));
        assert!(LIVE_SHIM.contains("shot.json"));
        assert!(LIVE_SHIM.contains("0.15"));
        assert!(LIVE_SHIM.contains("screen vnide_live"));
        assert!(LIVE_SHIM.contains("replay.json"));
        assert!(LIVE_SHIM.contains("jump_out_of_context"));
        assert!(LIVE_SHIM.contains("_vnide_menu"));
        assert!(LIVE_SHIM.contains("JumpOutException"));
        // Control-flow exceptions have to leave the timer action, or the jump is swallowed.
        assert!(LIVE_SHIM.contains("if _vnide_control(e):\n                raise"));
    }

    #[test]
    fn live_shim_carries_the_image_list_and_stays_on_python2() {
        let src = live_shim();
        assert!(src.contains("_vnide_image_payload"));
        assert!(src.contains("elif op == u\"images\":"));
        assert!(src.contains("screen vnide_live"));
        assert!(!src.contains("print("));
        let bytes = src.as_bytes();
        let fstring = bytes.windows(2).enumerate().any(|(i, pair)| {
            let marker = pair == b"f\"" || pair == b"f'";
            marker && (i == 0 || !bytes[i - 1].is_ascii_alphanumeric())
        });
        assert!(!fstring, "f-strings need Python 3");
    }

    #[test]
    fn images_json_in_a_live_dir_parses() {
        let dir = std::env::temp_dir().join(format!("vnide-live-images-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("images.json"),
            r#"{"images":[{"name":"eileen","kind":"image","file":"e.png"}],"characters":[],"callbacks":false}"#,
        )
        .unwrap();
        let dump = renpy_core::engine::parse_image_dump(
            &fs::read_to_string(dir.join("images.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(dump.images.len(), 1);
        assert_eq!(dump.images[0].name, "eileen");
        assert_eq!(dump.images[0].file.as_deref(), Some("e.png"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn shot_json_keeps_the_sequence_and_the_line() {
        let shot: LiveShot =
            serde_json::from_str(r#"{"seq":3,"file":"script.rpy","line":12}"#).unwrap();
        assert_eq!(shot.seq, 3);
        assert_eq!(shot.file, "script.rpy");
        assert_eq!(shot.line, 12);
    }

    #[test]
    fn norm_script_strips_a_game_prefix() {
        assert_eq!(norm_script(r"C:\proj\game\chapter\a.rpy"), "chapter/a.rpy");
        assert_eq!(norm_script("game/script.rpy"), "script.rpy");
        assert_eq!(norm_script("None"), "");
    }

    #[test]
    fn enclosing_label_picks_the_inner_span() {
        let root = std::env::temp_dir().join(format!("vnide-live-label-{}", std::process::id()));
        let game = root.join("game");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&game).unwrap();
        fs::write(
            game.join("script.rpy"),
            "label start:\n    \"a\"\nlabel later:\n    \"c\"\n",
        )
        .unwrap();
        let project = Project::open(&root).unwrap();
        assert_eq!(
            enclosing_label(&project, "script.rpy", 2).as_deref(),
            Some("start")
        );
        assert_eq!(
            enclosing_label(&project, "script.rpy", 4).as_deref(),
            Some("later")
        );
        let _ = fs::remove_dir_all(&root);
    }
}
