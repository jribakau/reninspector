//! Protocol-0 string decoders and `_codecs.encode` bytes.
//!
//! `STRING` is a Python `repr` of a byte string (latin-1, with escapes).
//! `UNICODE` is raw-unicode-escape: latin-1 bytes, with `\u`, `\U`, and the
//! same short escapes the archive reader already accepted.

use super::PickleError;

pub fn latin1_bytes(text: &str) -> Result<Vec<u8>, PickleError> {
    let mut out = Vec::with_capacity(text.len());
    for ch in text.chars() {
        let c = u32::from(ch);
        if c > 255 {
            return Err(PickleError::new("text is not latin-1"));
        }
        out.push(c as u8);
    }
    Ok(out)
}

pub(crate) fn decode_encoded(text: &str, encoding: &str) -> Result<Vec<u8>, PickleError> {
    match encoding.to_ascii_lowercase().as_str() {
        "latin1" | "latin-1" | "iso-8859-1" | "iso8859-1" => latin1_bytes(text),
        "utf-8" | "utf8" => Ok(text.as_bytes().to_vec()),
        "ascii" if text.is_ascii() => Ok(text.as_bytes().to_vec()),
        "ascii" => Err(PickleError::new("ascii encoding contains non-ascii text")),
        other => Err(PickleError::new(format!(
            "pickle encoding {other} is not allowed"
        ))),
    }
}

pub(crate) fn decode_repr(line: &str) -> Result<Vec<u8>, PickleError> {
    let mut chars = line.chars().peekable();
    let quote = chars
        .next()
        .ok_or_else(|| PickleError::new("empty pickle string"))?;
    if quote != '\'' && quote != '"' {
        return Err(PickleError::new("pickle string is not quoted"));
    }
    let mut out = Vec::new();
    while let Some(c) = chars.next() {
        if c == quote {
            if chars.peek().is_some() {
                return Err(PickleError::new("trailing data in pickle string"));
            }
            return Ok(out);
        }
        if c != '\\' {
            latin1_push(&mut out, c)?;
            continue;
        }
        let esc = chars
            .next()
            .ok_or_else(|| PickleError::new("truncated pickle escape"))?;
        match esc {
            '\\' => out.push(b'\\'),
            '\'' => out.push(b'\''),
            '"' => out.push(b'"'),
            'n' => out.push(b'\n'),
            'r' => out.push(b'\r'),
            't' => out.push(b'\t'),
            'x' => {
                let h1 = chars
                    .next()
                    .ok_or_else(|| PickleError::new("truncated \\x escape"))?;
                let h2 = chars
                    .next()
                    .ok_or_else(|| PickleError::new("truncated \\x escape"))?;
                let hex = format!("{h1}{h2}");
                let b =
                    u8::from_str_radix(&hex, 16).map_err(|_| PickleError::new("bad \\x escape"))?;
                out.push(b);
            }
            other => latin1_push(&mut out, other)?,
        }
    }
    Err(PickleError::new("unterminated pickle string"))
}

fn latin1_push(out: &mut Vec<u8>, c: char) -> Result<(), PickleError> {
    let u = u32::from(c);
    if u > 255 {
        return Err(PickleError::new("pickle string is not latin-1"));
    }
    out.push(u as u8);
    Ok(())
}

/// Raw-unicode-escape over latin-1 bytes. `\uXXXX` and `\UXXXXXXXX` are
/// unicode escapes. A high surrogate followed by a low one becomes one scalar.
pub(crate) fn decode_raw_unicode(bytes: &[u8]) -> Result<String, PickleError> {
    let mut out = String::new();
    let mut i = 0;
    let mut pending_high: Option<u32> = None;
    while i < bytes.len() {
        let b = bytes[i];
        i += 1;
        if b != b'\\' {
            push_code(&mut out, &mut pending_high, u32::from(b));
            continue;
        }
        if i >= bytes.len() {
            break;
        }
        let esc = bytes[i];
        i += 1;
        match esc {
            b'\\' => push_code(&mut out, &mut pending_high, u32::from(b'\\')),
            b'n' => push_code(&mut out, &mut pending_high, u32::from(b'\n')),
            b'r' => push_code(&mut out, &mut pending_high, u32::from(b'\r')),
            b't' => push_code(&mut out, &mut pending_high, u32::from(b'\t')),
            b'u' => {
                let cp = take_hex(bytes, &mut i, 4)?;
                push_code(&mut out, &mut pending_high, cp);
            }
            b'U' => {
                let cp = take_hex(bytes, &mut i, 8)?;
                push_code(&mut out, &mut pending_high, cp);
            }
            other => push_code(&mut out, &mut pending_high, u32::from(other)),
        }
    }
    if pending_high.take().is_some() {
        out.push('\u{FFFD}');
    }
    Ok(out)
}

fn take_hex(bytes: &[u8], i: &mut usize, n: usize) -> Result<u32, PickleError> {
    let end = i
        .checked_add(n)
        .filter(|end| *end <= bytes.len())
        .ok_or_else(|| {
            PickleError::new(if n == 8 {
                "truncated \\U escape"
            } else {
                "truncated \\u escape"
            })
        })?;
    let hex =
        std::str::from_utf8(&bytes[*i..end]).map_err(|_| PickleError::new("bad unicode escape"))?;
    *i = end;
    u32::from_str_radix(hex, 16).map_err(|_| PickleError::new("bad unicode escape"))
}

fn push_code(out: &mut String, pending_high: &mut Option<u32>, cp: u32) {
    if let Some(high) = pending_high.take() {
        if (0xDC00..=0xDFFF).contains(&cp) {
            let combined = 0x10000 + (((high - 0xD800) << 10) | (cp - 0xDC00));
            out.push(char::from_u32(combined).unwrap_or('\u{FFFD}'));
            return;
        }
        out.push('\u{FFFD}');
    }
    if (0xD800..=0xDBFF).contains(&cp) {
        *pending_high = Some(cp);
    } else {
        out.push(char::from_u32(cp).unwrap_or('\u{FFFD}'));
    }
}
