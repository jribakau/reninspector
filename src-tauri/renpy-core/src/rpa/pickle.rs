//! Protocol-2 writer for the archive indexes we emit.
//!
//! Reading an index goes through [`crate::pickle`] with
//! [`Policy::ARCHIVE_INDEX`](crate::pickle::Policy::ARCHIVE_INDEX). That reader
//! accepts protocols 0 through 5 and calls only `_codecs.encode`.

use crate::pickle::opcode::{
    APPENDS, BININT, BININT1, BININT2, BINUNICODE, EMPTY_DICT, EMPTY_LIST, LONG1, MARK, PROTO,
    SETITEMS, STOP, TUPLE2,
};

/// One `(offset, length)` pair stored under a name. Offsets are already the
/// on-disk values the caller wants pickled (XOR them first for RPA-3.0).
pub struct IndexPair {
    pub offset: u64,
    pub len: u64,
}

/// Protocol-2 pickle of `{name: [(offset, len), ...]}`.
pub fn dump_index<S: AsRef<str>>(entries: &[(S, Vec<IndexPair>)]) -> Vec<u8> {
    let mut out = vec![PROTO, 2, EMPTY_DICT, MARK];
    for (name, pairs) in entries {
        write_binunicode(&mut out, name.as_ref());
        out.push(EMPTY_LIST);
        if !pairs.is_empty() {
            out.push(MARK);
            for pair in pairs {
                write_int(&mut out, pair.offset);
                write_int(&mut out, pair.len);
                out.push(TUPLE2);
            }
            out.push(APPENDS);
        }
    }
    out.push(SETITEMS);
    out.push(STOP);
    out
}

fn write_binunicode(out: &mut Vec<u8>, s: &str) {
    let b = s.as_bytes();
    out.push(BINUNICODE);
    out.extend_from_slice(&(b.len() as u32).to_le_bytes());
    out.extend_from_slice(b);
}

fn write_int(out: &mut Vec<u8>, n: u64) {
    if n <= 0xff {
        out.push(BININT1);
        out.push(n as u8);
    } else if n <= 0xffff {
        out.push(BININT2);
        out.extend_from_slice(&(n as u16).to_le_bytes());
    } else if n <= i32::MAX as u64 {
        out.push(BININT);
        out.extend_from_slice(&(n as i32).to_le_bytes());
    } else {
        let bytes = long_bytes(n as i128);
        out.push(LONG1);
        out.push(bytes.len() as u8);
        out.extend_from_slice(&bytes);
    }
}

/// Minimal little-endian two's complement, with the sign bit in the last byte.
fn long_bytes(n: i128) -> Vec<u8> {
    if n == 0 {
        return vec![0];
    }
    let mut bytes = Vec::new();
    if n > 0 {
        let mut v = n;
        while v > 0 {
            bytes.push((v & 0xff) as u8);
            v >>= 8;
        }
        if bytes.last().copied().unwrap_or(0) & 0x80 != 0 {
            bytes.push(0);
        }
    } else {
        let mut v = n;
        loop {
            bytes.push((v & 0xff) as u8);
            v >>= 8;
            if v == -1 && bytes.last().copied().unwrap_or(0) & 0x80 != 0 {
                break;
            }
        }
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pickle::{load, Policy, Value};

    #[test]
    fn round_trip_protocol2_index() {
        let entries = vec![(
            "script.rpy",
            vec![
                IndexPair {
                    offset: 34,
                    len: 12,
                },
                IndexPair {
                    offset: 5_000_000_000,
                    len: 3,
                },
            ],
        )];
        let bytes = dump_index(&entries);
        let v = load(&bytes, &Policy::ARCHIVE_INDEX).unwrap();
        match v {
            Value::Dict(pairs) => {
                assert_eq!(pairs.len(), 1);
                assert_eq!(pairs[0].0, Value::Str("script.rpy".into()));
                match &pairs[0].1 {
                    Value::List(items) => {
                        assert_eq!(items.len(), 2);
                        assert_eq!(items[0], Value::Tuple(vec![Value::Int(34), Value::Int(12)]));
                        assert_eq!(
                            items[1],
                            Value::Tuple(vec![Value::Int(5_000_000_000), Value::Int(3)])
                        );
                    }
                    other => panic!("list expected, got {other:?}"),
                }
            }
            other => panic!("dict expected, got {other:?}"),
        }
    }
}
