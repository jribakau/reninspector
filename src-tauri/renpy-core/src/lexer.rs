//! Logical-line lexer for Ren'Py scripts.
//!
//! A logical line may span several physical lines (triple-quoted strings,
//! open brackets, trailing backslash). Comments are removed and the indent of
//! the first physical line is recorded. Line numbers are 1-based.

#[derive(Debug, Clone)]
pub struct LLine {
    pub line: u32,
    pub end_line: u32,
    pub indent: u32,
    /// Comment-free, trimmed text. Physical lines are joined with `\n`.
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct LexIssue {
    pub line: u32,
    pub message: String,
}

#[derive(Debug, Default)]
pub struct LexOut {
    pub lines: Vec<LLine>,
    pub issues: Vec<LexIssue>,
    pub total_lines: u32,
}

const MAX_STRING_LINES: usize = 40;
// Real games keep data tables of several thousand lines inside one bracket
// (Our Red String has a 4,660-line gallery list).
const MAX_BRACKET_LINES: usize = 30000;

pub fn lex(src: &str) -> LexOut {
    let src = src.strip_prefix('\u{feff}').unwrap_or(src);
    let phys: Vec<&str> = src
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect();
    let total = if src.ends_with('\n') {
        phys.len() - 1
    } else {
        phys.len()
    };
    let mut out = LexOut {
        lines: Vec::with_capacity(total / 2),
        issues: Vec::new(),
        total_lines: total as u32,
    };

    let mut i = 0usize;
    while i < total {
        let first = phys[i];
        let trimmed = first.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            i += 1;
            continue;
        }
        let indent = indent_width(first);
        match scan_logical(&phys[..total], i, false) {
            Scan::Ok { end, text } => {
                out.lines.push(LLine {
                    line: (i + 1) as u32,
                    end_line: (end + 1) as u32,
                    indent,
                    text,
                });
                i = end + 1;
            }
            Scan::Unterminated => {
                out.issues.push(LexIssue {
                    line: (i + 1) as u32,
                    message:
                        "Unterminated string or bracket; treating this physical line on its own."
                            .into(),
                });
                if let Scan::Ok { text, .. } = scan_logical(&phys[..total], i, true) {
                    out.lines.push(LLine {
                        line: (i + 1) as u32,
                        end_line: (i + 1) as u32,
                        indent,
                        text,
                    });
                }
                i += 1;
            }
        }
    }
    out
}

fn indent_width(line: &str) -> u32 {
    let mut width = 0u32;
    for b in line.bytes() {
        match b {
            b' ' => width += 1,
            b'\t' => width = (width / 8 + 1) * 8,
            _ => break,
        }
    }
    width
}

enum Scan {
    Ok { end: usize, text: String },
    Unterminated,
}

/// Scan one logical line starting at physical line `start`.
/// With `single` set, the scan never crosses a physical line.
fn scan_logical(phys: &[&str], start: usize, single: bool) -> Scan {
    let mut out: Vec<u8> = Vec::with_capacity(phys[start].len());
    let mut depth: i32 = 0;
    // (quote byte, triple)
    let mut in_str: Option<(u8, bool)> = None;
    let mut str_lines = 0usize;
    let mut j = start;
    loop {
        let bytes = phys[j].as_bytes();
        let mut k = 0usize;
        if j == start {
            while k < bytes.len() && (bytes[k] == b' ' || bytes[k] == b'\t') {
                k += 1;
            }
        }
        let mut continued = false;
        while k < bytes.len() {
            let c = bytes[k];
            if let Some((q, triple)) = in_str {
                if c == b'\\' {
                    out.push(c);
                    if k + 1 < bytes.len() {
                        out.push(bytes[k + 1]);
                        k += 2;
                    } else {
                        k += 1;
                    }
                    continue;
                }
                if c == q {
                    if triple {
                        if k + 2 < bytes.len() + 0 && bytes[k + 1] == q && bytes[k + 2] == q {
                            out.extend_from_slice(&[q, q, q]);
                            k += 3;
                            in_str = None;
                            continue;
                        }
                    } else {
                        out.push(c);
                        k += 1;
                        in_str = None;
                        continue;
                    }
                }
                out.push(c);
                k += 1;
                continue;
            }
            match c {
                b'#' => break,
                b'"' | b'\'' => {
                    if k + 2 < bytes.len() + 0 && bytes[k + 1] == c && bytes[k + 2] == c {
                        out.extend_from_slice(&[c, c, c]);
                        k += 3;
                        in_str = Some((c, true));
                        str_lines = 0;
                    } else {
                        out.push(c);
                        k += 1;
                        in_str = Some((c, false));
                        str_lines = 0;
                    }
                }
                b'(' | b'[' | b'{' => {
                    depth += 1;
                    out.push(c);
                    k += 1;
                }
                b')' | b']' | b'}' => {
                    if depth > 0 {
                        depth -= 1;
                    }
                    out.push(c);
                    k += 1;
                }
                b'\\' if k + 1 == bytes.len() => {
                    continued = true;
                    k += 1;
                }
                _ => {
                    out.push(c);
                    k += 1;
                }
            }
        }

        let more = if single {
            false
        } else if in_str.is_some() {
            str_lines += 1;
            true
        } else {
            depth > 0 || continued
        };
        if !more {
            // Trim trailing whitespace of the logical line.
            while matches!(out.last(), Some(b' ') | Some(b'\t')) {
                out.pop();
            }
            return Scan::Ok {
                end: j,
                text: String::from_utf8_lossy(&out).into_owned(),
            };
        }
        // Continue on the next physical line.
        if let Some((_, triple)) = in_str {
            if !triple && str_lines > MAX_STRING_LINES {
                return Scan::Unterminated;
            }
        } else if j - start > MAX_BRACKET_LINES {
            return Scan::Unterminated;
        }
        j += 1;
        if j >= phys.len() {
            return Scan::Unterminated;
        }
        out.push(if continued { b' ' } else { b'\n' });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_comments_but_not_hash_in_strings() {
        let out = lex("a \"{color=#fff}hi{/color}\" # note\n");
        assert_eq!(out.lines.len(), 1);
        assert_eq!(out.lines[0].text, "a \"{color=#fff}hi{/color}\"");
    }

    #[test]
    fn indent_and_blank_lines() {
        let out = lex("label x:\n\n    # c\n    jump y\n");
        assert_eq!(out.lines.len(), 2);
        assert_eq!(out.lines[1].indent, 4);
        assert_eq!(out.lines[1].line, 4);
        assert_eq!(out.total_lines, 4);
    }

    #[test]
    fn joins_triple_quotes_and_brackets() {
        let out = lex("e \"\"\"a\nb # not comment\nc\"\"\"\n$ x = [1,\n  2]\nnext\n");
        assert_eq!(out.lines.len(), 3);
        assert_eq!(out.lines[0].end_line, 3);
        assert_eq!(out.lines[1].line, 4);
        assert_eq!(out.lines[1].end_line, 5);
        assert_eq!(out.lines[2].text, "next");
    }

    #[test]
    fn unterminated_string_recovers() {
        let mut src = String::from("e \"oops\n");
        for _ in 0..60 {
            src.push_str("jump x\n");
        }
        let out = lex(&src);
        assert_eq!(out.issues.len(), 1);
        assert!(out.lines.len() > 50);
    }
}
