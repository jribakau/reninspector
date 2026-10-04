//! Headers and index records for RPA-1.0, RPA-2.0 and RPA-3.0.

use std::fmt;

use super::pickle::{self, Value};

/// Compressed index cap. A real index is a few megabytes; this bounds the read from disk.
pub const MAX_COMPRESSED_INDEX_BYTES: usize = 64 * 1024 * 1024;
/// Decompressed index cap (about 1.5M entries); this stops a zlib bomb.
pub const MAX_INDEX_BYTES: usize = 128 * 1024 * 1024;
pub const MAX_ENTRIES: usize = 2_000_000;
const MAX_PREFIX: usize = 16 * 1024 * 1024;

#[derive(Debug)]
pub struct RpaError {
    pub message: String,
}

impl RpaError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub fn unsupported(header: &[u8]) -> Self {
        let n = header.len().min(16);
        let text = String::from_utf8_lossy(&header[..n]);
        let hex = header[..n]
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<Vec<_>>()
            .join(" ");
        Self::new(format!(
            "unsupported archive header {text:?} ({hex}). Only RPA-1.0 (.rpi), RPA-2.0 and RPA-3.0 can be read."
        ))
    }
}

impl fmt::Display for RpaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for RpaError {}

impl From<std::io::Error> for RpaError {
    fn from(e: std::io::Error) -> Self {
        if e.raw_os_error() == Some(32) {
            Self::new("The file is in use by another program (the game may be running).")
        } else {
            Self::new(e.to_string())
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveVersion {
    V1,
    V2,
    V3,
}

impl ArchiveVersion {
    pub fn label(self) -> &'static str {
        match self {
            ArchiveVersion::V1 => "RPA-1.0",
            ArchiveVersion::V2 => "RPA-2.0",
            ArchiveVersion::V3 => "RPA-3.0",
        }
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
    /// Present for v2 and v3. v1's index is the whole file.
    pub index_offset: Option<u64>,
    pub key: Option<u32>,
}

/// Parse the start of an `.rpa` file the same way `renpy/loader.py` does.
pub fn parse_rpa_header(buf: &[u8]) -> Result<Header, RpaError> {
    if buf.starts_with(b"RPA-3.0 ") {
        if buf.len() < 34 {
            return Err(RpaError::new("RPA-3.0 header is truncated"));
        }
        let offset = parse_hex_u64(&buf[8..24])?;
        if buf[24] != b' ' {
            return Err(RpaError::unsupported(buf));
        }
        let key = parse_hex_u32(&buf[25..33])?;
        return Ok(Header {
            version: ArchiveVersion::V3,
            index_offset: Some(offset),
            key: Some(key),
        });
    }
    if buf.starts_with(b"RPA-2.0 ") {
        if buf.len() < 24 {
            return Err(RpaError::new("RPA-2.0 header is truncated"));
        }
        let offset = parse_hex_u64(&buf[8..24])?;
        return Ok(Header {
            version: ArchiveVersion::V2,
            index_offset: Some(offset),
            key: None,
        });
    }
    Err(RpaError::unsupported(buf))
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

fn parse_hex_u32(bytes: &[u8]) -> Result<u32, RpaError> {
    let text = std::str::from_utf8(bytes).map_err(|_| RpaError::new("header key is not hex"))?;
    u32::from_str_radix(text.trim(), 16)
        .map_err(|_| RpaError::new(format!("header key {text:?} is not hex")))
}

/// True when `buf` looks like a zlib stream (an RPA-1.0 `.rpi`).
pub fn looks_like_zlib(buf: &[u8]) -> bool {
    buf.len() >= 2 && buf[0] == 0x78 && matches!(buf[1], 0x01 | 0x5e | 0x9c | 0xda)
}

pub fn decode_index(
    pickle_bytes: &[u8],
    key: Option<u32>,
) -> Result<Vec<(String, Entry)>, RpaError> {
    let value = pickle::loads(pickle_bytes)?;
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

fn decode_entry(v: Value, key: Option<u32>) -> Result<Entry, RpaError> {
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

fn unxor(n: i128, key: Option<u32>) -> Result<u64, RpaError> {
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
        Value::Str(s) => pickle::latin1_bytes(s),
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
