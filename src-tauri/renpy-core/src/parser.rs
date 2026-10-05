//! Indentation-aware parser producing a statement tree with exact line spans.
//! Anything not understood becomes `Kind::Opaque`; the parser never fails.

use std::collections::HashMap;

use serde::Serialize;

use crate::ast::*;
use crate::lexer::LLine;

/// A syntax problem in an unsaved buffer. Line numbers are 1-based.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyntaxIssue {
    pub line: u32,
    pub message: String,
}

/// Lex and parse `src` without touching the project. Used while typing, so a
/// full reanalysis (reachability, the map) waits until the file is saved.
pub fn check_syntax(src: &str) -> Vec<SyntaxIssue> {
    let lexed = crate::lexer::lex(src);
    let parsed = parse(&lexed.lines);
    let mut out = Vec::with_capacity(lexed.issues.len() + parsed.issues.len());
    for i in lexed.issues {
        out.push(SyntaxIssue {
            line: i.line,
            message: i.message,
        });
    }
    for i in parsed.issues {
        out.push(SyntaxIssue {
            line: i.line,
            message: i.message,
        });
    }
    out
}

#[derive(Debug, Clone)]
pub struct Issue {
    pub line: u32,
    pub message: String,
}

#[derive(Debug, Default)]
pub struct ParseOut {
    pub stmts: Vec<Stmt>,
    pub issues: Vec<Issue>,
    pub opaque: u32,
    /// Identifier-like string literals found in code, defines and screens
    /// (label names stored as data, e.g. Event("party")).
    pub data_refs: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Meta {
    /// Names that are assigned (`define x =`, `default x =`, `$ x =`, `x =` in python).
    pub defs: Vec<String>,
    /// Explicit image names (`image a b = ...`, `layeredimage a:`).
    pub images: Vec<String>,
    /// `config.*` assignments: (key without prefix, raw value).
    pub config: Vec<(String, String)>,
    /// Label names referenced by screen actions such as `Jump("x")`.
    pub action_refs: Vec<String>,
    /// See ParseOut::data_refs.
    pub data_refs: Vec<String>,
    /// `translate` blocks and string tables.
    pub translates: Vec<TrNote>,
    /// Quoted file paths on `image`, `play`, `voice` and `Movie` lines.
    pub quoted_paths: Vec<QuotedPath>,
    /// Every bare identifier in code, with the lines it appears on. Skips
    /// attributes (`obj.greet`), `def`/`class` names and Ren'Py-only statements.
    pub idents: HashMap<String, Vec<u32>>,
    /// Exact style names used by `style ... is parent`, `style "name"` and
    /// `style.name`. `style_prefix` is left out because it derives names.
    pub style_uses: Vec<(String, u32)>,
}

/// One translated string or one line inside a `translate` block.
#[derive(Debug, Clone)]
pub struct TrNote {
    pub line: u32,
    pub lang: String,
    /// Block id, or the `old` text for a string translation.
    pub ident: String,
    /// English (or source) text. Empty for a dialogue block when it is not in the file.
    pub source: String,
    pub translated: String,
    /// `strings` or `block`.
    pub kind: &'static str,
}

/// A file name written in quotes (`play music "audio/x.ogg"`, `image a = "a.png"`).
#[derive(Debug, Clone)]
pub struct QuotedPath {
    pub line: u32,
    /// `image`, `audio` or `movie`.
    pub kind: &'static str,
    pub path: String,
}

pub fn parse(lines: &[LLine]) -> ParseOut {
    let mut p = Parser {
        lines,
        pos: 0,
        issues: Vec::new(),
        opaque: 0,
        cur_global: String::new(),
        data: Vec::new(),
    };
    let base = lines.first().map(|l| l.indent).unwrap_or(0);
    let mut stmts = Vec::new();
    while p.pos < lines.len() {
        let mut chunk = p.parse_block(base.min(lines[p.pos].indent));
        stmts.append(&mut chunk);
    }
    let mut data = p.data;
    data.sort();
    data.dedup();
    ParseOut {
        stmts,
        issues: p.issues,
        opaque: p.opaque,
        data_refs: data,
    }
}

struct Parser<'a> {
    lines: &'a [LLine],
    pos: usize,
    issues: Vec<Issue>,
    opaque: u32,
    cur_global: String,
    data: Vec<String>,
}

const PRESENT_WORDS: &[&str] = &[
    "show", "scene", "hide", "with", "pause", "play", "stop", "queue", "voice", "window", "nvl",
    "camera",
];

const OPAQUE_WORDS: &[&str] = &[
    "screen",
    "translate",
    "testcase",
    "layeredimage",
    "rpy",
    "init",
];

fn present_cmd(word: &str) -> Option<&'static str> {
    PRESENT_WORDS.iter().copied().find(|w| *w == word)
}

impl<'a> Parser<'a> {
    fn issue(&mut self, line: u32, message: &str) {
        if self.issues.len() < 500 {
            self.issues.push(Issue {
                line,
                message: message.to_string(),
            });
        }
    }

    fn parse_block(&mut self, indent: u32) -> Vec<Stmt> {
        let mut out = Vec::new();
        while let Some(l) = self.lines.get(self.pos) {
            if l.indent < indent {
                break;
            }
            if l.indent > indent {
                self.issue(l.line, "Unexpected indentation.");
            }
            out.push(self.parse_stmt());
        }
        out
    }

    fn child_block(&mut self, header_indent: u32) -> Vec<Stmt> {
        match self.lines.get(self.pos) {
            Some(n) if n.indent > header_indent => self.parse_block(n.indent),
            _ => Vec::new(),
        }
    }

    /// Swallow every following line deeper than `header_indent`.
    /// Returns the last physical line covered and any label references found.
    fn consume_raw(&mut self, header_indent: u32, base_end: u32) -> (u32, Vec<PyRef>, Vec<PyName>) {
        let mut end = base_end;
        let mut refs = Vec::new();
        let mut names = Vec::new();
        // Only the block's own `def` / `class`; methods and nested helpers are not store names.
        let mut base: Option<u32> = None;
        while let Some(n) = self.lines.get(self.pos) {
            if n.indent <= header_indent {
                break;
            }
            end = end.max(n.end_line);
            let top = *base.get_or_insert(n.indent);
            if n.text.contains('(') {
                refs.extend(scan_refs(&n.text).into_iter().map(|mut r| {
                    r.line = n.line;
                    r
                }));
            }
            if n.indent <= top {
                if let Some((kind, name)) = py_name(&n.text) {
                    names.push(PyName {
                        kind,
                        name,
                        line: n.line,
                    });
                }
            }
            harvest_strings(&n.text, &mut self.data);
            self.pos += 1;
        }
        (end, refs, names)
    }

    /// Swallow a screen body. Returns the last physical line plus the labels
    /// (`Jump("x")`, `renpy.call("x")`, ...) and other screens (`use x`,
    /// `Show("x")`, ...) it can lead to.
    fn consume_screen(
        &mut self,
        header_indent: u32,
        base_end: u32,
    ) -> (u32, Vec<NameAt>, Vec<NameAt>) {
        let mut end = base_end;
        let mut labels: Vec<NameAt> = Vec::new();
        let mut uses: Vec<NameAt> = Vec::new();
        while let Some(n) = self.lines.get(self.pos) {
            if n.indent <= header_indent {
                break;
            }
            end = end.max(n.end_line);
            if let Some(rest) = n.text.strip_prefix("use ") {
                let name = ident_prefix(rest.trim_start());
                if !name.is_empty() {
                    uses.push(NameAt { name, line: n.line });
                }
            }
            if n.text.contains('(') {
                for r in scan_refs(&n.text) {
                    let Some(name) = r.name else { continue };
                    let list = match r.kind {
                        RefKind::Action | RefKind::RenpyJump | RefKind::RenpyCall => &mut labels,
                        RefKind::Screen | RefKind::ScreenAction => &mut uses,
                    };
                    list.push(NameAt { name, line: n.line });
                }
            }
            harvest_strings(&n.text, &mut self.data);
            self.pos += 1;
        }
        (end, labels, uses)
    }

    fn parse_stmt(&mut self) -> Stmt {
        let lines = self.lines;
        let l = &lines[self.pos];
        self.pos += 1;
        let text = l.text.as_str();
        let (word, rest) = first_word(text);
        let mut end = l.end_line;

        let kind = match word {
            "label" => {
                let header = rest.trim_end_matches(':').trim();
                let name_end = header
                    .find(|c: char| c == '(' || c.is_whitespace())
                    .unwrap_or(header.len());
                let raw = &header[..name_end];
                if raw.is_empty() {
                    self.issue(l.line, "label without a name");
                    self.opaque += 1;
                    let (e, _, _) = self.consume_raw(l.indent, end);
                    end = e;
                    Kind::Opaque {
                        kind: "label".into(),
                    }
                } else {
                    let name = if let Some(local) = raw.strip_prefix('.') {
                        format!("{}.{}", self.cur_global, local)
                    } else {
                        self.cur_global = raw.to_string();
                        raw.to_string()
                    };
                    let body = self.child_block(l.indent);
                    end = end_of(&body, end);
                    Kind::Label { name, body }
                }
            }
            "menu" => {
                let header = rest.trim_end_matches(':').trim();
                let name_end = header
                    .find(|c: char| c == '(' || c.is_whitespace())
                    .unwrap_or(header.len());
                let raw = &header[..name_end];
                let name = if raw.is_empty() {
                    None
                } else if let Some(local) = raw.strip_prefix('.') {
                    Some(format!("{}.{}", self.cur_global, local))
                } else {
                    Some(raw.to_string())
                };
                let (caption, choices) = self.parse_menu_body(l.indent);
                if let Some(c) = choices.last() {
                    end = end.max(c.end_line);
                }
                Kind::Menu {
                    name,
                    caption,
                    choices,
                }
            }
            "jump" => {
                let (target, dynamic) = self.target_of(rest);
                Kind::Jump { target, dynamic }
            }
            "call" => {
                let r = rest.trim();
                if let Some(after) = r
                    .strip_prefix("screen")
                    .filter(|a| a.starts_with(char::is_whitespace))
                {
                    let name = ident_prefix(after.trim_start());
                    let (e, _, _) = self.consume_raw(l.indent, end);
                    end = e;
                    if name.is_empty() {
                        Kind::Present {
                            cmd: "call screen",
                            name: None,
                            text: short(r),
                        }
                    } else {
                        Kind::ScreenRef { how: "call", name }
                    }
                } else {
                    let from = find_from(r);
                    let head = match find_word_pos(r, "from") {
                        Some(p) => r[..p].trim(),
                        None => r,
                    };
                    let (target, dynamic) = self.target_of(head);
                    Kind::Call {
                        target,
                        dynamic,
                        from,
                    }
                }
            }
            "return" => Kind::Return,
            "pass" => Kind::Pass,
            "if" => {
                let cond = rest.trim_end_matches(':').trim().to_string();
                let body = self.child_block(l.indent);
                end = end_of(&body, end);
                let mut branches = vec![Branch {
                    line: l.line,
                    end_line: end,
                    kind: BranchKind::If,
                    cond,
                    body,
                }];
                while let Some(n) = lines.get(self.pos) {
                    if n.indent != l.indent {
                        break;
                    }
                    let (w, r) = first_word(&n.text);
                    let bk = match w {
                        "elif" => BranchKind::Elif,
                        "else" if r.trim() == ":" => BranchKind::Else,
                        _ => break,
                    };
                    self.pos += 1;
                    let bcond = if bk == BranchKind::Else {
                        String::new()
                    } else {
                        r.trim_end_matches(':').trim().to_string()
                    };
                    let b = self.child_block(n.indent);
                    let bend = end_of(&b, n.end_line);
                    end = end.max(bend);
                    branches.push(Branch {
                        line: n.line,
                        end_line: bend,
                        kind: bk,
                        cond: bcond,
                        body: b,
                    });
                }
                Kind::If { branches }
            }
            "elif" | "else" => {
                self.issue(l.line, "`elif`/`else` without a matching `if`.");
                self.opaque += 1;
                let (e, _, _) = self.consume_raw(l.indent, end);
                end = e;
                Kind::Opaque { kind: word.into() }
            }
            "while" => {
                let cond = rest.trim_end_matches(':').trim().to_string();
                let body = self.child_block(l.indent);
                end = end_of(&body, end);
                Kind::While { cond, body }
            }
            "$" => {
                let t = rest.trim();
                harvest_strings(t, &mut self.data);
                Kind::Python {
                    block: false,
                    text: short(t),
                    refs: if t.contains('(') {
                        scan_refs(t)
                            .into_iter()
                            .map(|mut r| {
                                r.line = l.line;
                                r
                            })
                            .collect()
                    } else {
                        Vec::new()
                    },
                    names: py_name(t)
                        .map(|(kind, name)| PyName {
                            kind,
                            name,
                            line: l.line,
                        })
                        .into_iter()
                        .collect(),
                }
            }
            "python" => {
                let (e, refs, names) = self.consume_raw(l.indent, end);
                end = e;
                Kind::Python {
                    block: true,
                    text: short(text),
                    refs,
                    names,
                }
            }
            "init" if find_word_pos(rest, "python").is_some() => {
                let (e, refs, names) = self.consume_raw(l.indent, end);
                end = e;
                Kind::Python {
                    block: true,
                    text: short(text),
                    refs,
                    names,
                }
            }
            "define" | "default" => {
                let keyword = if word == "define" {
                    "define"
                } else {
                    "default"
                };
                let (name, value) = parse_define(rest);
                harvest_strings(rest, &mut self.data);
                let (e, _, _) = self.consume_raw(l.indent, end);
                end = e;
                Kind::Define {
                    keyword,
                    name,
                    value,
                }
            }
            "transform" => {
                let name = ident_prefix(rest.trim_start());
                let (e, _, _) = self.consume_raw(l.indent, end);
                end = e;
                if name.is_empty() {
                    self.opaque += 1;
                    Kind::Opaque {
                        kind: "transform".into(),
                    }
                } else {
                    Kind::Transform { name }
                }
            }
            "style" => {
                let name = ident_prefix(rest.trim_start());
                let (e, _, _) = self.consume_raw(l.indent, end);
                end = e;
                if name.is_empty() {
                    self.opaque += 1;
                    Kind::Opaque { kind: "style".into() }
                } else {
                    Kind::Style { name }
                }
            }
            "image" => {
                let head = rest.split('=').next().unwrap_or(rest).trim_end_matches(':');
                let name = collapse_ws(head);
                let (e, _, _) = self.consume_raw(l.indent, end);
                end = e;
                Kind::Image { name }
            }
            "show"
                if rest
                    .strip_prefix("screen")
                    .map(|a| {
                        a.starts_with(char::is_whitespace)
                            && !ident_prefix(a.trim_start()).is_empty()
                    })
                    .unwrap_or(false) =>
            {
                let name = ident_prefix(rest["screen".len()..].trim_start());
                let (e, _, _) = self.consume_raw(l.indent, end);
                end = e;
                Kind::ScreenRef { how: "show", name }
            }
            "screen" if !ident_prefix(rest).is_empty() => {
                let name = ident_prefix(rest);
                let (e, labels, uses) = self.consume_screen(l.indent, end);
                end = e;
                Kind::Screen { name, labels, uses }
            }
            w if present_cmd(w).is_some() && !looks_like_assignment(rest) => {
                let cmd = present_cmd(w).unwrap();
                let name = if cmd == "show" || cmd == "scene" {
                    image_name_of(rest)
                } else {
                    None
                };
                let (e, _, _) = self.consume_raw(l.indent, end);
                end = e;
                Kind::Present {
                    cmd,
                    name,
                    text: short(rest),
                }
            }
            w if OPAQUE_WORDS.contains(&w) => {
                self.opaque += 1;
                let (e, _, _) = self.consume_raw(l.indent, end);
                end = e;
                Kind::Opaque {
                    kind: w.to_string(),
                }
            }
            _ => {
                if let Some((who, text)) = parse_say(text) {
                    Kind::Say { who, text }
                } else {
                    self.opaque += 1;
                    let (e, _, _) = self.consume_raw(l.indent, end);
                    end = e;
                    let kind = if word.is_empty() { "?" } else { word };
                    Kind::Opaque {
                        kind: kind.to_string(),
                    }
                }
            }
        };

        Stmt {
            line: l.line,
            end_line: end,
            kind,
        }
    }

    /// Resolve `name`, `.local` or `expression ...` after `jump`/`call`.
    fn target_of(&self, rest: &str) -> (String, bool) {
        let r = rest.trim();
        if let Some(expr) = r
            .strip_prefix("expression")
            .filter(|a| a.is_empty() || a.starts_with(char::is_whitespace))
        {
            return (short(expr.trim()), true);
        }
        let end = r
            .find(|c: char| c == '(' || c.is_whitespace())
            .unwrap_or(r.len());
        let raw = &r[..end];
        if let Some(local) = raw.strip_prefix('.') {
            (format!("{}.{}", self.cur_global, local), false)
        } else {
            (raw.to_string(), false)
        }
    }

    fn parse_menu_body(&mut self, header_indent: u32) -> (Option<String>, Vec<Choice>) {
        let lines = self.lines;
        let mut caption = None;
        let mut choices = Vec::new();
        let ci = match lines.get(self.pos) {
            Some(first) if first.indent > header_indent => first.indent,
            _ => return (caption, choices),
        };
        while let Some(m) = lines.get(self.pos) {
            if m.indent < ci {
                break;
            }
            self.pos += 1;
            if m.indent > ci {
                self.issue(m.line, "Unexpected indentation.");
                continue;
            }
            let t = m.text.as_str();
            let is_quote = t.starts_with('"') || t.starts_with('\'');
            if is_quote && t.ends_with(':') {
                if let Some((text, cond)) = parse_choice_header(t) {
                    let body = self.child_block(m.indent);
                    let end = end_of(&body, m.end_line);
                    choices.push(Choice {
                        line: m.line,
                        end_line: end,
                        text,
                        cond,
                        body,
                    });
                    continue;
                }
            }
            if caption.is_none() {
                if let Some((_, txt)) = parse_say(t) {
                    caption = Some(txt);
                }
            }
            if t.ends_with(':') {
                let _ = self.consume_raw(m.indent, m.end_line);
            }
        }
        (caption, choices)
    }
}

fn end_of(body: &[Stmt], base: u32) -> u32 {
    body.last().map(|s| s.end_line.max(base)).unwrap_or(base)
}

fn short(s: &str) -> String {
    let flat = collapse_ws(s);
    if flat.chars().count() > 160 {
        let cut: String = flat.chars().take(160).collect();
        format!("{cut}…")
    } else {
        flat
    }
}

pub fn collapse_ws(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut space = false;
    for c in s.trim().chars() {
        if c.is_whitespace() {
            if !space {
                out.push(' ');
            }
            space = true;
        } else {
            out.push(c);
            space = false;
        }
    }
    out
}

/// Split `text` into its leading keyword and the remainder.
/// `$` is its own keyword.
pub fn first_word(text: &str) -> (&str, &str) {
    if let Some(rest) = text.strip_prefix('$') {
        return ("$", rest);
    }
    let end = text
        .find(|c: char| !(c.is_alphanumeric() || c == '_'))
        .unwrap_or(text.len());
    (&text[..end], text[end..].trim_start())
}

fn looks_like_assignment(rest: &str) -> bool {
    let r = rest.trim_start();
    r.starts_with('=') && !r.starts_with("==")
}

/// Parse a Ren'Py string literal at the start of `s`.
/// Returns (unescaped content, byte index after the closing quote).
pub fn parse_string_at(s: &str) -> Option<(String, usize)> {
    let bytes = s.as_bytes();
    let q = *bytes.first()?;
    if q != b'"' && q != b'\'' {
        return None;
    }
    let triple = bytes.len() >= 3 && bytes[1] == q && bytes[2] == q;
    let mut i = if triple { 3 } else { 1 };
    let mut out: Vec<u8> = Vec::new();
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'\\' && i + 1 < bytes.len() {
            let n = bytes[i + 1];
            match n {
                b'n' => out.push(b' '),
                b'"' | b'\'' | b'\\' => out.push(n),
                _ => {
                    out.push(b'\\');
                    out.push(n);
                }
            }
            i += 2;
            continue;
        }
        if c == q {
            if triple {
                if i + 2 < bytes.len() + 0 && bytes[i + 1] == q && bytes[i + 2] == q {
                    return Some((String::from_utf8_lossy(&out).into_owned(), i + 3));
                }
            } else {
                return Some((String::from_utf8_lossy(&out).into_owned(), i + 1));
            }
        }
        out.push(c);
        i += 1;
    }
    None
}

/// Recognise `who "text"`, `who attrs "text"` and `"text"`.
pub fn parse_say(text: &str) -> Option<(Option<String>, String)> {
    let (who, what) = parse_say_full(text)?;
    Some((who.map(|w| short(&w)), short(&what)))
}

/// `parse_say` without shortening the text.
pub fn parse_say_full(text: &str) -> Option<(Option<String>, String)> {
    let t = text.trim();
    if t.starts_with('"') || t.starts_with('\'') {
        let (content, end) = parse_string_at(t)?;
        let after = t[end..].trim_start();
        if after.starts_with('"') || after.starts_with('\'') {
            let (second, _) = parse_string_at(after)?;
            // String-literal speaker: keep the quotes so it is never mistaken for a variable.
            return Some((Some(format!("\"{content}\"")), second));
        }
        return Some((None, content));
    }
    let who_end = t
        .find(|c: char| !(c.is_alphanumeric() || c == '_' || c == '.'))
        .unwrap_or(t.len());
    if who_end == 0 {
        return None;
    }
    let who = &t[..who_end];
    let mut rest = t[who_end..].trim_start();
    // Optional attributes (`happy`, `@ happy`, `-hide`) before the string.
    loop {
        if rest.starts_with('"') || rest.starts_with('\'') {
            let (content, _) = parse_string_at(rest)?;
            return Some((Some(who.to_string()), content));
        }
        if rest.starts_with('@') {
            rest = rest[1..].trim_start();
            continue;
        }
        let end = rest
            .find(|c: char| !(c.is_alphanumeric() || c == '_' || c == '-'))
            .unwrap_or(rest.len());
        if end == 0 {
            return None;
        }
        rest = rest[end..].trim_start();
    }
}

fn parse_choice_header(t: &str) -> Option<(String, Option<String>)> {
    let (content, end) = parse_string_at(t)?;
    let rest = t[end..].trim();
    let rest = rest.strip_suffix(':').unwrap_or(rest).trim();
    let cond = find_if_keyword(rest).map(|p| rest[p + 2..].trim().to_string());
    Some((short(&content), cond))
}

/// Byte index of a top-level `if` keyword in `s`.
fn find_if_keyword(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    let mut depth = 0i32;
    let mut quote: Option<u8> = None;
    let mut i = 0usize;
    while i < b.len() {
        let c = b[i];
        if let Some(q) = quote {
            if c == b'\\' {
                i += 2;
                continue;
            }
            if c == q {
                quote = None;
            }
        } else {
            match c {
                b'"' | b'\'' => quote = Some(c),
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' => depth -= 1,
                b'i' if depth == 0
                    && i + 1 < b.len()
                    && b[i + 1] == b'f'
                    && (i == 0 || b[i - 1].is_ascii_whitespace())
                    && (i + 2 >= b.len() || b[i + 2].is_ascii_whitespace() || b[i + 2] == b'(') =>
                {
                    return Some(i);
                }
                _ => {}
            }
        }
        i += 1;
    }
    None
}

fn find_word_pos(s: &str, word: &str) -> Option<usize> {
    let mut from = 0usize;
    while let Some(p) = s[from..].find(word) {
        let at = from + p;
        let before_ok = at == 0 || !is_ident_byte(s.as_bytes()[at - 1]);
        let after = at + word.len();
        let after_ok = after >= s.len() || !is_ident_byte(s.as_bytes()[after]);
        if before_ok && after_ok {
            return Some(at);
        }
        from = at + word.len();
    }
    None
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// `call foo from bar` -> `Some("bar")`.
fn find_from(r: &str) -> Option<String> {
    let p = find_word_pos(r, "from")?;
    let name = r[p + 4..].trim();
    let end = name
        .find(|c: char| !(c.is_alphanumeric() || c == '_'))
        .unwrap_or(name.len());
    if end == 0 {
        None
    } else {
        Some(name[..end].to_string())
    }
}

fn parse_define(rest: &str) -> (String, String) {
    let mut r = rest.trim_start();
    // `define -1 name = ...` / `define offset`
    let first_end = r.find(char::is_whitespace).unwrap_or(r.len());
    let first = &r[..first_end];
    if first
        .chars()
        .next()
        .map(|c| c == '-' || c.is_ascii_digit())
        .unwrap_or(false)
    {
        r = r[first_end..].trim_start();
    }
    let (head, value) = match r.find('=') {
        Some(p) => (&r[..p], r[p + 1..].trim()),
        None => (r, ""),
    };
    (head.trim().to_string(), short(value))
}

const IMAGE_STOP_WORDS: &[&str] = &[
    "at",
    "as",
    "behind",
    "with",
    "zorder",
    "onlayer",
    "expression",
    "tag",
];

/// Image name of `show`/`scene` (the words before `at`, `with`, ...).
fn image_name_of(rest: &str) -> Option<String> {
    let mut words: Vec<&str> = Vec::new();
    for w in rest.split_whitespace() {
        let w = w.trim_end_matches(':');
        if w.is_empty() || IMAGE_STOP_WORDS.contains(&w) {
            break;
        }
        if w.contains(['"', '\'', '(', '[', '=']) {
            break;
        }
        words.push(w);
    }
    // `show screen foo` / `hide screen foo` are not images.
    if words.is_empty() || words[0] == "expression" || words[0] == "screen" {
        None
    } else {
        Some(words.join(" "))
    }
}

/// Leading identifier of `s` (letters, digits, `_`).
/// `def name` / `class name` at the start of a Python line.
fn py_name(line: &str) -> Option<(&'static str, String)> {
    let t = line.trim();
    let (kind, rest) = if let Some(rest) = t.strip_prefix("def ") {
        ("function", rest)
    } else if let Some(rest) = t.strip_prefix("async def ") {
        ("function", rest)
    } else if let Some(rest) = t.strip_prefix("class ") {
        ("class", rest)
    } else {
        return None;
    };
    let name = ident_prefix(rest.trim_start());
    if name.is_empty() {
        None
    } else {
        Some((kind, name))
    }
}

fn ident_prefix(s: &str) -> String {
    s.chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect()
}

/// Collect identifier-like string literals (candidate label names kept as data).
fn harvest_strings(text: &str, out: &mut Vec<String>) {
    let mut i = 0;
    while let Some(p) = text[i..].find(['"', '\'']) {
        let at = i + p;
        match parse_string_at(&text[at..]) {
            Some((s, used)) => {
                if is_label_like(&s) {
                    out.push(s);
                }
                i = at + used.max(1);
            }
            None => i = at + 1,
        }
    }
}

fn is_label_like(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if c.is_alphabetic() || c == '_')
        && s.len() <= 100
        && s.chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '.')
}

/// Find string-literal label references in a code line.
pub fn scan_refs(text: &str) -> Vec<PyRef> {
    const PATTERNS: &[(&str, RefKind)] = &[
        ("renpy.jump(", RefKind::RenpyJump),
        ("renpy.call(", RefKind::RenpyCall),
        ("renpy.call_in_new_context(", RefKind::RenpyCall),
        ("Jump(", RefKind::Action),
        ("Call(", RefKind::Action),
        ("Start(", RefKind::Action),
        ("renpy.call_screen(", RefKind::Screen),
        ("renpy.show_screen(", RefKind::Screen),
        ("Show(", RefKind::ScreenAction),
        ("ShowMenu(", RefKind::ScreenAction),
        ("ToggleScreen(", RefKind::ScreenAction),
    ];
    let b = text.as_bytes();
    let mut out = Vec::new();
    for (pat, kind) in PATTERNS {
        let mut from = 0usize;
        while let Some(p) = text[from..].find(pat) {
            let at = from + p;
            from = at + pat.len();
            if at > 0
                && (is_ident_byte(b[at - 1]) || b[at - 1] == b'.')
                && matches!(kind, RefKind::Action | RefKind::ScreenAction)
            {
                continue;
            }
            if at > 0 && is_ident_byte(b[at - 1]) {
                continue;
            }
            let arg = text[from..].trim_start();
            if arg.starts_with('"') || arg.starts_with('\'') {
                if let Some((name, used)) = parse_string_at(arg) {
                    // `"v%s" % x`, `"a" + b`, `"a".format(x)`: the target is computed.
                    let next = arg[used..].trim_start().chars().next();
                    let computed = matches!(next, Some('%') | Some('+') | Some('.'));
                    if !computed {
                        out.push(PyRef {
                            kind: *kind,
                            name: Some(name),
                            line: 0,
                        });
                    } else if !matches!(kind, RefKind::Action | RefKind::ScreenAction) {
                        out.push(PyRef {
                            kind: *kind,
                            name: None,
                            line: 0,
                        });
                    }
                }
            } else if *pat == "Start(" && arg.starts_with(')') {
                out.push(PyRef {
                    kind: *kind,
                    name: Some("start".into()),
                    line: 0,
                });
            } else if !matches!(kind, RefKind::Action | RefKind::ScreenAction) {
                out.push(PyRef {
                    kind: *kind,
                    name: None,
                    line: 0,
                });
            }
        }
    }
    out
}

/// Collect definitions / images / config from every logical line.
pub fn scan_meta(lines: &[LLine]) -> Meta {
    let mut meta = Meta::default();
    for l in lines {
        let t = l.text.as_str();
        if let Some(rest) = t
            .strip_prefix("image ")
            .or_else(|| t.strip_prefix("layeredimage "))
        {
            let head = rest.split('=').next().unwrap_or(rest).trim_end_matches(':');
            let name = collapse_ws(head);
            if !name.is_empty() {
                meta.images.push(name);
            }
            continue;
        }
        if t.contains("Jump(") || t.contains("Call(") || t.contains("Start(") {
            for r in scan_refs(t) {
                if r.kind == RefKind::Action {
                    if let Some(n) = r.name {
                        meta.action_refs.push(n);
                    }
                }
            }
        }
        let body = if let Some(r) = t.strip_prefix('$') {
            r.trim_start()
        } else if let Some(r) = t
            .strip_prefix("define ")
            .or_else(|| t.strip_prefix("default "))
        {
            let r = r.trim_start();
            let first_end = r.find(char::is_whitespace).unwrap_or(r.len());
            let first = &r[..first_end];
            if first
                .chars()
                .next()
                .map(|c| c == '-' || c.is_ascii_digit())
                .unwrap_or(false)
            {
                r[first_end..].trim_start()
            } else {
                r
            }
        } else {
            t
        };
        let end = body
            .find(|c: char| !(c.is_alphanumeric() || c == '_' || c == '.'))
            .unwrap_or(body.len());
        if end == 0 {
            continue;
        }
        let name = &body[..end];
        let after = body[end..].trim_start();
        if after.starts_with('=') && !after.starts_with("==") {
            if let Some(key) = name.strip_prefix("config.") {
                meta.config
                    .push((key.to_string(), after[1..].trim().to_string()));
            } else if name == "build.name" {
                meta.config
                    .push(("build.name".to_string(), after[1..].trim().to_string()));
            } else if !name.contains('.') {
                meta.defs.push(name.to_string());
            }
        }
    }
    scan_notes(lines, &mut meta);
    meta
}

fn unquote_first(s: &str) -> String {
    let s = s.trim().trim_end_matches(':').trim();
    for q in ["\"\"\"", "'''", "\"", "'"] {
        if let Some(rest) = s.strip_prefix(q) {
            if let Some(i) = rest.find(q) {
                return rest[..i].to_string();
            }
        }
    }
    s.to_string()
}

fn quoted_in(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let q = b[i];
        if q == b'"' || q == b'\'' {
            let start = i + 1;
            i += 1;
            while i < b.len() && b[i] != q {
                if b[i] == b'\\' {
                    i += 1;
                }
                if i < b.len() {
                    i += 1;
                }
            }
            if i < b.len() && start <= i {
                out.push(s[start..i].to_string());
            }
            i += 1;
            continue;
        }
        i += 1;
    }
    out
}

fn looks_like_file(s: &str) -> bool {
    let lower = s.to_ascii_lowercase();
    lower.contains('.')
        && [
            ".png", ".jpg", ".jpeg", ".webp", ".gif", ".avif", ".ogg", ".mp3", ".wav", ".opus",
            ".webm", ".mp4", ".ogv",
        ]
        .iter()
        .any(|e| lower.ends_with(e))
}

/// Translation blocks and quoted asset paths. Comments are already stripped.
fn scan_notes(lines: &[LLine], meta: &mut Meta) {
    let mut lang = String::new();
    let mut strings = false;
    let mut block = false;
    let mut header_indent = 0u32;
    let mut pending_old: Option<(u32, String)> = None;

    for l in lines {
        let t = l.text.trim();
        if t.is_empty() {
            continue;
        }
        let closed = (strings || block)
            && l.indent <= header_indent
            && !t.starts_with("old ")
            && !t.starts_with("new ");
        if closed && !t.starts_with("translate ") {
            strings = false;
            block = false;
            pending_old = None;
        }
        if let Some(rest) = t.strip_prefix("translate ") {
            let mut parts = rest.split_whitespace();
            lang = parts.next().unwrap_or("").trim_end_matches(':').to_string();
            let ident = parts.next().unwrap_or("").trim_end_matches(':').to_string();
            header_indent = l.indent;
            pending_old = None;
            strings = ident == "strings";
            block = !strings && !lang.is_empty();
            continue;
        }
        if strings && l.indent > header_indent {
            if let Some(rest) = t.strip_prefix("old ") {
                pending_old = Some((l.line, unquote_first(rest)));
            } else if let Some(rest) = t.strip_prefix("new ") {
                if let Some((_, old)) = pending_old.take() {
                    let translated = unquote_first(rest);
                    meta.translates.push(TrNote {
                        line: l.line,
                        lang: lang.clone(),
                        ident: old.clone(),
                        source: old,
                        translated,
                        kind: "strings",
                    });
                }
            }
            continue;
        }
        if block && l.indent > header_indent {
            if let Some((_who, text)) = parse_say(t) {
                meta.translates.push(TrNote {
                    line: l.line,
                    lang: lang.clone(),
                    ident: String::new(),
                    source: String::new(),
                    translated: text,
                    kind: "block",
                });
            }
            continue;
        }
        let audio = t.starts_with("play ") || t.starts_with("queue ") || t.starts_with("voice ");
        let movie = t.contains("Movie(");
        let image = t.starts_with("image ") || t.starts_with("layeredimage ");
        if audio || movie || image {
            let kind = if audio {
                "audio"
            } else if movie {
                "movie"
            } else {
                "image"
            };
            for q in quoted_in(t) {
                if looks_like_file(&q) {
                    meta.quoted_paths.push(QuotedPath {
                        line: l.line,
                        kind,
                        path: q,
                    });
                }
            }
        }
        // Runs here, not in `scan_meta`, so a `translate` block (whose lines `continue`
        // above) is not searched for uses.
        collect_idents(t, l.line, &mut meta.idents);
        // A logical line can span several physical lines; rename works per physical line.
        for (k, part) in t.split('\n').enumerate() {
            collect_style_uses(part, l.line + k as u32, &mut meta.style_uses);
        }
    }
}

/// Statements whose first word is Ren'Py syntax, so the words after it are not
/// Python names (`show greet` is an image, not a call).
const RENPY_LINE: &[&str] = &[
    "show", "scene", "hide", "play", "jump", "call", "label", "image", "layeredimage", "with",
    "menu", "voice", "queue", "stop", "window", "pause", "translate", "nvl",
];

/// Record bare identifiers on a logical line. Quoted strings and comments are
/// skipped, as are attributes (`obj.greet`) and the name after `def` or `class`.
/// `text` joins physical lines with `\n`, and each hit gets its own physical line,
/// because a rename rewrites whole physical lines.
fn collect_idents(text: &str, first_line: u32, out: &mut HashMap<String, Vec<u32>>) {
    let mut line = first_line;
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    let first = ident_at(bytes, i);
    if let Some((_, word)) = first {
        if RENPY_LINE.contains(&word) {
            return;
        }
    }
    let mut prev_dot = false;
    let mut prev_def = false;
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'#' {
            // Comments are stripped by the lexer; this is only a safety net.
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if c == b'"' || c == b'\'' {
            i += 1;
            while i < bytes.len() && bytes[i] != c {
                if bytes[i] == b'\\' {
                    i += 1;
                }
                if i < bytes.len() {
                    if bytes[i] == b'\n' {
                        line += 1;
                    }
                    i += 1;
                }
            }
            i += 1;
            prev_dot = false;
            prev_def = false;
            continue;
        }
        if c == b'\n' {
            line += 1;
            i += 1;
            continue;
        }
        if c == b'.' {
            prev_dot = true;
            prev_def = false;
            i += 1;
            continue;
        }
        if c.is_ascii_alphabetic() || c == b'_' {
            let start = i;
            i += 1;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let word = &text[start..i];
            let is_def = word == "def" || word == "class";
            if !prev_dot && !prev_def {
                let lines = out.entry(word.to_string()).or_default();
                if lines.last() != Some(&line) {
                    lines.push(line);
                }
            }
            prev_dot = false;
            prev_def = is_def;
            continue;
        }
        // Whitespace keeps the `def`/`class` and `.` flags, so `def name` and
        // `obj . name` are still recognised.
        if !c.is_ascii_whitespace() {
            prev_dot = false;
            prev_def = false;
        }
        i += 1;
    }
}

fn ident_at(bytes: &[u8], mut i: usize) -> Option<(usize, &str)> {
    if i >= bytes.len() || !(bytes[i].is_ascii_alphabetic() || bytes[i] == b'_') {
        return None;
    }
    let start = i;
    i += 1;
    while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
        i += 1;
    }
    Some((i, std::str::from_utf8(&bytes[start..i]).unwrap_or("")))
}

fn is_style_key(word: &str) -> bool {
    word == "style" || word.ends_with("_style")
}

/// Exact style names: the parent after `is` on a `style` header, `style "name"`
/// and `*_style "name"` properties, and `style.name`. `style_prefix` derives
/// names (`name_text`), so it is not a style key and is left out.
fn collect_style_uses(text: &str, line: u32, out: &mut Vec<(String, u32)>) {
    let t = text.trim();
    let bytes = t.as_bytes();
    // `style name:` and `style name is parent:` declare a style. Only the parent is a use.
    let header = t.starts_with("style ")
        && !t["style ".len()..].trim_start().starts_with(['"', '\''])
        && (t.ends_with(':') || t.contains(" is "));
    let mut i = 0;
    let mut prev = "";
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'#' {
            break;
        }
        if c == b'"' || c == b'\'' {
            let start = i + 1;
            i += 1;
            while i < bytes.len() && bytes[i] != c {
                if bytes[i] == b'\\' {
                    i += 1;
                }
                if i < bytes.len() {
                    i += 1;
                }
            }
            if is_style_key(prev) && start <= i {
                let name = &t[start..i];
                if !name.is_empty() {
                    out.push((name.to_string(), line));
                }
            }
            i += 1;
            prev = "";
            continue;
        }
        if c == b'.' {
            if prev == "style" {
                let name = ident_prefix(&t[i + 1..]);
                if !name.is_empty() {
                    out.push((name, line));
                }
            }
            prev = "";
            i += 1;
            continue;
        }
        if c.is_ascii_alphabetic() || c == b'_' {
            let start = i;
            i += 1;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let word = &t[start..i];
            if is_style_key(prev) && !header {
                out.push((word.to_string(), line));
            }
            if header && prev == "is" {
                out.push((word.to_string(), line));
            }
            prev = word;
            continue;
        }
        if !c.is_ascii_whitespace() {
            prev = "";
        }
        i += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::lex;

    fn parse_src(src: &str) -> ParseOut {
        parse(&lex(src).lines)
    }

    const GYM: &str = r#"label start:
    scene tvroom1a
    with Dissolve(0.3)
    "narration here."

    menu:
        "I don't like that type of cocky guy...":
            $ good += 1
            jump good1

        "Wow, look at that body... So hot!":
            $ bad += 1
            jump bad1

    label good1:
        $ renpy.block_rollback()
        a "I don't like that."
        jump gym

    label bad1:
        a "Wow."
        jump gym

    label gym:
        scene gym1
        e "So what should we do?"
"#;

    #[test]
    fn nested_labels_menu_and_jumps() {
        let out = parse_src(GYM);
        assert!(out.issues.is_empty(), "{:?}", out.issues);
        assert_eq!(out.stmts.len(), 1);
        let Kind::Label { name, body } = &out.stmts[0].kind else {
            panic!()
        };
        assert_eq!(name, "start");
        let labels: Vec<&str> = body
            .iter()
            .filter_map(|s| match &s.kind {
                Kind::Label { name, .. } => Some(name.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(labels, ["good1", "bad1", "gym"]);
        let menu = body.iter().find_map(|s| match &s.kind {
            Kind::Menu { choices, .. } => Some(choices),
            _ => None,
        });
        let choices = menu.unwrap();
        assert_eq!(choices.len(), 2);
        assert_eq!(choices[0].text, "I don't like that type of cocky guy...");
        assert!(matches!(&choices[0].body[1].kind, Kind::Jump { target, .. } if target == "good1"));
    }

    #[test]
    fn named_menu_conditions_and_if_chain() {
        let src = "label x:\n    menu chat:\n        \"Hi\" if met:\n            jump a\n        \"Bye\":\n            jump b\n    if tatu:\n        show a1\n    elif sexy or thug:\n        show a2\n    else:\n        show a3\n    return\n";
        let out = parse_src(src);
        let Kind::Label { body, .. } = &out.stmts[0].kind else {
            panic!()
        };
        let Kind::Menu { name, choices, .. } = &body[0].kind else {
            panic!()
        };
        assert_eq!(name.as_deref(), Some("chat"));
        assert_eq!(choices[0].cond.as_deref(), Some("met"));
        assert!(choices[1].cond.is_none());
        let Kind::If { branches } = &body[1].kind else {
            panic!()
        };
        assert_eq!(branches.len(), 3);
        assert_eq!(branches[1].cond, "sexy or thug");
        assert!(is_cosmetic(&branches[0].body));
        assert!(matches!(body[2].kind, Kind::Return));
    }

    #[test]
    fn say_forms() {
        assert_eq!(parse_say("e \"Hi\""), Some((Some("e".into()), "Hi".into())));
        assert_eq!(
            parse_say("e happy \"Hi\""),
            Some((Some("e".into()), "Hi".into()))
        );
        assert_eq!(parse_say("\"Narration\""), Some((None, "Narration".into())));
        assert_eq!(parse_say("foo = 3"), None);
        assert_eq!(parse_say("mystery"), None);
    }

    #[test]
    fn local_labels_and_call_from() {
        let src = "label main:\n    jump .loop\nlabel .loop:\n    call helper from _call_helper_1\n    return\n";
        let out = parse_src(src);
        let Kind::Jump { target, .. } = &(match &out.stmts[0].kind {
            Kind::Label { body, .. } => &body[0],
            _ => panic!(),
        })
        .kind
        else {
            panic!()
        };
        assert_eq!(target, "main.loop");
        let Kind::Label { name, body } = &out.stmts[1].kind else {
            panic!()
        };
        assert_eq!(name, "main.loop");
        assert!(matches!(&body[0].kind, Kind::Call { from: Some(f), .. } if f == "_call_helper_1"));
    }

    #[test]
    fn opaque_blocks_and_refs() {
        let src = "screen s():\n    textbutton \"x\" action Jump(\"outro\")\ninit python:\n    def f():\n        renpy.jump(\"zz\")\nlabel start:\n    $ renpy.jump(\"end\")\n";
        let out = parse_src(src);
        assert_eq!(out.stmts.len(), 3);
        let Kind::Screen { name, labels, .. } = &out.stmts[0].kind else {
            panic!()
        };
        assert_eq!(name, "s");
        assert_eq!(
            labels.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(),
            ["outro"]
        );
        assert_eq!(labels[0].line, 2);
        assert_eq!(out.stmts[0].end_line, 2);
        let Kind::Python { refs, .. } = &out.stmts[1].kind else {
            panic!()
        };
        assert_eq!(refs[0].name.as_deref(), Some("zz"));
        let meta = scan_meta(&lex(src).lines);
        assert_eq!(meta.action_refs, ["outro"]);
    }

    #[test]
    fn screens_uses_and_screen_calls() {
        let src = "screen hub():\n    use bar\n    textbutton \"a\" action [Show(\"info\"), Jump(\"shop\")]\n    textbutton \"b\" action Function(renpy.call, \"x\")\nlabel start:\n    call screen hub\n    show screen hud(1) with dissolve\n    $ renpy.show_screen(\"phone\")\n    $ renpy.jump(\"v%s\" % n)\n";
        let out = parse_src(src);
        let Kind::Screen { name, labels, uses } = &out.stmts[0].kind else {
            panic!()
        };
        assert_eq!(name, "hub");
        assert_eq!(
            labels.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(),
            ["shop"]
        );
        assert_eq!(
            uses.iter().map(|u| u.name.as_str()).collect::<Vec<_>>(),
            ["bar", "info"]
        );
        assert_eq!(uses[0].line, 2);
        assert_eq!(labels[0].line, 3);
        let Kind::Label { body, .. } = &out.stmts[1].kind else {
            panic!()
        };
        assert!(matches!(&body[0].kind, Kind::ScreenRef { how: "call", name } if name == "hub"));
        assert!(matches!(&body[1].kind, Kind::ScreenRef { how: "show", name } if name == "hud"));
        let Kind::Python { refs, .. } = &body[2].kind else {
            panic!()
        };
        assert_eq!(refs[0].name.as_deref(), Some("phone"));
        // A computed target is dynamic, not a label named `v%s`.
        let Kind::Python { refs, .. } = &body[3].kind else {
            panic!()
        };
        assert_eq!(refs[0].name, None);
    }

    #[test]
    fn meta_collects_defs_images_config() {
        let src = "define e = Character(\"Eileen\")\ndefault points = 0\nimage ice cream = \"x.png\"\ndefine config.name = _(\"Game\")\ndefine build.name = \"game\"\ninit python:\n    narr = Character(None)\n";
        let meta = scan_meta(&lex(src).lines);
        assert!(meta.defs.contains(&"e".to_string()));
        assert!(meta.defs.contains(&"points".to_string()));
        assert!(meta.defs.contains(&"narr".to_string()));
        assert_eq!(meta.images, ["ice cream"]);
        assert_eq!(meta.config[0].0, "name");
        assert!(meta.config.iter().any(|(k, v)| k == "build.name" && v == "\"game\""));
    }

    #[test]
    fn check_syntax_reports_an_unterminated_string() {
        let issues = check_syntax("label start:\n    \"hello\n");
        assert!(issues.iter().any(|i| i.message.contains("Unterminated")));
        assert!(check_syntax("label start:\n    \"hello\"\n    return\n").is_empty());
    }

    #[test]
    fn meta_collects_idents_and_style_uses() {
        let src = r##"
init python:
    def greet(who):
        return who
    class Box:
        def method(self):
            pass
    x = greet(1)
    y = obj.greet(2)
    z = "greet"
label start:
    $ greet("x")
    if greet():
        e "Hello"
    show greet
screen phone():
    textbutton "Go" style "big" text_style "my_text" action Function(greet)
style say_dialogue is default:
    color "#fff"
style say_thought:
    pass
$ style.big.color = "#000"
"##;
        let lexed = lex(src);
        let meta = scan_meta(&lexed.lines);
        let has = |name: &str| meta.idents.get(name).map(|v| !v.is_empty()).unwrap_or(false);
        // A call is recorded; the `def` name, an attribute and a quoted string are not.
        assert!(has("greet"));
        assert!(!has("method"));
        let def_line = lexed
            .lines
            .iter()
            .find(|l| l.text.trim().starts_with("def greet"))
            .unwrap()
            .line;
        assert!(!meta.idents.get("greet").unwrap().contains(&def_line));
        assert!(meta.style_uses.iter().any(|(n, _)| n == "default"));
        assert!(meta.style_uses.iter().any(|(n, _)| n == "big"));
        assert!(meta.style_uses.iter().any(|(n, _)| n == "my_text"));
        // A `style name:` header is a declaration, not a use of that name.
        assert!(!meta.style_uses.iter().any(|(n, _)| n == "say_thought"));
        assert!(!meta.style_uses.iter().any(|(n, _)| n == "say_dialogue"));
    }

    #[test]
    fn idents_keep_their_physical_line_in_a_multi_line_statement() {
        let src = "init python:\n    def greet(n):\n        return n\ndefine table = [\n    greet(1),\n    \"greet\",\n    greet(2),\n]\n";
        let meta = scan_meta(&lex(src).lines);
        // Lines 5 and 7 hold the calls; the string on line 6 and the def on line 2 do not count.
        assert_eq!(meta.idents.get("greet").unwrap(), &vec![5, 7]);
    }

    #[test]
    fn image_name_extraction() {
        assert_eq!(
            image_name_of("eileen happy at left with Dissolve(0.3)").as_deref(),
            Some("eileen happy")
        );
        assert_eq!(image_name_of("expression \"foo\""), None);
        assert_eq!(image_name_of("black").as_deref(), Some("black"));
    }
}
