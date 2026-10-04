//! `.rpyc` container. Slot 1 is the statement tree before static transforms.

use flate2::read::ZlibDecoder;
use std::io::Read;

use super::unpickle::{self, Value};

const HEADER: &[u8] = b"RENPY RPC2";
const MAX_SLOT: usize = 64 * 1024 * 1024;

pub fn read_statements(bytes: &[u8]) -> Result<Vec<Value>, String> {
    let value = if bytes.starts_with(HEADER) {
        let slot = slot_bytes(bytes, 1).or_else(|_| slot_bytes(bytes, 2))?;
        unpickle::loads(&slot).map_err(|e| e.0)?
    } else {
        let plain = inflate(bytes)?;
        unpickle::loads(&plain).map_err(|e| e.0)?
    };
    match value {
        Value::Tuple(mut items) if items.len() >= 2 => match items.swap_remove(1) {
            Value::List(nodes) => Ok(nodes),
            _ => Err("rpyc statements are not a list".into()),
        },
        Value::List(nodes) => Ok(nodes),
        _ => Err("rpyc root is not a statement list".into()),
    }
}

fn slot_bytes(bytes: &[u8], want: u32) -> Result<Vec<u8>, String> {
    if bytes.len() < 10 + 12 {
        return Err("rpyc header is truncated".into());
    }
    let mut pos = 10usize;
    let mut found = None;
    while pos + 12 <= bytes.len() {
        let slot = u32::from_le_bytes(bytes[pos..pos + 4].try_into().unwrap());
        let start = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().unwrap()) as usize;
        let len = u32::from_le_bytes(bytes[pos + 8..pos + 12].try_into().unwrap()) as usize;
        if slot == 0 {
            break;
        }
        if start.saturating_add(len) > bytes.len() || len > MAX_SLOT {
            return Err("rpyc slot is outside the file".into());
        }
        if slot == want {
            found = Some(&bytes[start..start + len]);
            break;
        }
        pos += 12;
        if pos > bytes.len() {
            break;
        }
    }
    let raw = found.ok_or_else(|| format!("rpyc has no slot {want}"))?;
    inflate(raw)
}

fn inflate(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let mut dec = ZlibDecoder::new(bytes);
    let mut out = Vec::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = dec.read(&mut buf).map_err(|e| format!("rpyc zlib: {e}"))?;
        if n == 0 {
            break;
        }
        if out.len().saturating_add(n) > MAX_SLOT {
            return Err("rpyc decompressed slot is too large".into());
        }
        out.extend_from_slice(&buf[..n]);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::write::ZlibEncoder;
    use flate2::Compression;
    use std::io::Write;

    #[test]
    fn inflate_stops_before_a_decompression_bomb() {
        let mut enc = ZlibEncoder::new(Vec::new(), Compression::fast());
        let chunk = vec![0u8; 1024 * 1024];
        for _ in 0..=(MAX_SLOT / chunk.len()) {
            enc.write_all(&chunk).unwrap();
        }
        let compressed = enc.finish().unwrap();
        let err = inflate(&compressed).unwrap_err();
        assert!(err.contains("too large"), "{err}");
        assert!(
            compressed.len() < 1024 * 1024,
            "the bomb should stay small on disk"
        );
    }
}
