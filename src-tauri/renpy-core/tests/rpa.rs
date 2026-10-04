//! Archive fixtures (built by `tests/fixtures/rpa/make_fixtures.py`) plus
//! round-trips of the writer. These tests do not need Python.

use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use flate2::write::ZlibEncoder;
use flate2::Compression;
use renpy_core::rpa::{
    build_from_dir, extract, patch_stem, Archive, ArchiveWriter, ExtractOptions, RpaError,
};

const KEY: u32 = 0x11223344;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/rpa")
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("vn-ide-rpa-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn wrap_v3(pickle: &[u8], payload: &[u8]) -> Vec<u8> {
    let mut enc = ZlibEncoder::new(Vec::new(), Compression::new(6));
    enc.write_all(pickle).unwrap();
    let compressed = enc.finish().unwrap();
    let offset = 34 + payload.len() as u64;
    let mut out = format!("RPA-3.0 {offset:016x} {KEY:08x}\n").into_bytes();
    assert_eq!(out.len(), 34);
    out.extend_from_slice(payload);
    out.extend_from_slice(&compressed);
    out
}

fn read_named(archive: &Archive, name: &str) -> String {
    let bytes = archive.read_entry(name, 1024).unwrap();
    String::from_utf8(bytes).unwrap()
}

#[test]
fn python_protocol_indexes_decode() {
    let payload = b"HELLONOTE";
    for proto in [0, 2, 4, 5] {
        let pickle = fs::read(fixtures().join(format!("index-proto{proto}.pickle"))).unwrap();
        let bytes = wrap_v3(&pickle, payload);
        let dir = temp_dir(&format!("proto{proto}"));
        let path = dir.join("a.rpa");
        fs::write(&path, &bytes).unwrap();
        let archive = Archive::open(&path).unwrap_or_else(|e| panic!("proto {proto}: {e}"));
        assert_eq!(read_named(&archive, "dir/script.rpy"), "HELLO");
        assert_eq!(read_named(&archive, "note.txt"), "PRE:NOTE");
        assert_eq!(read_named(&archive, "multi.txt"), "HELLONOTE");
        let _ = fs::remove_dir_all(dir);
    }
}

#[test]
fn python2_style_bytes_key() {
    let pickle = fs::read(fixtures().join("index-bytes-key.pickle")).unwrap();
    let bytes = wrap_v3(&pickle, b"HELLO");
    let dir = temp_dir("byteskey");
    let path = dir.join("a.rpa");
    fs::write(&path, &bytes).unwrap();
    let archive = Archive::open(&path).unwrap();
    assert_eq!(read_named(&archive, "script.rpy"), "HELLO");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn v2_and_v1_and_shipped_v3() {
    let v3 = Archive::open(&fixtures().join("v3-proto2.rpa")).unwrap();
    assert_eq!(v3.version_label(), "RPA-3.0");
    assert_eq!(read_named(&v3, "dir/script.rpy"), "HELLO");
    assert_eq!(read_named(&v3, "note.txt"), "PRE:NOTE");

    let v2 = Archive::open(&fixtures().join("v2.rpa")).unwrap();
    assert_eq!(v2.version_label(), "RPA-2.0");
    assert_eq!(read_named(&v2, "dir/script.rpy"), "HELLO");
    assert_eq!(read_named(&v2, "note.txt"), "PRE:NOTE");

    let v1 = Archive::open(&fixtures().join("v1.rpi")).unwrap();
    assert_eq!(v1.version_label(), "RPA-1.0");
    assert_eq!(read_named(&v1, "dir/script.rpy"), "HELLO");
    assert_eq!(read_named(&v1, "note.txt"), "NOTE");
}

#[test]
fn unknown_header_names_itself() {
    let err = Archive::open(&fixtures().join("custom.rpa")).unwrap_err();
    let text = err.to_string();
    assert!(text.contains("RPA-3.2"), "{text}");
    assert!(text.contains("unsupported"), "{text}");
}

#[test]
fn writer_round_trip_matches_bytes_and_crc() {
    let dir = temp_dir("round");
    let target = dir.join("out.rpa");
    let mut w = ArchiveWriter::create(&target).unwrap();
    w.add_bytes("script.rpy", b"label start:\n    return\n")
        .unwrap();
    w.add_bytes("empty.rpy", b"").unwrap();
    w.finish().unwrap();
    assert!(target.is_file());
    assert!(!dir.join(".out.rpa.vnide-tmp").exists());

    let archive = Archive::open(&target).unwrap();
    assert_eq!(archive.version_label(), "RPA-3.0");
    let body = archive.read_entry("script.rpy", 1024).unwrap();
    assert_eq!(body, b"label start:\n    return\n");
    let stats = archive
        .copy_entry("empty.rpy", &mut std::io::sink())
        .unwrap();
    assert_eq!(stats.len, 0);

    // A second build replaces the file and still reads back.
    let mut w = ArchiveWriter::create(&target).unwrap();
    w.add_bytes("script.rpy", b"changed\n").unwrap();
    w.finish().unwrap();
    let archive = Archive::open(&target).unwrap();
    assert_eq!(archive.read_entry("script.rpy", 100).unwrap(), b"changed\n");
    assert!(archive.get("empty.rpy").is_none());
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn build_from_dir_round_trip() {
    let dir = temp_dir("build");
    let src = dir.join("src");
    fs::create_dir_all(src.join("sub")).unwrap();
    fs::write(src.join("a.rpy"), b"one").unwrap();
    fs::write(src.join("sub/b.txt"), b"two").unwrap();
    let target = dir.join("pack.rpa");
    let report = build_from_dir(&src, &target, None).unwrap();
    assert_eq!(report.files, 2);
    let archive = Archive::open(&target).unwrap();
    assert_eq!(archive.read_entry("a.rpy", 10).unwrap(), b"one");
    assert_eq!(archive.read_entry("sub/b.txt", 10).unwrap(), b"two");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn unsafe_names_are_refused() {
    let dir = temp_dir("unsafe");
    let target = dir.join("a.rpa");
    let mut w = ArchiveWriter::create(&target).unwrap();
    for bad in [
        "../x",
        "/abs",
        "C:/x",
        "foo/../../x",
        "CON",
        "dir/NUL.txt",
        "a/./b",
    ] {
        let err = w.add_bytes(bad, b"x").unwrap_err();
        assert!(err.to_string().contains("unsafe"), "{bad}: {err}");
    }
    drop(w);
    assert!(
        !target.exists(),
        "a failed writer must not replace the target"
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn extract_rejects_escape_and_case_collisions() {
    let dir = temp_dir("extract");
    let err = extract(
        &Archive::open(&fixtures().join("escape.rpa")).unwrap(),
        &dir.join("out"),
        &ExtractOptions::default(),
    )
    .unwrap_err();
    assert!(err.to_string().contains("unsafe"), "{err}");
    assert!(!dir.join("out").join("evil.txt").exists());

    let target = dir.join("case.rpa");
    let mut w = ArchiveWriter::create(&target).unwrap();
    // The writer accepts both; extraction must not pick one silently.
    w.add_bytes("Readme.txt", b"A").unwrap();
    w.add_bytes("readme.txt", b"B").unwrap();
    w.finish().unwrap();
    let err = extract(
        &Archive::open(&target).unwrap(),
        &dir.join("case-out"),
        &ExtractOptions::default(),
    )
    .unwrap_err();
    assert!(err.to_string().contains("collides"), "{err}");

    let mut only = HashSet::new();
    only.insert("Readme.txt".into());
    let report = extract(
        &Archive::open(&target).unwrap(),
        &dir.join("one"),
        &ExtractOptions {
            names: Some(&only),
            overwrite: false,
            progress: None,
            cancel: None,
        },
    )
    .unwrap();
    assert_eq!(report.files, 1);
    assert_eq!(fs::read(dir.join("one/Readme.txt")).unwrap(), b"A");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn cancel_stops_a_build() {
    let dir = temp_dir("cancel");
    fs::write(dir.join("a.txt"), b"hi").unwrap();
    let flag = AtomicBool::new(true);
    let err = build_from_dir(&dir, &dir.join("out.rpa"), Some(&flag)).unwrap_err();
    assert!(err.to_string().contains("cancelled"), "{err}");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn truncations_and_bitflips_do_not_panic() {
    let path = fixtures().join("v3-proto2.rpa");
    let bytes = fs::read(&path).unwrap();
    for n in 0..bytes.len() {
        let result = std::panic::catch_unwind(|| {
            let dir =
                std::env::temp_dir().join(format!("vn-ide-rpa-trunc-{}-{n}", std::process::id()));
            let _ = fs::create_dir_all(&dir);
            let p = dir.join("t.rpa");
            let _ = fs::write(&p, &bytes[..n]);
            let opened = Archive::open(&p);
            if let Ok(archive) = &opened {
                for name in archive.entries.keys() {
                    let _ = archive.read_entry(name, 10_000_000);
                }
            }
            opened.is_err()
        });
        assert!(result.is_ok(), "truncation {n} panicked");
        assert!(result.unwrap(), "truncation {n} was accepted");
        let _ = fs::remove_dir_all(
            std::env::temp_dir().join(format!("vn-ide-rpa-trunc-{}-{n}", std::process::id())),
        );
    }

    // Flips in the header and the compressed index must not panic. zlib's
    // checksum makes almost all of them fail to open; a flip that still
    // parses has to read without panicking.
    let mut state = 0x1234_5678u32;
    for i in 0..4_000 {
        let bit_pos = (xorshift(&mut state) as usize) % (bytes.len() * 8);
        let mut flipped = bytes.clone();
        flipped[bit_pos / 8] ^= 1 << (bit_pos % 8);
        let result = std::panic::catch_unwind(|| {
            let dir =
                std::env::temp_dir().join(format!("vn-ide-rpa-flip-{}-{i}", std::process::id()));
            let _ = fs::create_dir_all(&dir);
            let p = dir.join("t.rpa");
            let _ = fs::write(&p, &flipped);
            if let Ok(archive) = Archive::open(&p) {
                for name in archive.entries.keys() {
                    let _ = archive.read_entry(name, 10_000_000);
                }
            }
        });
        assert!(result.is_ok(), "bit flip {i} panicked");
        let _ = fs::remove_dir_all(
            std::env::temp_dir().join(format!("vn-ide-rpa-flip-{}-{i}", std::process::id())),
        );
    }
}

fn xorshift(state: &mut u32) -> u32 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *state = x;
    x
}

#[test]
fn patch_stem_orders_after_real_names() {
    let name = patch_stem(["archive", "scripts", "zz_sounds"]);
    let mut all = vec![
        "archive".to_string(),
        "scripts".into(),
        "zz_sounds".into(),
        name.clone(),
    ];
    all.sort();
    assert_eq!(all.last().unwrap(), &name);
}

#[test]
fn python_can_read_what_we_write() {
    let py = python_cmd();
    let Some(py) = py else {
        eprintln!("python not on PATH; skipping");
        return;
    };
    let dir = temp_dir("pyread");
    let target = dir.join("out.rpa");
    let mut w = ArchiveWriter::create(&target).unwrap();
    w.add_bytes("script.rpy", b"label start:\n").unwrap();
    w.finish().unwrap();
    let script = r#"
import pickle, sys, zlib
p = open(sys.argv[1], "rb").read()
off = int(p[8:24], 16)
key = int(p[25:33], 16)
index = pickle.loads(zlib.decompress(p[off:]))
name, recs = next(iter(index.items()))
off2, ln = recs[0][0] ^ key, recs[0][1] ^ key
print(name)
print(p[off2:off2 + ln].decode())
"#;
    let out = std::process::Command::new(&py)
        .arg("-c")
        .arg(script)
        .arg(&target)
        .output()
        .expect("python");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("script.rpy"), "{text}");
    assert!(text.contains("label start:"), "{text}");
    let _ = fs::remove_dir_all(dir);
}

fn python_cmd() -> Option<String> {
    for cmd in ["py", "python", "python3"] {
        if let Ok(out) = std::process::Command::new(cmd).arg("--version").output() {
            if out.status.success() {
                return Some(cmd.into());
            }
        }
    }
    None
}

#[allow(dead_code)]
fn _use_error_type(e: RpaError) -> String {
    e.to_string()
}

#[allow(dead_code)]
fn _path(p: &Path) -> &Path {
    p
}
