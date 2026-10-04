//! Write an RPA-3.0 archive, then prove the file we wrote can be read back
//! before it replaces anything.

use std::fs::{self, File};
use std::hash::{BuildHasher, Hash, Hasher};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::SystemTime;

use flate2::write::ZlibEncoder;
use flate2::Compression;

use super::format::{safe_relative, write_v3_header, RpaError};
use super::pickle::{self, IndexPair};
use super::reader::Archive;

#[derive(Debug)]
pub struct BuildReport {
    pub files: u32,
    pub bytes: u64,
}

struct Planned {
    name: String,
    offset: u64,
    len: u64,
    crc32: u32,
}

pub struct ArchiveWriter {
    temp: PathBuf,
    target: PathBuf,
    file: Option<File>,
    key: u32,
    offset: u64,
    planned: Vec<Planned>,
}

impl ArchiveWriter {
    /// Start an archive that will replace `target` only after it verifies.
    pub fn create(target: &Path) -> Result<Self, RpaError> {
        if target.as_os_str().is_empty() {
            return Err(RpaError::new("archive path is empty"));
        }
        let parent = target
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)?;
        let name = target
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "archive.rpa".into());
        let temp = parent.join(format!(".{name}.vnide-tmp"));
        let mut file = File::create(&temp)?;
        let key = random_key();
        file.write_all(&write_v3_header(0, key))?;
        Ok(Self {
            temp,
            target: target.to_path_buf(),
            file: Some(file),
            key,
            offset: 34,
            planned: Vec::new(),
        })
    }

    pub fn add_bytes(&mut self, name: &str, data: &[u8]) -> Result<(), RpaError> {
        self.add_reader(name, &mut &data[..])
    }

    pub fn add_file(&mut self, name: &str, path: &Path) -> Result<(), RpaError> {
        let mut file = File::open(path)
            .map_err(|e| RpaError::new(format!("Could not read {}: {e}", path.display())))?;
        self.add_reader(name, &mut file)
    }

    pub fn add_reader(&mut self, name: &str, reader: &mut dyn Read) -> Result<(), RpaError> {
        let name = normalize_name(name)?;
        if self.planned.iter().any(|p| p.name == name) {
            return Err(RpaError::new(format!("duplicate archive entry `{name}`")));
        }
        let start = self.offset;
        let mut hasher = crc32fast::Hasher::new();
        let mut len = 0u64;
        let mut buf = [0u8; 64 * 1024];
        let mut out = std::io::BufWriter::new(self.file.as_mut().expect("archive file"));
        loop {
            let n = reader.read(&mut buf)?;
            if n == 0 {
                break;
            }
            out.write_all(&buf[..n])?;
            hasher.update(&buf[..n]);
            len += n as u64;
        }
        out.flush()?;
        self.offset = self.offset.saturating_add(len);
        self.planned.push(Planned {
            name,
            offset: start,
            len,
            crc32: hasher.finalize(),
        });
        Ok(())
    }

    /// Compress the index, patch the header, sync, re-read every entry, then
    /// replace `target`. On any failure `target` is left as it was.
    pub fn finish(mut self) -> Result<(), RpaError> {
        let index_at = self.offset;
        let index = build_pickle(&self.planned, self.key);
        let mut enc = ZlibEncoder::new(Vec::new(), Compression::new(6));
        enc.write_all(&index)?;
        let compressed = enc.finish()?;
        {
            let file = self.file.as_mut().expect("archive file");
            file.write_all(&compressed)?;
            file.seek(SeekFrom::Start(0))?;
            file.write_all(&write_v3_header(index_at, self.key))?;
            file.sync_all()?;
        }
        self.file.take();

        if let Err(e) = verify(&self.temp, &self.planned) {
            let _ = fs::remove_file(&self.temp);
            return Err(e);
        }
        if let Err(e) = atomic_replace(&self.temp, &self.target) {
            let _ = fs::remove_file(&self.temp);
            return Err(e);
        }
        Ok(())
    }
}

impl Drop for ArchiveWriter {
    fn drop(&mut self) {
        // A writer that wasn't finished leaves a temp file. Remove it.
        // After finish(), the file was moved, so this remove is a no-op.
        let _ = fs::remove_file(&self.temp);
    }
}

fn build_pickle(planned: &[Planned], key: u32) -> Vec<u8> {
    let entries: Vec<(String, Vec<IndexPair>)> = planned
        .iter()
        .map(|p| {
            (
                p.name.clone(),
                vec![IndexPair {
                    offset: p.offset ^ key as u64,
                    len: p.len ^ key as u64,
                }],
            )
        })
        .collect();
    pickle::dump_index(&entries)
}

fn verify(path: &Path, planned: &[Planned]) -> Result<(), RpaError> {
    let archive = Archive::open(path)?;
    if archive.entries.len() != planned.len() {
        return Err(RpaError::new(format!(
            "wrote {} entries but read back {}",
            planned.len(),
            archive.entries.len()
        )));
    }
    for p in planned {
        let stats = archive.copy_entry(&p.name, &mut std::io::sink())?;
        if stats.len != p.len || stats.crc32 != p.crc32 {
            return Err(RpaError::new(format!(
                "`{}` did not match what was written (crc {:08x} vs {:08x})",
                p.name, stats.crc32, p.crc32
            )));
        }
    }
    Ok(())
}

fn atomic_replace(temp: &Path, target: &Path) -> Result<(), RpaError> {
    if !target.exists() {
        return fs::rename(temp, target)
            .map_err(|e| RpaError::new(format!("Could not move the new archive into place: {e}")));
    }
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "archive.rpa".into());
    let bak = target.with_file_name(format!(".{name}.vnide-bak"));
    let _ = fs::remove_file(&bak);
    fs::rename(target, &bak).map_err(|e| {
        RpaError::new(format!(
            "Could not move {} aside ({e}). If the game is running, close it and try again.",
            target.display()
        ))
    })?;
    if let Err(e) = fs::rename(temp, target) {
        let _ = fs::rename(&bak, target);
        return Err(RpaError::new(format!(
            "Could not move the new archive into place: {e}"
        )));
    }
    let _ = fs::remove_file(&bak);
    Ok(())
}

fn normalize_name(name: &str) -> Result<String, RpaError> {
    let name = name.replace('\\', "/");
    let _ = safe_relative(&name)?;
    Ok(name)
}

fn random_key() -> u32 {
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    std::process::id().hash(&mut hasher);
    SystemTime::now().hash(&mut hasher);
    let key = hasher.finish() as u32;
    if key == 0 {
        0x4242_4242
    } else {
        key
    }
}

/// Pack every file under `src` into `target`. Names are relative, with `/`.
pub fn build_from_dir(
    src: &Path,
    target: &Path,
    cancel: Option<&AtomicBool>,
) -> Result<BuildReport, RpaError> {
    if !src.is_dir() {
        return Err(RpaError::new(format!("{} is not a folder", src.display())));
    }
    let mut files = Vec::new();
    collect(src, src, &mut files)?;
    files.sort();
    let target_canon = fs::canonicalize(target).ok();
    let mut writer = ArchiveWriter::create(target)?;
    let mut bytes = 0u64;
    for path in &files {
        if cancel.map(|c| c.load(Ordering::Relaxed)).unwrap_or(false) {
            return Err(RpaError::new("cancelled"));
        }
        if target_canon
            .as_ref()
            .is_some_and(|t| fs::canonicalize(path).ok().as_ref() == Some(t))
        {
            continue;
        }
        let rel = path
            .strip_prefix(src)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        let len = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        writer.add_file(&rel, path)?;
        bytes += len;
    }
    let count = writer.planned.len() as u32;
    writer.finish()?;
    Ok(BuildReport {
        files: count,
        bytes,
    })
}

fn collect(dir: &Path, root: &Path, out: &mut Vec<PathBuf>) -> Result<(), RpaError> {
    let entries = fs::read_dir(dir)
        .map_err(|e| RpaError::new(format!("Could not read {}: {e}", dir.display())))?;
    for entry in entries {
        let entry = entry.map_err(|e| RpaError::new(e.to_string()))?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let ty = entry
            .file_type()
            .map_err(|e| RpaError::new(e.to_string()))?;
        if ty.is_dir() {
            collect(&path, root, out)?;
        } else if ty.is_file() {
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            // Skip names we would refuse to extract, so a built archive always extracts.
            let _ = safe_relative(&rel)?;
            out.push(path);
        }
    }
    Ok(())
}
