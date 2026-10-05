//! Open an archive and stream entries out of it.
//!
//! `Archive` keeps the index only. Each read opens the file, then closes it,
//! so a running game is not blocked from opening the same archive.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use flate2::read::ZlibDecoder;

use super::format::{
    self, looks_like_zlib, ArchiveVersion, Entry, Header, RpaError, Segment,
    MAX_COMPRESSED_INDEX_BYTES, MAX_INDEX_BYTES,
};
use super::zix;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CopyStats {
    pub len: u64,
    pub crc32: u32,
}

#[derive(Clone, Debug)]
pub struct Archive {
    /// The file that was opened (`.rpa` or `.rpi`).
    pub path: PathBuf,
    /// Where entry bytes live. For RPA-1.0 this is the sibling `.rpa`.
    pub data_path: PathBuf,
    pub version: ArchiveVersion,
    pub key: Option<u64>,
    pub entries: BTreeMap<String, Entry>,
    pub data_len: u64,
    /// ZiX-12B prefix length. `None` for every other format.
    pub zix_prefix: Option<u64>,
}

impl Archive {
    pub fn open(path: &Path) -> Result<Self, RpaError> {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if ext == "rpi" {
            return open_v1(path);
        }
        open_rpa(path)
    }

    pub fn version_label(&self) -> String {
        self.version.label().to_string()
    }

    pub fn read_only_format(&self) -> bool {
        self.version.read_only()
    }

    pub fn get(&self, name: &str) -> Option<&Entry> {
        self.entries.get(name)
    }

    pub fn read_entry(&self, name: &str, max: u64) -> Result<Vec<u8>, RpaError> {
        let entry = self
            .entries
            .get(name)
            .ok_or_else(|| RpaError::new(format!("`{name}` is not in the archive")))?;
        let len = entry.declared_len()?;
        if len > max {
            return Err(RpaError::new(format!(
                "`{name}` is {len} bytes, over the {max} byte limit"
            )));
        }
        let mut out = Vec::with_capacity(len as usize);
        self.copy_entry(name, &mut out)?;
        Ok(out)
    }

    /// Stream one entry. The returned CRC32 covers the bytes that were written,
    /// including any prefix stored in the index. ZiX-12B deobfuscates its prefix first.
    pub fn copy_entry(&self, name: &str, out: &mut dyn Write) -> Result<CopyStats, RpaError> {
        if let Some(amount) = self.zix_prefix {
            let mut raw = Vec::new();
            self.copy_segments(name, &mut raw)?;
            if raw.len() > 512 * 1024 * 1024 {
                return Err(RpaError::named(
                    "ZiX-12B",
                    format!("`{name}` is too large to deobfuscate"),
                ));
            }
            let plain = zix::restore_prefix(&raw, amount, self.key.unwrap_or(0));
            let mut hasher = crc32fast::Hasher::new();
            hasher.update(&plain);
            out.write_all(&plain)?;
            return Ok(CopyStats {
                len: plain.len() as u64,
                crc32: hasher.finalize(),
            });
        }
        self.copy_segments(name, out)
    }

    fn copy_segments(&self, name: &str, out: &mut dyn Write) -> Result<CopyStats, RpaError> {
        let entry = self
            .entries
            .get(name)
            .ok_or_else(|| RpaError::new(format!("`{name}` is not in the archive")))?;
        let mut file = File::open(&self.data_path)?;
        let mut hasher = crc32fast::Hasher::new();
        let mut total = 0u64;
        let mut buf = vec![0u8; 64 * 1024];
        for seg in &entry.segments {
            match seg {
                Segment::Prefix(bytes) => {
                    hasher.update(bytes);
                    out.write_all(bytes)?;
                    total = total.saturating_add(bytes.len() as u64);
                }
                Segment::Range { offset, len } => {
                    let end = offset
                        .checked_add(*len)
                        .filter(|end| *end <= self.data_len)
                        .ok_or_else(|| {
                            RpaError::new(format!("`{name}` extends past the end of the archive"))
                        })?;
                    let _ = end;
                    file.seek(SeekFrom::Start(*offset))?;
                    let mut left = *len;
                    while left > 0 {
                        let n = std::cmp::min(buf.len() as u64, left) as usize;
                        file.read_exact(&mut buf[..n])?;
                        hasher.update(&buf[..n]);
                        out.write_all(&buf[..n])?;
                        left -= n as u64;
                        total += n as u64;
                    }
                }
            }
        }
        Ok(CopyStats {
            len: total,
            crc32: hasher.finalize(),
        })
    }
}

fn open_rpa(path: &Path) -> Result<Archive, RpaError> {
    let mut file = File::open(path)
        .map_err(|e| RpaError::new(format!("Could not open {}: {e}", path.display())))?;
    let data_len = file.metadata().map(|m| m.len()).unwrap_or(0);
    let head = read_header_line(&mut file)?;
    if head.is_empty() {
        return Err(RpaError::new(format!("{} is empty", path.display())));
    }
    let header = if head.starts_with(b"ZiX-12A") || head.starts_with(b"ZiX-12B") {
        zix_header(path, &head)?
    } else {
        parse_header(&head, path)?
    };
    let index_offset = header
        .index_offset
        .ok_or_else(|| RpaError::new("archive has no index offset"))?;
    if index_offset > data_len {
        return Err(RpaError::new(
            "archive index starts past the end of the file",
        ));
    }
    file.seek(SeekFrom::Start(index_offset))?;
    let pickle = decompress_limited(&mut file)?;
    let entries = finish_index(path, path, header, data_len, &pickle)?;
    Ok(entries)
}

fn open_v1(path: &Path) -> Result<Archive, RpaError> {
    let mut file = File::open(path)
        .map_err(|e| RpaError::new(format!("Could not open {}: {e}", path.display())))?;
    let mut magic = [0u8; 2];
    let n = file.read(&mut magic)?;
    if n < 2 || !looks_like_zlib(&magic) {
        return Err(RpaError::unsupported(&magic[..n]));
    }
    file.seek(SeekFrom::Start(0))?;
    let pickle = decompress_limited(&mut file)?;
    let data_path = path.with_extension("rpa");
    if !data_path.is_file() {
        return Err(RpaError::new(format!(
            "RPA-1.0 index {} has no matching {}",
            path.display(),
            data_path.display()
        )));
    }
    let data_len = std::fs::metadata(&data_path).map(|m| m.len()).unwrap_or(0);
    let header = Header {
        version: ArchiveVersion::Rpa1,
        index_offset: None,
        key: None,
        zix_prefix: None,
    };
    finish_index(path, &data_path, header, data_len, &pickle)
}

fn read_header_line(file: &mut File) -> Result<Vec<u8>, RpaError> {
    file.seek(SeekFrom::Start(0))?;
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    for _ in 0..512 {
        let n = file.read(&mut byte)?;
        if n == 0 {
            break;
        }
        buf.push(byte[0]);
        if byte[0] == b'\n' {
            break;
        }
    }
    Ok(buf)
}

fn zix_header(path: &Path, line: &[u8]) -> Result<Header, RpaError> {
    let zix12b = line.starts_with(b"ZiX-12B");
    let version = if zix12b {
        ArchiveVersion::Zix12b
    } else {
        ArchiveVersion::Zix12a
    };
    let params = zix::load_params(path, zix12b)?;
    let parts: Vec<&[u8]> = line
        .split(|b| *b == b' ' || *b == b'\n' || *b == b'\r')
        .filter(|p| !p.is_empty())
        .collect();
    let token = parts.last().copied().unwrap_or(b"");
    let offset = zix::offset_from_token(token)?;
    Ok(Header {
        version,
        index_offset: Some(offset),
        key: Some(params.key),
        zix_prefix: params.prefix,
    })
}

fn parse_header(buf: &[u8], path: &Path) -> Result<Header, RpaError> {
    match format::parse_rpa_header(buf) {
        Ok(h) => Ok(h),
        Err(e) => {
            if looks_like_zlib(buf) {
                return Err(RpaError::new(format!(
                    "{} looks like an RPA-1.0 index. Open the .rpi file next to it.",
                    path.display()
                )));
            }
            Err(e)
        }
    }
}

fn finish_index(
    path: &Path,
    data_path: &Path,
    header: Header,
    data_len: u64,
    pickle: &[u8],
) -> Result<Archive, RpaError> {
    let decoded = format::decode_index(pickle, header.key)?;
    let mut entries = BTreeMap::new();
    for (name, entry) in decoded {
        check_bounds(&name, &entry, data_len)?;
        entries.insert(name, entry);
    }
    Ok(Archive {
        path: path.to_path_buf(),
        data_path: data_path.to_path_buf(),
        version: header.version,
        key: header.key,
        entries,
        data_len,
        zix_prefix: header.zix_prefix,
    })
}

fn check_bounds(name: &str, entry: &Entry, data_len: u64) -> Result<(), RpaError> {
    for seg in &entry.segments {
        if let Segment::Range { offset, len } = seg {
            let end = offset
                .checked_add(*len)
                .ok_or_else(|| RpaError::new(format!("`{name}` has a bad range")))?;
            if end > data_len {
                return Err(RpaError::new(format!(
                    "`{name}` points at bytes {offset}+{len}, past the end of the archive ({data_len})"
                )));
            }
        }
    }
    let _ = entry.declared_len()?;
    Ok(())
}

/// Read the rest of `file` (the index sits at the end) and zlib-decompress it.
/// Both the compressed and decompressed sizes are capped.
fn decompress_limited(file: &mut File) -> Result<Vec<u8>, RpaError> {
    let mut compressed = Vec::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        if compressed.len().saturating_add(n) > MAX_COMPRESSED_INDEX_BYTES {
            return Err(RpaError::new(format!(
                "compressed archive index is larger than {} MiB",
                MAX_COMPRESSED_INDEX_BYTES >> 20
            )));
        }
        compressed.extend_from_slice(&buf[..n]);
    }
    let mut dec = ZlibDecoder::new(compressed.as_slice());
    let mut out = Vec::new();
    loop {
        let n = dec
            .read(&mut buf)
            .map_err(|e| RpaError::new(format!("archive index could not be decompressed: {e}")))?;
        if n == 0 {
            break;
        }
        if out.len().saturating_add(n) > MAX_INDEX_BYTES {
            return Err(RpaError::new(format!(
                "archive index is larger than {} MiB once decompressed",
                MAX_INDEX_BYTES >> 20
            )));
        }
        out.extend_from_slice(&buf[..n]);
    }
    if out.is_empty() {
        return Err(RpaError::new("archive index is empty"));
    }
    Ok(out)
}
