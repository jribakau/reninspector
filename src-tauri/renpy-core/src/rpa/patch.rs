//! The IDE's patch archive. It is a normal RPA-3.0 file whose name sorts
//! after every other archive, so Ren'Py searches it first (`arc_files.sort(reverse=True)`
//! in `renpy/loader.py`). A loose file in `game/` still wins over it.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::format::RpaError;
use super::reader::Archive;

pub const MANIFEST_NAME: &str = "vnide_patch.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PatchEntry {
    /// Archive the original bytes came from, relative to `game/`.
    pub source: Option<String>,
    /// CRC32 of that original entry at the time of the bake. `None` for a new file.
    pub base_crc32: Option<u32>,
    pub baked_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PatchManifest {
    pub version: u32,
    pub baked_at: String,
    pub entries: BTreeMap<String, PatchEntry>,
}

impl PatchManifest {
    pub fn to_bytes(&self) -> Result<Vec<u8>, RpaError> {
        serde_json::to_vec_pretty(self)
            .map_err(|e| RpaError::new(format!("Could not write the patch manifest: {e}")))
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, RpaError> {
        serde_json::from_slice(bytes)
            .map_err(|e| RpaError::new(format!("Patch manifest is not readable: {e}")))
    }
}

/// Stem (no extension) of the patch archive. It sorts after every stem in
/// `existing` except other `vnide_patch` names, which are the IDE's own.
pub fn patch_stem<I, S>(existing: I) -> String
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let stems: Vec<String> = existing
        .into_iter()
        .map(|s| s.as_ref().to_string())
        .filter(|s| !s.contains("vnide_patch"))
        .collect();
    let mut name = "zz_vnide_patch".to_string();
    while stems.iter().any(|s| s.as_str() >= name.as_str()) {
        name.insert(0, 'z');
        if name.len() > 240 {
            break;
        }
    }
    name
}

pub fn read_manifest(archive: &Archive) -> Result<Option<PatchManifest>, RpaError> {
    if !archive.entries.contains_key(MANIFEST_NAME) {
        return Ok(None);
    }
    let bytes = archive.read_entry(MANIFEST_NAME, 1_048_576)?;
    Ok(Some(PatchManifest::parse(&bytes)?))
}

/// Names whose original entry no longer has the CRC recorded at bake time.
/// `crc_of(archive, name)` reads that entry from the named source archive.
pub fn check_stale(
    manifest: &PatchManifest,
    crc_of: impl Fn(&str, &str) -> Option<u32>,
) -> Vec<String> {
    let mut stale = Vec::new();
    for (name, entry) in &manifest.entries {
        let Some(expected) = entry.base_crc32 else {
            continue;
        };
        let Some(archive) = entry.source.as_deref() else {
            continue;
        };
        match crc_of(archive, name) {
            Some(crc) if crc == expected => {}
            Some(_) => stale.push(format!("{name} in {archive} changed after it was patched")),
            None => stale.push(format!(
                "{name} was patched from {archive}, which no longer contains it"
            )),
        }
    }
    stale
}

/// UTC time as `YYYY-MM-DDTHH:MM:SSZ`. No extra crates.
pub fn utc_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format_unix(secs)
}

fn format_unix(secs: u64) -> String {
    let days = secs / 86_400;
    let tod = secs % 86_400;
    let (y, m, d) = civil_from_days(days as i64);
    let hh = tod / 3600;
    let mm = (tod % 3600) / 60;
    let ss = tod % 60;
    format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}Z")
}

/// Howard Hinnant's civil_from_days. `days` is days since 1970-01-01.
fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m as u32, d as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patch_name_sorts_last() {
        let stems = ["archive", "scripts", "zzz_sounds", "zz_vnide_patch"];
        let name = patch_stem(stems);
        for s in stems {
            if s.contains("vnide_patch") {
                continue;
            }
            assert!(name.as_str() > s, "{name} should sort after {s}");
        }
        assert!(name.starts_with('z'));
    }

    #[test]
    fn unix_epoch_formats() {
        assert_eq!(format_unix(0), "1970-01-01T00:00:00Z");
    }
}
