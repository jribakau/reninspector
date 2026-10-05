//! ZiX-12A / ZiX-12B, matching unrpa's `versions/zix.py`.
//!
//! The index key is not in the header. It is derived from a `verificationcode`
//! string in the game's `loader.py` / `loader.pyo`. Nothing is executed: the
//! file is scanned for that literal. ZiX-12B also obfuscates a prefix of each
//! extracted file.

use std::fs;
use std::path::{Path, PathBuf};

use super::format::RpaError;

const MAGIC_KEYS: [u64; 7] = [
    3621826839565189698,
    8167163782024462963,
    5643161164948769306,
    4940859562182903807,
    2672489546482320731,
    8917212212349173728,
    7093854916990953299,
];

const KEY_BIAS: u128 = 102464652121606009;

pub struct ZixParams {
    pub key: u64,
    /// Set for ZiX-12B: the first this many extracted bytes are deobfuscated.
    pub prefix: Option<u64>,
}

/// `obfuscation_sha1` from unrpa. Python 3 `round` (half to even) on the cube root.
pub fn key_from_code(code: &str) -> Result<u64, RpaError> {
    let digits: String = code.chars().filter(|c| c.is_ascii_digit()).collect();
    let n: u128 = if digits.is_empty() {
        0
    } else {
        digits
            .parse()
            .map_err(|_| RpaError::named("ZiX", "ZiX verification code has too many digits"))?
    };
    let a = n.saturating_add(KEY_BIAS) as f64;
    let rounded = round_half_even(a.cbrt());
    let b = rounded / 23.0 * 109.0;
    if !b.is_finite() || b < 0.0 || b >= (u64::MAX as f64) {
        return Err(RpaError::named("ZiX", "ZiX key does not fit in 64 bits"));
    }
    Ok(b as u64)
}

/// `obfuscation_offset`: rearranges the first eight characters of the header token.
pub fn offset_from_token(token: &[u8]) -> Result<u64, RpaError> {
    if token.len() < 8 {
        return Err(RpaError::named("ZiX", "ZiX offset field is too short"));
    }
    let hex = [
        token[7], token[6], token[0], token[1], token[2], token[5], token[4], token[3],
    ];
    let text = std::str::from_utf8(&hex)
        .map_err(|_| RpaError::named("ZiX", "ZiX offset field is not hex"))?;
    u64::from_str_radix(text, 16)
        .map_err(|_| RpaError::named("ZiX", format!("ZiX offset field {text:?} is not hex")))
}

/// Undo `obfuscation_run` on the first `amount` bytes. A trailing partial
/// 8-byte group inside that prefix is dropped, as unrpa does.
pub fn restore_prefix(data: &[u8], amount: u64, key: u64) -> Vec<u8> {
    let amount = (amount as usize).min(data.len());
    let whole = amount / 8 * 8;
    let mut out = Vec::with_capacity(data.len() - (amount - whole));
    for (i, chunk) in data[..whole].chunks_exact(8).enumerate() {
        let part = u64::from_le_bytes(chunk.try_into().unwrap());
        let decoded = MAGIC_KEYS[i % MAGIC_KEYS.len()] ^ key ^ part;
        out.extend_from_slice(&decoded.to_le_bytes());
    }
    out.extend_from_slice(&data[amount..]);
    out
}

pub fn load_params(archive: &Path, zix12b: bool) -> Result<ZixParams, RpaError> {
    let version = if zix12b { "ZiX-12B" } else { "ZiX-12A" };
    let candidates = loader_candidates(archive);
    let mut saw = None;
    for path in &candidates {
        let Ok(bytes) = fs::read(path) else {
            continue;
        };
        saw = Some(path.clone());
        let text = String::from_utf8_lossy(&bytes);
        let Some(code) = verification_code(&text) else {
            continue;
        };
        let key = key_from_code(&code).map_err(|e| RpaError::named(version, e.message))?;
        let prefix = if zix12b {
            Some(obfuscation_length(&text).ok_or_else(|| {
                RpaError::named(
                    version,
                    format!(
                        "{version} detected, loader `{}` has no obfuscation length",
                        file_label(path)
                    ),
                )
            })?)
        } else {
            None
        };
        return Ok(ZixParams { key, prefix });
    }
    let label = saw
        .as_ref()
        .map(|p| file_label(p))
        .unwrap_or_else(|| "loader.pyo".to_string());
    Err(RpaError::named(
        version,
        format!("{version} detected, loader `{label}` could not be read"),
    ))
}

fn file_label(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "loader.pyo".into())
}

fn loader_candidates(archive: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(dir) = archive.parent() {
        push_loaders(&mut out, dir);
        if let Some(parent) = dir.parent() {
            push_loaders(&mut out, &parent.join("renpy"));
        }
    }
    out
}

fn push_loaders(out: &mut Vec<PathBuf>, dir: &Path) {
    for name in ["loader.py", "loader.pyo", "loader.pyc"] {
        out.push(dir.join(name));
    }
}

fn verification_code(text: &str) -> Option<String> {
    let marker = "_string.sha1(";
    let rest = text.get(text.find(marker)? + marker.len()..)?;
    let rest = rest.trim_start();
    let quote = rest.chars().next()?;
    if quote != '\'' && quote != '"' {
        return None;
    }
    let body = rest.get(1..)?;
    let end = body.find(quote)?;
    Some(body[..end].to_string())
}

fn obfuscation_length(text: &str) -> Option<u64> {
    let marker = "_string.run(rv.read(";
    let rest = text.get(text.find(marker)? + marker.len()..)?;
    let end = rest.find(')')?;
    rest[..end].trim().parse().ok()
}

fn round_half_even(x: f64) -> f64 {
    let floor = x.floor();
    let frac = x - floor;
    if frac > 0.5 {
        floor + 1.0
    } else if frac < 0.5 || (floor as i64) % 2 == 0 {
        floor
    } else {
        floor + 1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_matches_unrpa_for_a_short_code() {
        assert_eq!(key_from_code("abc123").unwrap(), 2_217_633);
    }

    #[test]
    fn offset_rearranges_the_token() {
        assert_eq!(offset_from_token(b"01234567abcdef").unwrap(), 1_979_786_563);
    }

    #[test]
    fn restore_round_trips_a_qword_prefix() {
        let key = 2_217_633u64;
        let plain = b"HELLO!!!WORLD";
        let mut stored = plain.to_vec();
        let part = u64::from_le_bytes(plain[..8].try_into().unwrap());
        let encoded = MAGIC_KEYS[0] ^ key ^ part;
        stored[..8].copy_from_slice(&encoded.to_le_bytes());
        assert_eq!(restore_prefix(&stored, 8, key), plain);
    }
}
