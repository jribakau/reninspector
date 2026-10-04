//! Words inside dialogue and menu choices, with positions in the original script.
//!
//! Tags, interpolations and format placeholders are not words. A string that
//! runs past the end of its line is followed until it closes.

use crate::ast::{Kind, Stmt};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProseWord {
    pub line: u32,
    /// UTF-16 column of the first character, matching a CodeMirror position in the line.
    pub from: u32,
    /// UTF-16 column just after the word.
    pub to: u32,
    pub word: String,
}

/// Words in say statements and menu choices. Other strings are left alone.
pub fn prose_words(src: &str) -> Vec<ProseWord> {
    let lexed = crate::lexer::lex(src);
    let parsed = crate::parser::parse(&lexed.lines);
    let mut sites = Vec::new();
    collect(&parsed.stmts, &mut sites);
    let mut words = Vec::new();
    for (line_no, menu) in sites {
        let Some((line, offset)) = line_at(src, line_no) else { continue };
        let Some(rel) = dialogue_quote(line, menu) else { continue };
        scan_string(src, offset + rel, &mut words);
    }
    words
}

fn collect(stmts: &[Stmt], out: &mut Vec<(u32, bool)>) {
    for stmt in stmts {
        match &stmt.kind {
            Kind::Say { .. } => out.push((stmt.line, false)),
            Kind::Menu { choices, .. } => {
                for choice in choices {
                    out.push((choice.line, true));
                    collect(&choice.body, out);
                }
            }
            Kind::Label { body, .. } | Kind::While { body, .. } => collect(body, out),
            Kind::If { branches } => {
                for branch in branches {
                    collect(&branch.body, out);
                }
            }
            _ => {}
        }
    }
}

fn line_at(src: &str, line_no: u32) -> Option<(&str, usize)> {
    let mut offset = 0usize;
    for (idx, raw) in src.split_inclusive('\n').enumerate() {
        if idx as u32 + 1 == line_no {
            let text = raw.trim_end_matches(['\n', '\r']);
            return Some((text, offset));
        }
        offset += raw.len();
    }
    None
}

fn is_ident(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c == b'.'
}

/// Byte index of the opening quote of the dialogue string on this physical line.
fn dialogue_quote(line: &str, menu: bool) -> Option<usize> {
    let bytes = line.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    if i >= bytes.len() {
        return None;
    }
    if menu {
        return (bytes[i] == b'"' || bytes[i] == b'\'').then_some(i);
    }
    if bytes[i] == b'"' || bytes[i] == b'\'' {
        return match close_on_line(line, i) {
            Some(end) => {
                let mut j = end;
                while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                    j += 1;
                }
                if j < bytes.len() && (bytes[j] == b'"' || bytes[j] == b'\'') {
                    Some(j)
                } else {
                    Some(i)
                }
            }
            None => Some(i),
        };
    }
    if !bytes[i].is_ascii_alphabetic() && bytes[i] != b'_' {
        return None;
    }
    while i < bytes.len() && is_ident(bytes[i]) {
        i += 1;
    }
    loop {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            return None;
        }
        if bytes[i] == b'"' || bytes[i] == b'\'' {
            return Some(i);
        }
        if bytes[i] == b'@' {
            i += 1;
            continue;
        }
        if bytes[i].is_ascii_alphabetic() || bytes[i] == b'_' || bytes[i] == b'-' {
            while i < bytes.len() && (is_ident(bytes[i]) || bytes[i] == b'-') {
                i += 1;
            }
            continue;
        }
        return None;
    }
}

/// Index just after a string that starts at `at` and closes on this same line.
fn close_on_line(line: &str, at: usize) -> Option<usize> {
    let bytes = line.as_bytes();
    let q = *bytes.get(at)?;
    let triple = at + 2 < bytes.len() && bytes[at + 1] == q && bytes[at + 2] == q;
    let mut i = at + if triple { 3 } else { 1 };
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            i += 2;
            continue;
        }
        if bytes[i] == q {
            if triple {
                if i + 2 < bytes.len() && bytes[i + 1] == q && bytes[i + 2] == q {
                    return Some(i + 3);
                }
            } else {
                return Some(i + 1);
            }
        }
        i += 1;
    }
    None
}

fn scan_string(src: &str, quote_at: usize, out: &mut Vec<ProseWord>) {
    let bytes = src.as_bytes();
    if quote_at >= bytes.len() {
        return;
    }
    let q = bytes[quote_at];
    if q != b'"' && q != b'\'' {
        return;
    }
    let triple = quote_at + 2 < bytes.len() && bytes[quote_at + 1] == q && bytes[quote_at + 2] == q;
    let mut i = quote_at + if triple { 3 } else { 1 };
    let (mut line, mut col) = line_col(src, i);
    let mut word = String::new();
    let mut word_line = line;
    let mut word_from = col;
    let start_line = line;

    let flush = |word: &mut String, out: &mut Vec<ProseWord>, line: u32, from: u32, to: u32| {
        if word.is_empty() {
            return;
        }
        out.push(ProseWord {
            line,
            from,
            to,
            word: std::mem::take(word),
        });
    };

    while i < bytes.len() && line.saturating_sub(start_line) < 40 {
        let c = bytes[i];
        if c == q && (!triple || (i + 2 < bytes.len() && bytes[i + 1] == q && bytes[i + 2] == q)) {
            if !triple || (i + 2 < bytes.len() && bytes[i + 1] == q && bytes[i + 2] == q) {
                flush(&mut word, out, word_line, word_from, col);
                return;
            }
        }
        if c == b'\\' && i + 1 < bytes.len() {
            flush(&mut word, out, word_line, word_from, col);
            step(src, &mut i, &mut line, &mut col);
            step(src, &mut i, &mut line, &mut col);
            continue;
        }
        if c == b'{' && i + 1 < bytes.len() && bytes[i + 1] == b'{' {
            flush(&mut word, out, word_line, word_from, col);
            step(src, &mut i, &mut line, &mut col);
            step(src, &mut i, &mut line, &mut col);
            continue;
        }
        if c == b'{' {
            flush(&mut word, out, word_line, word_from, col);
            step(src, &mut i, &mut line, &mut col);
            while i < bytes.len() && bytes[i] != b'}' && bytes[i] != b'\n' {
                step(src, &mut i, &mut line, &mut col);
            }
            if i < bytes.len() && bytes[i] == b'}' {
                step(src, &mut i, &mut line, &mut col);
            }
            continue;
        }
        if c == b'[' && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
            flush(&mut word, out, word_line, word_from, col);
            step(src, &mut i, &mut line, &mut col);
            step(src, &mut i, &mut line, &mut col);
            continue;
        }
        if c == b'[' {
            flush(&mut word, out, word_line, word_from, col);
            step(src, &mut i, &mut line, &mut col);
            while i < bytes.len() && bytes[i] != b']' && bytes[i] != b'\n' {
                step(src, &mut i, &mut line, &mut col);
            }
            if i < bytes.len() && bytes[i] == b']' {
                step(src, &mut i, &mut line, &mut col);
            }
            continue;
        }
        if c == b'%' {
            flush(&mut word, out, word_line, word_from, col);
            let end = skip_format(bytes, i);
            while i < end {
                step(src, &mut i, &mut line, &mut col);
            }
            continue;
        }
        let ch = src[i..].chars().next().unwrap_or('\u{fffd}');
        let next = src[i + ch.len_utf8()..].chars().next();
        if ch.is_alphabetic() || (ch == '\'' && q != b'\'' && !word.is_empty() && next.is_some_and(|n| n.is_alphabetic())) {
            if word.is_empty() {
                word_line = line;
                word_from = col;
            }
            word.push(ch);
            step(src, &mut i, &mut line, &mut col);
            continue;
        }
        flush(&mut word, out, word_line, word_from, col);
        step(src, &mut i, &mut line, &mut col);
    }
    flush(&mut word, out, word_line, word_from, col);
}

fn skip_format(bytes: &[u8], i: usize) -> usize {
    if i + 1 >= bytes.len() {
        return bytes.len();
    }
    if bytes[i + 1] == b'%' {
        return i + 2;
    }
    let mut j = i + 1;
    if bytes[j] == b'(' {
        while j < bytes.len() && bytes[j] != b')' && bytes[j] != b'\n' {
            j += 1;
        }
        if j < bytes.len() && bytes[j] == b')' {
            j += 1;
        }
    }
    while j < bytes.len() && matches!(bytes[j], b'0'..=b'9' | b'.' | b'-' | b'+' | b'#' ) {
        j += 1;
    }
    if j < bytes.len() && matches!(bytes[j], b's' | b'd' | b'r' | b'f' | b'i' | b'c' | b'%') {
        j += 1;
    }
    j
}

fn step(src: &str, i: &mut usize, line: &mut u32, col: &mut u32) {
    if *i >= src.len() {
        return;
    }
    let ch = src[*i..].chars().next().unwrap_or('\u{fffd}');
    *i += ch.len_utf8();
    if ch == '\n' {
        *line += 1;
        *col = 0;
    } else if ch != '\r' {
        *col += ch.len_utf16() as u32;
    }
}

fn line_col(src: &str, at: usize) -> (u32, u32) {
    let mut line = 1u32;
    let mut col = 0u32;
    let mut i = 0usize;
    while i < at && i < src.len() {
        step(src, &mut i, &mut line, &mut col);
    }
    (line, col)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(src: &str) -> Vec<String> {
        prose_words(src).into_iter().map(|w| w.word).collect()
    }

    #[test]
    fn prose_skips_tags_interpolation_and_escapes() {
        let src = "label start:\n    eileen \"Hello {b}there{/b}, [name].\"\n    \"She said \\\"hi\\\".\"\n";
        assert_eq!(words(src), vec!["Hello", "there", "She", "said", "hi"]);
    }

    #[test]
    fn prose_reads_menu_choices() {
        let src = "label start:\n    menu:\n        \"Go {i}home{/i}\":\n            jump x\n        \"Stay\" if False:\n            pass\n";
        assert_eq!(words(src), vec!["Go", "home", "Stay"]);
    }

    #[test]
    fn prose_follows_a_string_across_lines() {
        let src = "label start:\n    eileen \"Hello\nworld\"\n";
        let found = prose_words(src);
        assert_eq!(found.iter().map(|w| w.word.as_str()).collect::<Vec<_>>(), vec!["Hello", "world"]);
        assert_eq!(found[0].line, 2);
        assert_eq!(found[1].line, 3);
    }

    #[test]
    fn prose_ignores_format_placeholders() {
        let src = "label start:\n    \"Score: %s and %(name)s.\"\n";
        assert_eq!(words(src), vec!["Score", "and"]);
    }
}
