//! Symbols, variables, translations, dialogue stats and search helpers.
//! Built from the statement tree the parser already produced.

use std::collections::{HashMap, HashSet};

use serde::Serialize;

use crate::analysis::{walk_all, MapNode};
use crate::ast::*;
use crate::diagnostics::Diagnostic;
use crate::parser::TrNote;
use crate::project::SourceFile;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Symbol {
    pub kind: String,
    pub name: String,
    pub path: String,
    pub line: u32,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Variable {
    pub name: String,
    pub keyword: String,
    pub path: String,
    pub line: u32,
    pub value: String,
    pub uses: u32,
    pub persistent: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Translation {
    pub path: String,
    pub line: u32,
    pub lang: String,
    pub kind: String,
    pub source: String,
    pub translated: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageStat {
    pub lang: String,
    pub total: u32,
    pub translated: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeakerStat {
    pub name: String,
    pub lines: u32,
    pub words: u32,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DialogueStats {
    pub lines: u32,
    pub words: u32,
    pub minutes: u32,
    pub speakers: Vec<SpeakerStat>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub path: String,
    pub line: u32,
    pub text: String,
    pub kind: String,
    pub speaker: String,
}

/// One declaration or use of a name. Kept on the analysis, not sent to the UI.
#[derive(Debug, Clone)]
pub struct Occurrence {
    pub kind: String,
    pub name: String,
    /// `decl` or `use`.
    pub role: &'static str,
    pub path: String,
    pub line: u32,
}

#[derive(Debug, Clone, Default)]
pub struct Catalog {
    pub symbols: Vec<Symbol>,
    pub variables: Vec<Variable>,
    pub translations: Vec<Translation>,
    pub languages: Vec<LanguageStat>,
    pub dialogue: DialogueStats,
    pub diags: Vec<Diagnostic>,
    pub occurrences: Vec<Occurrence>,
}

const SKIP_IDENT: &[&str] = &[
    "and",
    "or",
    "not",
    "in",
    "is",
    "if",
    "elif",
    "else",
    "True",
    "False",
    "None",
    "renpy",
    "config",
    "persistent",
    "store",
    "gui",
    "define",
    "default",
    "menu",
    "jump",
    "call",
    "label",
    "show",
    "scene",
    "hide",
    "with",
    "play",
    "stop",
    "voice",
    "at",
    "as",
    "from",
    "pass",
    "return",
    "while",
    "for",
    "python",
    "init",
    "screen",
    "text",
    "action",
    "Set",
    "Jump",
    "Call",
    "Show",
    "Function",
    "dissolve",
    "fade",
    "None",
];

const CLOSING_TAGS: &[&str] = &[
    "i",
    "b",
    "u",
    "s",
    "color",
    "size",
    "font",
    "alpha",
    "outlinecolor",
    "a",
];

struct VarAcc {
    keyword: &'static str,
    path: String,
    line: u32,
    value: String,
    uses: u32,
}

pub fn build(files: &[SourceFile]) -> Catalog {
    let mut symbols: Vec<Symbol> = Vec::new();
    let mut vars: HashMap<String, VarAcc> = HashMap::new();
    let mut speakers: HashMap<String, (u32, u32)> = HashMap::new();
    let mut translations: Vec<Translation> = Vec::new();
    let mut diags: Vec<Diagnostic> = Vec::new();
    let mut declared: HashSet<String> = HashSet::new();
    let mut characters: HashSet<String> = HashSet::new();
    let mut say_lines = 0u32;
    let mut say_words = 0u32;
    let mut sites: Vec<Occurrence> = Vec::new();

    for f in files {
        walk_all(&f.stmts, &mut |s| match &s.kind {
            Kind::Label { name, .. } => {
                symbols.push(Symbol {
                    kind: "label".into(),
                    name: name.clone(),
                    path: f.rel.clone(),
                    line: s.line,
                    detail: String::new(),
                });
                push_site(&mut sites, "label", name, "decl", &f.rel, s.line);
            }
            Kind::Screen { name, labels, uses } => {
                symbols.push(Symbol {
                    kind: "screen".into(),
                    name: name.clone(),
                    path: f.rel.clone(),
                    line: s.line,
                    detail: String::new(),
                });
                push_site(&mut sites, "screen", name, "decl", &f.rel, s.line);
                for link in labels {
                    push_site(&mut sites, "label", &link.name, "use", &f.rel, link.line);
                }
                for link in uses {
                    push_site(&mut sites, "screen", &link.name, "use", &f.rel, link.line);
                }
            }
            Kind::Image { name } => {
                let detail = f
                    .meta
                    .quoted_paths
                    .iter()
                    .find(|q| q.line == s.line)
                    .map(|q| q.path.clone())
                    .unwrap_or_default();
                symbols.push(Symbol {
                    kind: "image".into(),
                    name: name.clone(),
                    path: f.rel.clone(),
                    line: s.line,
                    detail,
                });
                push_site(&mut sites, "image", name, "decl", &f.rel, s.line);
            }
            Kind::Transform { name } => {
                symbols.push(Symbol {
                    kind: "transform".into(),
                    name: name.clone(),
                    path: f.rel.clone(),
                    line: s.line,
                    detail: String::new(),
                });
                push_site(&mut sites, "transform", name, "decl", &f.rel, s.line);
            }
            Kind::Define {
                keyword,
                name,
                value,
            } => {
                if is_character(value) {
                    let (who, color) = character_bits(value);
                    characters.insert(name.clone());
                    symbols.push(Symbol {
                        kind: "character".into(),
                        name: name.clone(),
                        path: f.rel.clone(),
                        line: s.line,
                        detail: match (who, color) {
                            (Some(w), Some(c)) => format!("{w} · {c}"),
                            (Some(w), None) => w,
                            (None, Some(c)) => c,
                            _ => value.clone(),
                        },
                    });
                    push_site(&mut sites, "character", name, "decl", &f.rel, s.line);
                } else if !name.is_empty()
                    && !name.starts_with("config.")
                    && !name.starts_with("gui.")
                {
                    declared.insert(name.clone());
                    vars.entry(name.clone()).or_insert_with(|| VarAcc {
                        keyword,
                        path: f.rel.clone(),
                        line: s.line,
                        value: value.clone(),
                        uses: 0,
                    });
                    push_site(&mut sites, "variable", name, "decl", &f.rel, s.line);
                }
            }
            Kind::Jump { target, dynamic } if !*dynamic => {
                push_site(&mut sites, "label", target, "use", &f.rel, s.line);
            }
            Kind::Call {
                target, dynamic, ..
            } if !*dynamic => {
                push_site(&mut sites, "label", target, "use", &f.rel, s.line);
            }
            Kind::ScreenRef { name, .. } => {
                push_site(&mut sites, "screen", name, "use", &f.rel, s.line);
            }
            Kind::Present { cmd, name, text } => {
                if matches!(*cmd, "show" | "scene" | "hide") {
                    if let Some(name) = name {
                        push_site(&mut sites, "image", name, "use", &f.rel, s.line);
                    }
                }
                if matches!(*cmd, "show" | "scene") {
                    for transform in at_names(text) {
                        push_site(&mut sites, "transform", &transform, "use", &f.rel, s.line);
                    }
                }
            }
            Kind::Say { who, text } => {
                say_lines += 1;
                let w = word_count(text);
                say_words += w;
                let speaker = who.clone().unwrap_or_else(|| "narrator".into());
                let e = speakers.entry(speaker).or_insert((0, 0));
                e.0 += 1;
                e.1 += w;
                if let Some(who) = who {
                    push_site(&mut sites, "character", who, "use", &f.rel, s.line);
                }
                if let Some(msg) = tag_error(text) {
                    if diags.len() < 200 {
                        diags.push(Diagnostic {
                            severity: "warning",
                            code: "text-tag",
                            message: msg,
                            path: f.rel.clone(),
                            line: s.line,
                            label: None,
                        });
                    }
                }
                for name in interpolations(text) {
                    note_use(
                        &mut vars,
                        &declared,
                        &characters,
                        &name,
                        &f.rel,
                        s.line,
                        &mut diags,
                        &mut sites,
                    );
                }
            }
            Kind::If { branches } => {
                for b in branches {
                    for name in idents(&b.cond) {
                        note_use(
                            &mut vars,
                            &declared,
                            &characters,
                            &name,
                            &f.rel,
                            b.line,
                            &mut diags,
                            &mut sites,
                        );
                    }
                }
            }
            Kind::While { cond, .. } => {
                for name in idents(cond) {
                    note_use(
                        &mut vars,
                        &declared,
                        &characters,
                        &name,
                        &f.rel,
                        s.line,
                        &mut diags,
                        &mut sites,
                    );
                }
            }
            Kind::Menu { choices, .. } => {
                for c in choices {
                    if let Some(cond) = &c.cond {
                        for name in idents(cond) {
                            note_use(
                                &mut vars,
                                &declared,
                                &characters,
                                &name,
                                &f.rel,
                                c.line,
                                &mut diags,
                                &mut sites,
                            );
                        }
                    }
                }
            }
            Kind::Python { block, text, refs } => {
                for r in refs {
                    let Some(name) = r.name.as_deref() else {
                        continue;
                    };
                    let line = if r.line > 0 { r.line } else { s.line };
                    match r.kind {
                        RefKind::RenpyJump | RefKind::RenpyCall | RefKind::Action => {
                            push_site(&mut sites, "label", name, "use", &f.rel, line);
                        }
                        RefKind::Screen | RefKind::ScreenAction => {
                            push_site(&mut sites, "screen", name, "use", &f.rel, line);
                        }
                    }
                }
                if !*block {
                    for name in idents(text) {
                        note_use(
                            &mut vars,
                            &declared,
                            &characters,
                            &name,
                            &f.rel,
                            s.line,
                            &mut diags,
                            &mut sites,
                        );
                    }
                }
            }
            _ => {}
        });

        for note in &f.meta.translates {
            translations.push(note_to_tr(&f.rel, note));
            if note.kind == "strings" && note.translated.trim().is_empty() && diags.len() < 400 {
                diags.push(Diagnostic {
                    severity: "warning",
                    code: "untranslated",
                    message: format!(
                        "`{}` has no {} translation.",
                        truncate(&note.source, 80),
                        note.lang
                    ),
                    path: f.rel.clone(),
                    line: note.line,
                    label: None,
                });
            }
        }
    }

    let mut variables: Vec<Variable> = vars
        .into_iter()
        .map(|(name, v)| Variable {
            persistent: name.starts_with("persistent."),
            name,
            keyword: v.keyword.to_string(),
            path: v.path,
            line: v.line,
            value: v.value,
            uses: v.uses,
        })
        .collect();
    variables.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

    for v in variables
        .iter()
        .filter(|v| v.uses == 0 && !v.name.starts_with('_'))
        .take(80)
    {
        diags.push(Diagnostic {
            severity: "info",
            code: "unused-var",
            message: format!("`{}` is {} and never used.", v.name, v.keyword),
            path: v.path.clone(),
            line: v.line,
            label: None,
        });
    }

    let mut speakers: Vec<SpeakerStat> = speakers
        .into_iter()
        .map(|(name, (lines, words))| SpeakerStat { name, lines, words })
        .collect();
    speakers.sort_by(|a, b| b.lines.cmp(&a.lines).then_with(|| a.name.cmp(&b.name)));

    let mut lang_total: HashMap<String, (u32, u32)> = HashMap::new();
    for t in &translations {
        let e = lang_total.entry(t.lang.clone()).or_insert((0, 0));
        e.0 += 1;
        if !t.translated.trim().is_empty() {
            e.1 += 1;
        }
    }
    let mut languages: Vec<LanguageStat> = lang_total
        .into_iter()
        .map(|(lang, (total, translated))| LanguageStat {
            lang,
            total,
            translated,
        })
        .collect();
    languages.sort_by(|a, b| a.lang.cmp(&b.lang));

    symbols.sort_by(|a, b| {
        a.kind
            .cmp(&b.kind)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.path.cmp(&b.path))
            .then_with(|| a.line.cmp(&b.line))
    });

    Catalog {
        symbols,
        variables,
        translations,
        languages,
        dialogue: DialogueStats {
            lines: say_lines,
            words: say_words,
            minutes: say_words / 200,
            speakers,
        },
        diags,
        occurrences: sites,
    }
}

fn note_to_tr(path: &str, note: &TrNote) -> Translation {
    Translation {
        path: path.to_string(),
        line: note.line,
        lang: note.lang.clone(),
        kind: note.kind.to_string(),
        source: note.source.clone(),
        translated: note.translated.clone(),
    }
}

fn note_use(
    vars: &mut HashMap<String, VarAcc>,
    declared: &HashSet<String>,
    characters: &HashSet<String>,
    name: &str,
    path: &str,
    line: u32,
    diags: &mut Vec<Diagnostic>,
    sites: &mut Vec<Occurrence>,
) {
    if let Some(v) = vars.get_mut(name) {
        v.uses = v.uses.saturating_add(1);
        push_site(sites, "variable", name, "use", path, line);
        return;
    }
    if name.len() < 2
        || declared.contains(name)
        || characters.contains(name)
        || SKIP_IDENT.contains(&name)
    {
        return;
    }
    if name.starts_with('_') || name.contains('.') && !name.starts_with("persistent.") {
        return;
    }
    if diags.iter().filter(|d| d.code == "undeclared-var").count() >= 80 {
        return;
    }
    if diags
        .iter()
        .any(|d| d.code == "undeclared-var" && d.message.contains(&format!("`{name}`")))
    {
        return;
    }
    diags.push(Diagnostic {
        severity: "warning",
        code: "undeclared-var",
        message: format!("`{name}` is used here but not defined with define or default."),
        path: path.to_string(),
        line,
        label: None,
    });
}

pub fn flow_diags(nodes: &[MapNode], files: &[SourceFile]) -> Vec<Diagnostic> {
    let mut items = Vec::new();
    for n in nodes {
        if n.kind != "label" || !n.reachable || n.file < 0 {
            continue;
        }
        if n.out_degree == 0 && n.menus > 0 && !n.returns && !n.ends_script {
            let path = files
                .get(n.file as usize)
                .map(|f| f.rel.clone())
                .unwrap_or_default();
            items.push(Diagnostic {
                severity: "warning",
                code: "dead-end",
                message: format!("`{}` has a menu and then nowhere to go.", n.id),
                path,
                line: n.line,
                label: Some(n.id.clone()),
            });
        }
    }
    items
}

/// Labels that end the story, and labels whose menu goes nowhere.
pub fn endings(nodes: &[MapNode]) -> (Vec<String>, Vec<String>) {
    let mut ends = Vec::new();
    let mut dead = Vec::new();
    for n in nodes {
        if n.kind != "label" || !n.reachable || n.out_degree > 0 {
            continue;
        }
        if n.menus > 0 && !n.returns && !n.ends_script {
            dead.push(n.id.clone());
        } else if n.returns || n.ends_script {
            ends.push(n.id.clone());
        }
    }
    ends.truncate(40);
    dead.truncate(40);
    (ends, dead)
}

/// Shortest paths on the project map that reach `target`, from any entry label.
pub fn routes_to(
    nodes: &[MapNode],
    edges: &[crate::analysis::MapEdge],
    target: &str,
    limit: usize,
) -> Vec<Vec<String>> {
    let mut incoming: HashMap<&str, Vec<&str>> = HashMap::new();
    for e in edges {
        incoming
            .entry(e.to.as_str())
            .or_default()
            .push(e.from.as_str());
    }
    let roots: HashSet<&str> = nodes
        .iter()
        .filter(|n| n.root)
        .map(|n| n.id.as_str())
        .collect();
    let mut out = Vec::new();
    let mut stack = vec![vec![target]];
    while let Some(path) = stack.pop() {
        if out.len() >= limit {
            break;
        }
        let head = path[0];
        if roots.contains(head) || path.len() > 8 {
            if roots.contains(head) {
                out.push(path.iter().copied().map(|s| s.to_string()).collect());
            }
            continue;
        }
        let preds = incoming.get(head).cloned().unwrap_or_default();
        if preds.is_empty() && path.len() > 1 {
            continue;
        }
        for p in preds {
            if path.contains(&p) {
                continue;
            }
            let mut next = vec![p];
            next.extend(path.iter().copied());
            stack.push(next);
            if stack.len() > 400 {
                break;
            }
        }
    }
    out
}

pub fn search_dialogue(files: &[SourceFile], query: &str, limit: usize) -> Vec<SearchHit> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    let mut hits = Vec::new();
    for f in files {
        if hits.len() >= limit {
            break;
        }
        walk_all(&f.stmts, &mut |s| {
            if hits.len() >= limit {
                return;
            }
            if let Kind::Say { who, text } = &s.kind {
                if text.to_lowercase().contains(&q)
                    || who
                        .as_ref()
                        .map(|w| w.to_lowercase().contains(&q))
                        .unwrap_or(false)
                {
                    hits.push(SearchHit {
                        path: f.rel.clone(),
                        line: s.line,
                        text: truncate(text, 180),
                        kind: "dialogue".into(),
                        speaker: who.clone().unwrap_or_else(|| "narrator".into()),
                    });
                }
            }
        });
    }
    hits
}

pub fn search_lines(path: &str, text: &str, query: &str, limit: usize, hits: &mut Vec<SearchHit>) {
    let q = query.to_lowercase();
    if q.is_empty() {
        return;
    }
    for (i, line) in text.split('\n').enumerate() {
        if hits.len() >= limit {
            return;
        }
        let shown = line.trim_end().trim_start_matches('\r');
        if shown.to_lowercase().contains(&q) {
            hits.push(SearchHit {
                path: path.to_string(),
                line: (i + 1) as u32,
                text: truncate(shown.trim(), 180),
                kind: "text".into(),
                speaker: String::new(),
            });
        }
    }
}

pub fn line_is_reference(line: &str, name: &str, kind: &str) -> bool {
    if name.is_empty() || !contains_name(line, name) {
        return false;
    }
    let code = line.split('#').next().unwrap_or(line);
    match kind {
        "label" => {
            keyword_before(code, name, &["label", "jump", "call", "menu"])
                || quoted_call(code, name)
                || contains_word(code, name)
                    && (code.contains("Jump(") || code.contains("Call(") || code.contains("Start("))
        }
        "screen" => {
            code.contains("screen") && (contains_word(code, name) || quoted_exact(code, name))
        }
        "image" => contains_word(code, name) || quoted_exact(code, name),
        "character" => contains_word(code, name),
        "variable" => contains_word(code, name) || code.contains(&format!("[{name}")),
        _ => contains_word(code, name),
    }
}

pub fn rename_in_line(line: &str, old: &str, new: &str) -> String {
    if old.is_empty() || old == new {
        return line.to_string();
    }
    let mut out = String::new();
    let mut rest = line;
    let mut in_comment = false;
    while let Some(ch) = rest.chars().next() {
        let n = ch.len_utf8();
        if in_comment {
            out.push(ch);
            rest = &rest[n..];
            continue;
        }
        if ch == '#' {
            in_comment = true;
            out.push('#');
            rest = &rest[n..];
            continue;
        }
        if ch == '"' || ch == '\'' {
            let q = ch;
            let mut consumed = n;
            let mut chars = rest[n..].chars();
            let mut closed = false;
            while let Some(c) = chars.next() {
                if c == '\\' {
                    consumed += c.len_utf8();
                    if let Some(escaped) = chars.next() {
                        consumed += escaped.len_utf8();
                    }
                    continue;
                }
                consumed += c.len_utf8();
                if c == q {
                    closed = true;
                    break;
                }
            }
            let content_end = if closed {
                consumed - q.len_utf8()
            } else {
                consumed
            };
            let content = &rest[n..content_end];
            if closed && content == old {
                out.push(q);
                out.push_str(new);
                out.push(q);
            } else {
                out.push_str(&rest[..consumed]);
            }
            rest = &rest[consumed..];
            continue;
        }
        if is_ident_start(ch) {
            let mut consumed = n;
            let mut chars = rest[n..].chars();
            while let Some(c) = chars.clone().next() {
                if !is_ident_cont(c) {
                    break;
                }
                chars.next();
                consumed += c.len_utf8();
            }
            let word = &rest[..consumed];
            if word == old {
                out.push_str(new);
            } else {
                out.push_str(word);
            }
            rest = &rest[consumed..];
            continue;
        }
        out.push(ch);
        rest = &rest[n..];
    }
    out
}

/// Replace the first quoted string on a translation line. Used by the side-by-side editor.
pub fn replace_quoted(line: &str, text: &str) -> String {
    let escaped = text.replace('\\', "\\\\").replace('"', "\\\"");
    let mut rest = line;
    let mut prefix = 0usize;
    while let Some(ch) = rest.chars().next() {
        let n = ch.len_utf8();
        if ch == '"' || ch == '\'' {
            let q = ch;
            let mut consumed = n;
            let mut chars = rest[n..].chars();
            let mut closed = false;
            while let Some(c) = chars.next() {
                if c == '\\' {
                    consumed += c.len_utf8();
                    if let Some(escaped_ch) = chars.next() {
                        consumed += escaped_ch.len_utf8();
                    }
                    continue;
                }
                consumed += c.len_utf8();
                if c == q {
                    closed = true;
                    break;
                }
            }
            if !closed {
                break;
            }
            let mut out = String::new();
            out.push_str(&line[..prefix]);
            out.push('"');
            out.push_str(&escaped);
            out.push('"');
            out.push_str(&line[prefix + consumed..]);
            return out;
        }
        prefix += n;
        rest = &rest[n..];
    }
    line.to_string()
}

fn push_site(
    sites: &mut Vec<Occurrence>,
    kind: &str,
    name: &str,
    role: &'static str,
    path: &str,
    line: u32,
) {
    if name.is_empty() {
        return;
    }
    sites.push(Occurrence {
        kind: kind.to_string(),
        name: name.to_string(),
        role,
        path: path.to_string(),
        line,
    });
}

const AT_STOP: &[&str] = &["with", "as", "behind", "zorder", "onlayer"];

/// Transform names after `at` on a show or scene line, up to `with` / `as` / ...
fn at_names(text: &str) -> Vec<String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let Some(i) = words.iter().position(|w| *w == "at") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for w in words.iter().skip(i + 1) {
        let bare = w.trim_matches(',');
        if bare.is_empty() {
            continue;
        }
        if AT_STOP.contains(&bare) {
            break;
        }
        let name: String = bare
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        if name.is_empty() {
            break;
        }
        out.push(name);
    }
    out
}

pub struct Resolved {
    pub hit: bool,
    pub symbol: Option<Symbol>,
}

/// Declaration for the word on this line. `hit` is false when the index has nothing there.
pub fn resolve(cat: &Catalog, path: &str, line: u32, name: &str, prefer: Option<&str>) -> Resolved {
    if name.is_empty() {
        return Resolved {
            hit: false,
            symbol: None,
        };
    }
    let mut hits: Vec<&Occurrence> = cat
        .occurrences
        .iter()
        .filter(|o| o.path == path && o.line == line && name_hit(o, name))
        .collect();
    if hits.is_empty() {
        return Resolved {
            hit: false,
            symbol: None,
        };
    }
    if let Some(pref) = prefer.map(str::trim).filter(|p| !p.is_empty()) {
        let preferred: Vec<&Occurrence> = hits.iter().copied().filter(|o| o.kind == pref).collect();
        if !preferred.is_empty() {
            hits = preferred;
        }
    }
    let chosen = hits
        .iter()
        .copied()
        .find(|o| o.role == "decl")
        .unwrap_or(hits[0]);
    Resolved {
        hit: true,
        symbol: symbol_of(cat, &chosen.kind, &chosen.name),
    }
}

fn name_hit(site: &Occurrence, word: &str) -> bool {
    if site.name == word {
        return true;
    }
    site.kind == "image" && site.name.split(' ').any(|part| part == word)
}

fn symbol_of(cat: &Catalog, kind: &str, name: &str) -> Option<Symbol> {
    if let Some(s) = cat
        .symbols
        .iter()
        .find(|s| s.kind == kind && s.name == name)
    {
        return Some(s.clone());
    }
    if kind == "image" {
        let tag = name.split(' ').next().unwrap_or(name);
        if let Some(s) = cat
            .symbols
            .iter()
            .find(|s| s.kind == "image" && s.name == tag)
        {
            return Some(s.clone());
        }
    }
    if kind == "variable" {
        if let Some(v) = cat.variables.iter().find(|v| v.name == name) {
            return Some(Symbol {
                kind: "variable".into(),
                name: v.name.clone(),
                path: v.path.clone(),
                line: v.line,
                detail: v.value.clone(),
            });
        }
    }
    cat.occurrences
        .iter()
        .find(|o| o.role == "decl" && o.kind == kind && o.name == name)
        .map(|o| Symbol {
            kind: kind.to_string(),
            name: name.to_string(),
            path: o.path.clone(),
            line: o.line,
            detail: String::new(),
        })
}

fn is_character(value: &str) -> bool {
    value.contains("Character(") || value.contains("DynamicCharacter(")
}

fn character_bits(value: &str) -> (Option<String>, Option<String>) {
    let who = quoted_in_first(value);
    let color = value.split("color").nth(1).and_then(|rest| {
        let rest = rest.trim_start_matches(|c: char| c == '=' || c.is_whitespace());
        let q = quoted_in_first(rest)?;
        Some(q)
    });
    (who, color)
}

fn quoted_in_first(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let start = b.iter().position(|c| *c == b'"' || *c == b'\'')?;
    let q = b[start];
    let rest = &s[start + 1..];
    let end = rest.find(q as char)?;
    let text = rest[..end].to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

fn is_ident_cont(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

fn idents(s: &str) -> Vec<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'"' || c == b'\'' {
            i += 1;
            while i < bytes.len() && bytes[i] != c {
                i += 1;
            }
            i += 1;
            continue;
        }
        if c == b'#' {
            break;
        }
        if c.is_ascii_alphabetic() || c == b'_' {
            let start = i;
            i += 1;
            while i < bytes.len()
                && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'.')
            {
                i += 1;
            }
            let word = &s[start..i];
            if word.starts_with("persistent.") && word.len() > "persistent.".len() {
                out.push(word.to_string());
            } else if !word.contains('.') && !SKIP_IDENT.contains(&word) {
                out.push(word.to_string());
            }
            continue;
        }
        i += 1;
    }
    out
}

fn interpolations(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find('[') {
        rest = &rest[start + 1..];
        if rest.starts_with('[') {
            rest = &rest[1..];
            continue;
        }
        let end = rest.find(']').unwrap_or(rest.len());
        let inner = rest[..end].trim();
        let name = inner
            .split(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '.')
            .next()
            .unwrap_or("");
        if name.starts_with("persistent.") || (!name.contains('.') && name.len() > 1) {
            out.push(name.to_string());
        }
        if end < rest.len() {
            rest = &rest[end + 1..];
        } else {
            break;
        }
    }
    out
}

fn tag_error(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut i = 0;
    let mut stack: Vec<String> = Vec::new();
    while i < bytes.len() {
        if bytes[i] == b'{' && i + 1 < bytes.len() && bytes[i + 1] == b'{' {
            i += 2;
            continue;
        }
        if bytes[i] != b'{' {
            i += 1;
            continue;
        }
        let start = i + 1;
        let mut j = start;
        while j < bytes.len() && bytes[j] != b'}' {
            j += 1;
        }
        if j >= bytes.len() {
            return Some("A `{` in dialogue is never closed.".into());
        }
        let inner = &text[start..j];
        i = j + 1;
        if inner == "plain" {
            stack.clear();
            continue;
        }
        if let Some(name) = inner.strip_prefix('/') {
            let name = tag_name(name);
            if !CLOSING_TAGS.contains(&name) {
                continue;
            }
            if stack.pop().as_deref() != Some(name) {
                return Some(format!(
                    "`{{/{name}}}` does not close the tag that is open."
                ));
            }
            continue;
        }
        let name = tag_name(&inner);
        if CLOSING_TAGS.contains(&name) {
            stack.push(name.to_string());
        }
    }
    stack
        .last()
        .map(|name| format!("`{{{name}}}` is never closed."))
}

fn tag_name(s: &str) -> &str {
    s.split(|c: char| c == '=' || c.is_whitespace())
        .next()
        .unwrap_or(s)
}

fn word_count(text: &str) -> u32 {
    text.split_whitespace()
        .filter(|w| w.chars().any(|c| c.is_alphanumeric()))
        .count() as u32
}

fn contains_word(line: &str, name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let mut rest = line;
    while let Some(pos) = rest.find(name) {
        let before_ok = rest[..pos]
            .chars()
            .next_back()
            .map(|c| !is_ident_cont(c))
            .unwrap_or(true);
        let after = rest[pos + name.len()..].chars().next();
        let after_ok = after.map(|c| !is_ident_cont(c)).unwrap_or(true);
        if before_ok && after_ok {
            return true;
        }
        let step = rest[pos..]
            .chars()
            .next()
            .map(|c| c.len_utf8())
            .unwrap_or(1);
        rest = &rest[pos + step..];
    }
    false
}

fn contains_name(line: &str, name: &str) -> bool {
    contains_word(line, name) || quoted_exact(line, name)
}

fn quoted_exact(line: &str, name: &str) -> bool {
    line.contains(&format!("\"{name}\"")) || line.contains(&format!("'{name}'"))
}

fn quoted_call(line: &str, name: &str) -> bool {
    ["Jump", "Call", "Start", "Show", "ShowMenu"]
        .iter()
        .any(|k| {
            line.contains(&format!("{k}(\"{name}\"")) || line.contains(&format!("{k}('{name}'"))
        })
}

fn keyword_before(line: &str, name: &str, keywords: &[&str]) -> bool {
    let Some(at) = line.find(name) else {
        return false;
    };
    let before = line[..at].split_whitespace().last().unwrap_or("");
    keywords.iter().any(|k| before == *k || before.ends_with(k))
}

fn truncate(s: &str, max: usize) -> String {
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i >= max {
            out.push('…');
            break;
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::lex;
    use crate::parser::{parse, scan_meta};
    use crate::project::{Origin, SourceFile};

    fn file(src: &str) -> SourceFile {
        let lexed = lex(src);
        let parsed = parse(&lexed.lines);
        let mut meta = scan_meta(&lexed.lines);
        meta.data_refs = parsed.data_refs;
        SourceFile {
            rel: "script.rpy".into(),
            abs: Default::default(),
            origin: Origin::Loose,
            bytes: src.len() as u64,
            content_hash: 0,
            init_hash: 0,
            total_lines: lexed.total_lines,
            stmts: std::sync::Arc::new(parsed.stmts),
            issues: Vec::new(),
            opaque: parsed.opaque,
            meta: std::sync::Arc::new(meta),
            source: None,
            engine: None,
            decompile_reasons: Vec::new(),
        }
    }

    #[test]
    fn indexes_variables_characters_and_translations() {
        let src = r##"
define e = Character("Eileen", color="#c8ffc8")
default points = 0
default persistent.seen = False
label start:
    e "Hello, [points]."
    if points > 0:
        jump later
    return

translate french strings:
    old "Hello"
    new ""

translate french start_x:
    e "Bonjour."
"##;
        let cat = build(&[file(src)]);
        assert!(cat
            .symbols
            .iter()
            .any(|s| s.kind == "character" && s.name == "e" && s.detail.contains("Eileen")));
        assert!(cat
            .symbols
            .iter()
            .any(|s| s.kind == "label" && s.name == "start"));
        let points = cat.variables.iter().find(|v| v.name == "points").unwrap();
        assert!(points.uses >= 1, "interpolation and the if should count");
        assert!(cat
            .variables
            .iter()
            .any(|v| v.persistent && v.name == "persistent.seen"));
        assert!(cat
            .languages
            .iter()
            .any(|l| l.lang == "french" && l.total >= 2 && l.translated >= 1));
        assert!(cat.diags.iter().any(|d| d.code == "untranslated"));
        assert!(cat.dialogue.lines >= 1);
        assert_eq!(cat.dialogue.speakers[0].name, "e");
    }

    #[test]
    fn text_tags_and_unused_defaults() {
        let src = "default leftover = 1\nlabel start:\n    \"Hello {i}there.\"\n    return\n";
        let cat = build(&[file(src)]);
        assert!(cat.diags.iter().any(|d| d.code == "text-tag"));
        assert!(cat
            .diags
            .iter()
            .any(|d| d.code == "unused-var" && d.message.contains("leftover")));
    }

    #[test]
    fn rename_replaces_words_and_exact_strings() {
        assert_eq!(
            rename_in_line("    jump shop", "shop", "store"),
            "    jump store"
        );
        assert_eq!(
            rename_in_line("    $ renpy.jump(\"shop\")", "shop", "store"),
            "    $ renpy.jump(\"store\")"
        );
        assert!(line_is_reference("    jump shop", "shop", "label"));
        assert!(!line_is_reference(
            "    e \"a shop on the corner\"",
            "shop",
            "label"
        ));
        assert_eq!(
            replace_quoted("    new \"Hello\"", "Bonjour"),
            "    new \"Bonjour\""
        );
    }

    fn roles(cat: &Catalog, kind: &str, name: &str) -> Vec<(&'static str, u32)> {
        cat.occurrences
            .iter()
            .filter(|o| o.kind == kind && o.name == name)
            .map(|o| (o.role, o.line))
            .collect()
    }

    #[test]
    fn indexes_declarations_and_uses() {
        let src = "\
transform left:
    xalign 0.0
image eileen happy = \"e.png\"
define e = Character(\"Eileen\")
default points = 0
label start:
    show eileen happy at left
    e \"Hello, [points].\"
    e \"jump later in dialogue\"
    # jump later
    jump later
    call screen phone
    return
screen phone():
    textbutton \"Go\" action Jump(\"later\")
    use extra
label later:
    return
";
        let cat = build(&[file(src)]);
        assert!(cat
            .symbols
            .iter()
            .any(|s| s.kind == "transform" && s.name == "left" && s.line == 1));
        assert_eq!(
            roles(&cat, "label", "later"),
            vec![("use", 11), ("use", 15), ("decl", 17)]
        );
        assert_eq!(
            roles(&cat, "screen", "phone"),
            vec![("use", 12), ("decl", 14)]
        );
        assert_eq!(roles(&cat, "screen", "extra"), vec![("use", 16)]);
        assert_eq!(
            roles(&cat, "image", "eileen happy"),
            vec![("decl", 3), ("use", 7)]
        );
        assert_eq!(
            roles(&cat, "transform", "left"),
            vec![("decl", 1), ("use", 7)]
        );
        assert_eq!(
            roles(&cat, "character", "e"),
            vec![("decl", 4), ("use", 8), ("use", 9)]
        );
        assert!(roles(&cat, "variable", "points").contains(&("decl", 5)));
        assert!(roles(&cat, "variable", "points").contains(&("use", 8)));
        assert!(!cat.occurrences.iter().any(|o| o.line == 10));
        assert!(!cat
            .occurrences
            .iter()
            .any(|o| o.kind == "label" && o.name == "later" && o.line == 9));

        let jump = resolve(&cat, "script.rpy", 11, "later", Some("label"));
        assert!(jump.hit);
        assert_eq!(jump.symbol.unwrap().line, 17);

        let image = resolve(&cat, "script.rpy", 7, "eileen", Some("image"));
        assert_eq!(image.symbol.unwrap().name, "eileen happy");

        let transform = resolve(&cat, "script.rpy", 7, "left", Some("transform"));
        assert_eq!(transform.symbol.unwrap().kind, "transform");

        let speaker = resolve(&cat, "script.rpy", 8, "e", Some("character"));
        assert_eq!(speaker.symbol.unwrap().kind, "character");

        let points = resolve(&cat, "script.rpy", 8, "points", None);
        assert_eq!(points.symbol.unwrap().kind, "variable");

        let dialogue = resolve(&cat, "script.rpy", 9, "later", Some("label"));
        assert!(!dialogue.hit);
        assert!(dialogue.symbol.is_none());
    }

    #[test]
    fn dialogue_search_finds_a_line() {
        let src = "label start:\n    e \"The hidden gate opens.\"\n    return\n";
        let hits = search_dialogue(&[file(src)], "hidden gate", 10);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].speaker, "e");
        assert_eq!(hits[0].kind, "dialogue");
    }
}
