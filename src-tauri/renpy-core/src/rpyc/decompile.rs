//! Line-aligned `.rpy` text. A statement is placed on its original line when
//! the lines so far still leave that line free.

use super::ast::{Kind, MenuItem, Node, Tree};

/// Rendered script plus the compiled linenumber of each output line.
/// Blank and continuation lines keep the previous number, then any leading
/// gap takes the next known number.
pub fn render_mapped(tree: &Tree) -> (String, Vec<u32>) {
    let mut rows: Vec<(u32, String)> = Vec::new();
    emit_nodes(&tree.nodes, 0, &mut rows);
    join_rows(&rows)
}

fn emit_nodes(nodes: &[Node], indent: usize, rows: &mut Vec<(u32, String)>) {
    for node in nodes {
        emit(node, indent, rows);
    }
}

fn emit(node: &Node, indent: usize, rows: &mut Vec<(u32, String)>) {
    let pad = " ".repeat(indent);
    match &node.kind {
        Kind::Label {
            name,
            params,
            hide,
            body,
        } => {
            let params = if params.is_empty() {
                String::new()
            } else {
                format!("({params})")
            };
            let hide = if *hide { " hide" } else { "" };
            line(rows, node.line, format!("{pad}label {name}{params}{hide}:"));
            emit_nodes(body, indent + 4, rows);
        }
        Kind::Say { who, what, attrs } => {
            let mut head = String::new();
            if let Some(who) = who {
                head.push_str(who);
                for attr in attrs {
                    head.push_str(" @ ");
                    head.push_str(attr);
                }
                head.push(' ');
            }
            head.push_str(&quote(what));
            line(rows, node.line, format!("{pad}{head}"));
        }
        Kind::Menu {
            name,
            preface,
            items,
        } => {
            match name {
                Some(name) => line(rows, node.line, format!("{pad}menu {name}:")),
                None => line(rows, node.line, format!("{pad}menu:")),
            }
            if let Some(preface) = preface {
                line(rows, 0, format!("{pad}    {preface}"));
            }
            for item in items {
                emit_choice(item, indent + 4, rows);
            }
        }
        Kind::Jump { target } => line(rows, node.line, format!("{pad}jump {target}")),
        Kind::Call {
            label,
            arguments,
            from_label,
        } => {
            let args = arguments
                .as_deref()
                .filter(|s| !s.is_empty())
                .map(|s| format!("({s})"))
                .unwrap_or_default();
            let from = from_label
                .as_ref()
                .map(|name| format!(" from {name}"))
                .unwrap_or_default();
            line(rows, node.line, format!("{pad}call {label}{args}{from}"));
        }
        Kind::Return { expr } => match expr {
            Some(expr) => line(rows, node.line, format!("{pad}return {expr}")),
            None => line(rows, node.line, format!("{pad}return")),
        },
        Kind::If { entries } => {
            for (i, (cond, body)) in entries.iter().enumerate() {
                let last = i + 1 == entries.len();
                let head = if i == 0 {
                    format!("{pad}if {}:", cond.as_deref().unwrap_or("True"))
                } else if last && matches!(cond.as_deref(), None | Some("True")) {
                    format!("{pad}else:")
                } else {
                    format!("{pad}elif {}:", cond.as_deref().unwrap_or("True"))
                };
                let line_no = if i == 0 { node.line } else { 0 };
                line(rows, line_no, head);
                emit_nodes(body, indent + 4, rows);
            }
        }
        Kind::While { cond, body } => {
            line(rows, node.line, format!("{pad}while {cond}:"));
            emit_nodes(body, indent + 4, rows);
        }
        Kind::Pass => line(rows, node.line, format!("{pad}pass")),
        Kind::Python {
            code,
            early,
            one_line,
            init,
            hide,
            store,
        } => {
            if *one_line {
                let code = code.trim();
                line(rows, node.line, format!("{pad}$ {code}"));
            } else {
                let early = if *early { " early" } else { "" };
                let hide = if *hide { " hide" } else { "" };
                let store = store
                    .as_ref()
                    .map(|name| format!(" in {name}"))
                    .unwrap_or_default();
                let head = match init {
                    Some(priority) if *priority != 0 => {
                        format!("init {priority} python{early}{hide}{store}:")
                    }
                    Some(_) => format!("init python{early}{hide}{store}:"),
                    None => format!("python{early}{hide}{store}:"),
                };
                line(rows, node.line, format!("{pad}{head}"));
                for src in code.trim_matches('\n').lines() {
                    line(rows, 0, format!("{pad}    {src}"));
                }
            }
        }
        Kind::Define {
            keyword,
            priority,
            name,
            expr,
        } => {
            let pri = if *priority != 0 {
                format!("{priority} ")
            } else {
                String::new()
            };
            line(
                rows,
                node.line,
                format!("{pad}{keyword} {pri}{name} = {}", expr.trim()),
            )
        }
        Kind::Image { name, expr, atl } => match atl {
            Some(atl) => emit_block(rows, node.line, indent, &format!("image {name}:"), atl),
            None => line(
                rows,
                node.line,
                format!("{pad}image {name} = {}", expr.trim()),
            ),
        },
        Kind::Present { text } | Kind::User { line: text } => {
            emit_text(rows, node.line, indent, text)
        }
        Kind::With { expr, .. } => line(
            rows,
            node.line,
            format!("{pad}with {}", expr.as_deref().unwrap_or("None")),
        ),
        Kind::Init { priority, body }
            if !body.is_empty()
                && body
                    .iter()
                    .all(|n| matches!(n.kind, Kind::TranslateString { .. })) =>
        {
            emit_translate_strings(rows, node.line, indent, body);
        }
        Kind::Init { priority, body } => {
            line(rows, node.line, format!("{pad}init {priority}:"));
            emit_nodes(body, indent + 4, rows);
        }
        Kind::Screen { name, params, body } => {
            let params = if params.is_empty() {
                "()".to_string()
            } else {
                format!("({params})")
            };
            emit_block(
                rows,
                node.line,
                indent,
                &format!("screen {name}{params}:"),
                body,
            );
        }
        Kind::Translate { lang, ident, body } => {
            let id = ident.clone().unwrap_or_else(|| "id".into());
            line(rows, node.line, format!("{pad}translate {lang} {id}:"));
            emit_nodes(body, indent + 4, rows);
        }
        Kind::TranslateString { .. } => {
            emit_translate_strings(rows, node.line, indent, std::slice::from_ref(node))
        }
        Kind::Unknown { class } => {
            line(rows, node.line, format!("{pad}pass  # {class}"));
        }
    }
}

fn emit_text(rows: &mut Vec<(u32, String)>, first: u32, indent: usize, text: &str) {
    let pad = " ".repeat(indent);
    for (i, src) in text.lines().enumerate() {
        line(rows, if i == 0 { first } else { 0 }, format!("{pad}{src}"));
    }
}

fn emit_block(rows: &mut Vec<(u32, String)>, first: u32, indent: usize, head: &str, body: &str) {
    let pad = " ".repeat(indent);
    line(rows, first, format!("{pad}{head}"));
    let inner = " ".repeat(indent + 4);
    let mut any = false;
    for src in body.lines() {
        any = true;
        line(rows, 0, format!("{inner}{src}"));
    }
    if !any {
        line(rows, 0, format!("{inner}pass"));
    }
}

fn emit_choice(item: &MenuItem, indent: usize, rows: &mut Vec<(u32, String)>) {
    let pad = " ".repeat(indent);
    let caption = quote(&item.caption);
    if item.caption_only {
        line(rows, 0, format!("{pad}{caption}"));
        return;
    }
    match &item.cond {
        Some(cond) => line(rows, 0, format!("{pad}{caption} if {cond}:")),
        None => line(rows, 0, format!("{pad}{caption}:")),
    }
    if item.body.is_empty() {
        line(rows, 0, format!("{pad}    pass"));
    } else {
        emit_nodes(&item.body, indent + 4, rows);
    }
}

fn line(rows: &mut Vec<(u32, String)>, line: u32, text: String) {
    rows.push((line, text));
}

fn join_rows(rows: &[(u32, String)]) -> (String, Vec<u32>) {
    let mut out = String::new();
    let mut marks: Vec<Option<u32>> = Vec::new();
    let mut at = 1u32;
    for (want, text) in rows {
        let explicit = if *want == 0 { None } else { Some(*want) };
        let place = if *want == 0 { at } else { (*want).max(at) };
        while at < place {
            out.push('\n');
            marks.push(None);
            at += 1;
        }
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
            marks.push(None);
        }
        let parts: Vec<&str> = if text.is_empty() {
            vec![""]
        } else {
            text.split('\n').collect()
        };
        for (i, part) in parts.iter().enumerate() {
            if i > 0 {
                out.push('\n');
            }
            out.push_str(part);
            marks.push(if i == 0 { explicit } else { None });
        }
        out.push('\n');
        at = place + parts.len() as u32;
    }
    fill_marks(&mut marks);
    let lines = marks.into_iter().map(|m| m.unwrap_or(0)).collect();
    (out, lines)
}

fn fill_marks(marks: &mut [Option<u32>]) {
    let mut last = 0u32;
    for slot in marks.iter_mut() {
        if let Some(n) = *slot {
            if n > 0 {
                last = n;
            }
        } else if last > 0 {
            *slot = Some(last);
        }
    }
    let mut next = 0u32;
    for slot in marks.iter_mut().rev() {
        if let Some(n) = *slot {
            if n > 0 {
                next = n;
            }
        } else if next > 0 {
            *slot = Some(next);
        }
    }
}

fn emit_translate_strings(
    rows: &mut Vec<(u32, String)>,
    line_no: u32,
    indent: usize,
    nodes: &[Node],
) {
    let pad = " ".repeat(indent);
    let lang = nodes
        .iter()
        .find_map(|n| match &n.kind {
            Kind::TranslateString { lang, .. } => Some(lang.as_str()),
            _ => None,
        })
        .unwrap_or("None");
    line(rows, line_no, format!("{pad}translate {lang} strings:"));
    for node in nodes {
        let Kind::TranslateString { old, new, .. } = &node.kind else {
            continue;
        };
        line(rows, 0, format!("{pad}    old {}", quote(old)));
        line(rows, 0, format!("{pad}    new {}", quote(new)));
    }
}

pub(crate) fn quote(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::super::ast::{Kind, Node, Tree};
    use super::render_mapped;

    #[test]
    fn python_hide_and_store_are_written_back() {
        let tree = Tree {
            nodes: vec![Node {
                line: 1,
                kind: Kind::Python {
                    code: "x = 1\n".into(),
                    early: false,
                    one_line: false,
                    init: None,
                    hide: true,
                    store: Some("prefs".into()),
                },
            }],
            partial: false,
            reasons: Vec::new(),
        };
        let (text, _) = render_mapped(&tree);
        assert!(text.contains("python hide in prefs:"), "{text}");
        assert!(text.contains("x = 1"), "{text}");
    }

    #[test]
    fn output_lines_keep_the_compiled_line_number() {
        let tree = Tree {
            nodes: vec![
                Node {
                    line: 5,
                    kind: Kind::Label {
                        name: "start".into(),
                        params: String::new(),
                        hide: false,
                        body: vec![Node {
                            line: 6,
                            kind: Kind::Say {
                                who: None,
                                what: "hi".into(),
                                attrs: vec![],
                            },
                        }],
                    },
                },
                Node {
                    line: 20,
                    kind: Kind::Jump {
                        target: "end".into(),
                    },
                },
            ],
            partial: false,
            reasons: Vec::new(),
        };
        let (text, lines) = render_mapped(&tree);
        assert_eq!(lines.len(), text.lines().count(), "{text}");
        assert_eq!(lines[0], 5, "leading gap fills from the first statement");
        assert_eq!(lines[4], 5);
        assert_eq!(lines[5], 6);
        assert_eq!(lines[6], 6, "a blank after a statement keeps that line");
        assert_eq!(lines[19], 20);
    }
}
