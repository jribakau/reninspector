//! Extract archive entries into a folder without letting paths escape it.

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use rayon::prelude::*;

use super::format::{safe_relative, RpaError};
use super::reader::Archive;

#[derive(Default)]
pub struct ExtractOptions<'a> {
    /// Entry names to extract. `None` extracts everything.
    pub names: Option<&'a HashSet<String>>,
    pub overwrite: bool,
    pub progress: Option<&'a (dyn Fn(u64, u64) + Sync)>,
    pub cancel: Option<&'a AtomicBool>,
}

#[derive(Debug)]
pub struct ExtractReport {
    pub files: u32,
    pub bytes: u64,
}

pub fn extract(
    archive: &Archive,
    dest: &Path,
    opts: &ExtractOptions<'_>,
) -> Result<ExtractReport, RpaError> {
    fs::create_dir_all(dest)
        .map_err(|e| RpaError::new(format!("Could not create {}: {e}", dest.display())))?;
    let mut names: Vec<&str> = match opts.names {
        Some(set) => {
            for name in set {
                if !archive.entries.contains_key(name) {
                    return Err(RpaError::new(format!("`{name}` is not in the archive")));
                }
            }
            set.iter().map(|s| s.as_str()).collect()
        }
        None => archive.entries.keys().map(|s| s.as_str()).collect(),
    };
    names.sort();

    let mut seen = HashSet::new();
    let mut jobs = Vec::with_capacity(names.len());
    for name in names {
        let rel = safe_relative(name)?;
        let key = rel.to_string_lossy().to_ascii_lowercase();
        if !seen.insert(key) {
            return Err(RpaError::new(format!(
                "`{name}` collides with another entry when names are compared case-insensitively"
            )));
        }
        jobs.push((name.to_string(), dest.join(rel)));
    }

    let total = jobs.len() as u64;
    let done = AtomicU64::new(0);
    let bytes = AtomicU64::new(0);
    let progress = opts.progress;
    let cancel = opts.cancel;
    let overwrite = opts.overwrite;

    jobs.par_iter()
        .try_for_each(|(name, path)| -> Result<(), RpaError> {
            if cancel.map(|c| c.load(Ordering::Relaxed)).unwrap_or(false) {
                return Err(RpaError::new("cancelled"));
            }
            if path.exists() && !overwrite {
                return Err(RpaError::new(format!("{} already exists", path.display())));
            }
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut file = std::io::BufWriter::new(File::create(path)?);
            let stats = archive.copy_entry(name, &mut file)?;
            file.flush()?;
            let file = file
                .into_inner()
                .map_err(|e| RpaError::new(e.to_string()))?;
            file.sync_all()?;
            bytes.fetch_add(stats.len, Ordering::Relaxed);
            let n = done.fetch_add(1, Ordering::Relaxed) + 1;
            if let Some(cb) = progress {
                if n == total || n % 32 == 0 {
                    cb(n, total);
                }
            }
            Ok(())
        })?;

    Ok(ExtractReport {
        files: done.load(Ordering::Relaxed) as u32,
        bytes: bytes.load(Ordering::Relaxed),
    })
}
