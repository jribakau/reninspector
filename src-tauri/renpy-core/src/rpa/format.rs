//! Headers and index records for Ren'Py archives.
//!
//! Official RPA-1.0, RPA-2.0 and RPA-3.0, plus the variants unrpa reads:
//! RPA-3.2, RPA-4.0, ALT-1.0, ZiX-12A and ZiX-12B. Writing stays RPA-3.0.

use std::fmt;

use crate::pickle::{self, Policy, Value};

/// Compressed index cap. A real index is a few megabytes; this bounds the read from disk.
pub const MAX_COMPRESSED_INDEX_BYTES: usize = 64 * 1024 * 1024;
/// Decompressed index cap (about 1.5M entries); this stops a zlib bomb.
pub const MAX_INDEX_BYTES: usize = 128 * 1024 * 1024;
pub const MAX_ENTRIES: usize = 2_000_000;
const MAX_PREFIX: usize = 16 * 1024 * 1024;

#[derive(Debug)]
pub struct RpaError {
    pub message: String,
    /// Set when the header names a format, even if the rest could not be read.
    pub known_version: Option<String>,
}

impl RpaError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            known_version: None,
        }
    }

    pub fn named(version: &str, message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            known_version: Some(version.to_string()),
        }
    }

    pub fn unsupported(header: &[u8]) -> Self {
        let n = header.len().min(24);
        let text = String::from_utf8_lossy(&header[..n]);
        let hex = header[..n.min(16)]
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<Vec<_>>()
            .join(" ");
        let known = header_token(header);
        let mut err = Self::new(format!(
            "unsupported archive header {text:?} ({hex}). Only RPA-1.0 (.rpi), RPA-2.0, RPA-3.0, RPA-3.2, RPA-4.0, ALT-1.0, ZiX-12A and ZiX-12B can be read."
        ));
        err.known_version = known;
        err
    }
}

impl fmt::Display for RpaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for RpaError {}

impl From<crate::pickle::PickleError> for RpaError {
    fn from(e: crate::pickle::PickleError) -> Self {
        Self::new(e.0)
    }
}

impl From<std::io::Error> for RpaError {
    fn from(e: std::io::Error) -> Self {
        if e.raw_os_error() == Some(32) {
            Self::new("The file is in use by another program (the game may be running).")
        } else {
            Self::new(e.to_string())
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArchiveVersion {
    Rpa1,
    Rpa2,
    Rpa3,
    Rpa32,
    Rpa4,
    Alt1,
    Zix12a,
    Zix12b,
    /// A header token we can name but not decode.
    Custom(String),
}

impl ArchiveVersion {
    pub fn label(&self) -> &str {
        match self {
            ArchiveVersion::Rpa1 => "RPA-1.0",
            ArchiveVersion::Rpa2 => "RPA-2.0",
            ArchiveVersion::Rpa3 => "RPA-3.0",
            ArchiveVersion::Rpa32 => "RPA-3.2",
            ArchiveVersion::Rpa4 => "RPA-4.0",
            ArchiveVersion::Alt1 => "ALT-1.0",
            ArchiveVersion::Zix12a => "ZiX-12A",
            ArchiveVersion::Zix12b => "ZiX-12B",
            ArchiveVersion::Custom(name) => name.as_str(),
        }
    }

    /// Official archives can be the source of a patch. The others are read and extracted only.
    pub fn read_only(&self) -> bool {
        !matches!(
            self,
            ArchiveVersion::Rpa1 | ArchiveVersion::Rpa2 | ArchiveVersion::Rpa3
        )
    }
}

/// Where the bytes of one piece of an entry live.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Segment {
    /// Bytes stored in the index itself. They come before the following range.
    Prefix(Vec<u8>),
    Range {
        offset: u64,
        len: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub segments: Vec<Segment>,
}

impl Entry {
    pub fn declared_len(&self) -> Result<u64, RpaError> {
        let mut total = 0u64;
        for seg in &self.segments {
            let n = match seg {
                Segment::Prefix(b) => b.len() as u64,
                Segment::Range { len, .. } => *len,
            };
            total = total
                .checked_add(n)
                .ok_or_else(|| RpaError::new("entry length overflows"))?;
        }
        Ok(total)
    }
}

#[derive(Debug)]
pub struct Header {
    pub version: ArchiveVersion,
    /// Present for every format except RPA-1.0, whose index is the whole `.rpi`.
    pub index_offset: Option<u64>,
    pub key: Option<u64>,
    /// ZiX-12B: deobfuscate this many bytes at the start of each extracted file.
    pub zix_prefix: Option<u64>,
}

/// Name the format from the start of the file, without checking that it can be decoded.
pub fn detect_version(buf: &[u8]) -> ArchiveVersion {
    if buf.starts_with(b"RPA-3.0 ") {
        ArchiveVersion::Rpa3
    } else if buf.starts_with(b"RPA-3.2 ") || buf.starts_with(b"RPA-3.2\n") {
        ArchiveVersion::Rpa32
    } else if buf.starts_with(b"RPA-4.0 ") || buf.starts_with(b"RPA-4.0\n") {
        ArchiveVersion::Rpa4
    } else if buf.starts_with(b"RPA-2.0 ") {
        ArchiveVersion::Rpa2
    } else if buf.starts_with(b"ALT-1.0 ") || buf.starts_with(b"ALT-1.0\n") {
        ArchiveVersion::Alt1
    } else if buf.starts_with(b"ZiX-12A") {
        ArchiveVersion::Zix12a
    } else if buf.starts_with(b"ZiX-12B") {
        ArchiveVersion::Zix12b
    } else if let Some(token) = header_token(buf) {
        ArchiveVersion::Custom(token)
    } else {
        ArchiveVersion::Custom("unknown".into())
    }
}

/// Parse the first line of an `.rpa` the way unrpa does.
/// ZiX headers are handled by [`super::zix`] because the key lives in another file.
pub fn parse_rpa_header(buf: &[u8]) -> Result<Header, RpaError> {
    let version = detect_version(buf);
    match &version {
        ArchiveVersion::Rpa3 | ArchiveVersion::Rpa32 | ArchiveVersion::Rpa4 => {
            let (offset, key) = rpa3_offset_and_key(buf, version.label())?;
            Ok(Header {
                version,
                index_offset: Some(offset),
                key: Some(key),
                zix_prefix: None,
            })
        }
        ArchiveVersion::Rpa2 => {
            let offset = fixed_hex(buf, 8, 16, "RPA-2.0")?;
            Ok(Header {
                version,
                index_offset: Some(offset),
                key: None,
                zix_prefix: None,
            })
        }
        ArchiveVersion::Alt1 => {
            // unrpa: key is field 1 XORed with 0xDABE8DF0, offset is field 2.
            let parts = header_parts(buf);
            if parts.len() < 3 {
                return Err(RpaError::named(
                    "ALT-1.0",
                    "ALT-1.0 header is missing an offset and key",
                ));
            }
            let stored = parse_hex_u64(parts[1].as_bytes()).map_err(|_| {
                RpaError::named("ALT-1.0", format!("ALT-1.0 key {:?} is not hex", parts[1]))
            })?;
            let offset = parse_hex_u64(parts[2].as_bytes()).map_err(|_| {
                RpaError::named(
                    "ALT-1.0",
                    format!("ALT-1.0 offset {:?} is not hex", parts[2]),
                )
            })?;
            Ok(Header {
                version,
                index_offset: Some(offset),
                key: Some(stored ^ 0xDABE8DF0),
                zix_prefix: None,
            })
        }
        ArchiveVersion::Zix12a | ArchiveVersion::Zix12b => Err(RpaError::named(
            version.label(),
            format!("{} needs its loader file", version.label()),
        )),
        ArchiveVersion::Rpa1 => Err(RpaError::new(
            "RPA-1.0 indexes are .rpi files, not this header",
        )),
        ArchiveVersion::Custom(_) => Err(RpaError::unsupported(buf)),
    }
}

/// RPA-3.0 field order. RPA-3.2 and RPA-4.0 use the same order (unrpa subclasses RPA-3.0).
/// A real RPA-3.2 sample inserts a short extra token before the key
/// (`RPA-3.2 <offset> F <key>`, unrpa issue 18); the key is the last field then.
fn rpa3_offset_and_key(buf: &[u8], label: &str) -> Result<(u64, u64), RpaError> {
    let parts = header_parts(buf);
    if parts.len() < 3 {
        return Err(RpaError::named(
            label,
            format!("{label} header is missing an offset and key"),
        ));
    }
    let offset = parse_hex_u64(parts[1].as_bytes())
        .map_err(|_| RpaError::named(label, format!("{label} offset {:?} is not hex", parts[1])))?;
    let mut key_text = parts[2].as_str();
    if parts.len() > 3 && key_text.len() < 8 {
        if let Some(last) = parts.last() {
            if last.len() >= 2 && last.bytes().all(|b| b.is_ascii_hexdigit()) {
                key_text = last.as_str();
            }
        }
    }
    let key = parse_hex_u64(key_text.as_bytes())
        .map_err(|_| RpaError::named(label, format!("{label} key {key_text:?} is not hex")))?;
    Ok((offset, key))
}

fn header_parts(buf: &[u8]) -> Vec<String> {
    let line = buf
        .split(|b| *b == b'\n' || *b == b'\r')
        .next()
        .unwrap_or(buf);
    let text = String::from_utf8_lossy(line);
    text.split_whitespace().map(|s| s.to_string()).collect()
}

fn fixed_hex(buf: &[u8], at: usize, len: usize, label: &str) -> Result<u64, RpaError> {
    if buf.len() < at + len {
        return Err(RpaError::named(
            label,
            format!("{label} header is truncated"),
        ));
    }
    parse_hex_u64(&buf[at..at + len])
        .map_err(|_| RpaError::named(label, format!("{label} header offset is not hex")))
}

fn header_token(buf: &[u8]) -> Option<String> {
    let line = buf
        .split(|b| *b == b'\n' || *b == b'\r')
        .next()
        .unwrap_or(buf);
    let text = std::str::from_utf8(line).ok()?.trim();
    let token = text.split_whitespace().next()?;
    if (3..=32).contains(&token.len())
        && token.contains('-')
        && token.bytes().all(|b| b.is_ascii_graphic())
    {
        Some(token.to_string())
    } else {
        None
    }
}

pub fn write_v3_header(offset: u64, key: u32) -> [u8; 34] {
    let s = format!("RPA-3.0 {offset:016x} {key:08x}\n");
    let bytes = s.into_bytes();
    let mut out = [0u8; 34];
    out.copy_from_slice(&bytes);
    out
}

fn parse_hex_u64(bytes: &[u8]) -> Result<u64, RpaError> {
    let text = std::str::from_utf8(bytes).map_err(|_| RpaError::new("header offset is not hex"))?;
    u64::from_str_radix(text.trim(), 16)
        .map_err(|_| RpaError::new(format!("header offset {text:?} is not hex")))
}

/// True when `buf` looks like a zlib stream (an RPA-1.0 `.rpi`).
pub fn looks_like_zlib(buf: &[u8]) -> bool {
    buf.len() >= 2 && buf[0] == 0x78 && matches!(buf[1], 0x01 | 0x5e | 0x9c | 0xda)
}

pub fn decode_index(
    pickle_bytes: &[u8],
    key: Option<u64>,
) -> Result<Vec<(String, Entry)>, RpaError> {
    let value = pickle::load(pickle_bytes, &Policy::ARCHIVE_INDEX)?;
    let Value::Dict(pairs) = value else {
        return Err(RpaError::new("archive index is not a dict"));
    };
    if pairs.len() > MAX_ENTRIES {
        return Err(RpaError::new("archive index has too many entries"));
    }
    let mut out = Vec::with_capacity(pairs.len());
    for (k, v) in pairs {
        let name = key_to_string(k)?;
        if name.len() > 4096 {
            return Err(RpaError::new("archive entry name is too long"));
        }
        out.push((name, decode_entry(v, key)?));
    }
    Ok(out)
}

fn key_to_string(v: Value) -> Result<String, RpaError> {
    match v {
        Value::Str(s) => Ok(s),
        Value::Bytes(b) => {
            if let Ok(s) = String::from_utf8(b.clone()) {
                Ok(s)
            } else {
                Ok(b.iter().map(|c| *c as char).collect())
            }
        }
        _ => Err(RpaError::new("archive index key is not a string")),
    }
}

fn decode_entry(v: Value, key: Option<u64>) -> Result<Entry, RpaError> {
    let items = match v {
        Value::List(items) | Value::Tuple(items) => items,
        _ => return Err(RpaError::new("archive index entry is not a list")),
    };
    let mut segments = Vec::new();
    for item in items {
        let parts = match item {
            Value::List(p) | Value::Tuple(p) => p,
            _ => return Err(RpaError::new("archive index record is not a tuple")),
        };
        match parts.as_slice() {
            [off, len] => {
                segments.push(Segment::Range {
                    offset: unxor(as_int(off)?, key)?,
                    len: unxor(as_int(len)?, key)?,
                });
            }
            [off, len, start] => {
                let prefix = prefix_bytes(start)?;
                if !prefix.is_empty() {
                    if prefix.len() > MAX_PREFIX {
                        return Err(RpaError::new("archive prefix is too large"));
                    }
                    segments.push(Segment::Prefix(prefix));
                }
                segments.push(Segment::Range {
                    offset: unxor(as_int(off)?, key)?,
                    len: unxor(as_int(len)?, key)?,
                });
            }
            [Value::Bytes(b)] => {
                if b.len() > MAX_PREFIX {
                    return Err(RpaError::new("archive prefix is too large"));
                }
                segments.push(Segment::Prefix(b.clone()));
            }
            _ => {
                return Err(RpaError::new(
                    "archive index record must be (offset, length) or (offset, length, prefix)",
                ));
            }
        }
    }
    Ok(Entry { segments })
}

fn as_int(v: &Value) -> Result<i128, RpaError> {
    match v {
        Value::Int(n) => Ok(*n),
        Value::Bool(true) => Ok(1),
        Value::Bool(false) => Ok(0),
        _ => Err(RpaError::new("archive offset is not an integer")),
    }
}

fn unxor(n: i128, key: Option<u64>) -> Result<u64, RpaError> {
    let v = match key {
        Some(k) => n ^ (k as i128),
        None => n,
    };
    if v < 0 {
        return Err(RpaError::new("archive offset is negative"));
    }
    u64::try_from(v).map_err(|_| RpaError::new("archive offset does not fit in 64 bits"))
}

fn prefix_bytes(v: &Value) -> Result<Vec<u8>, RpaError> {
    match v {
        Value::Bytes(b) => Ok(b.clone()),
        Value::Str(s) => pickle::latin1_bytes(s).map_err(RpaError::from),
        Value::None => Ok(Vec::new()),
        _ => Err(RpaError::new("archive prefix is not bytes")),
    }
}

/// Reject names that would escape the destination directory.
pub fn safe_relative(name: &str) -> Result<std::path::PathBuf, RpaError> {
    if name.is_empty() || name.contains('\0') {
        return Err(RpaError::new(format!("unsafe archive path {name:?}")));
    }
    if name.starts_with('/')
        || name.starts_with('\\')
        || name.starts_with("//")
        || name.starts_with("\\\\")
    {
        return Err(RpaError::new(format!("unsafe archive path {name:?}")));
    }
    let bytes = name.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' {
        return Err(RpaError::new(format!("unsafe archive path {name:?}")));
    }
    let mut out = std::path::PathBuf::new();
    for comp in name.split(['/', '\\']) {
        if comp.is_empty() || comp == "." || comp == ".." {
            return Err(RpaError::new(format!("unsafe archive path {name:?}")));
        }
        if comp.ends_with('.') || comp.ends_with(' ') || is_reserved(comp) {
            return Err(RpaError::new(format!("unsafe archive path {name:?}")));
        }
        out.push(comp);
    }
    Ok(out)
}

fn is_reserved(comp: &str) -> bool {
    let base = comp.split('.').next().unwrap_or(comp);
    let u = base.to_ascii_uppercase();
    matches!(u.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (u.len() == 4
            && (u.starts_with("COM") || u.starts_with("LPT"))
            && u.as_bytes()[3].is_ascii_digit()
            && u.as_bytes()[3] != b'0')
}
