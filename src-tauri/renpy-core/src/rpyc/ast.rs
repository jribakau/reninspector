//! Turn unpickled `renpy.ast` objects into a small statement tree.

use super::unpickle::Value;

#[derive(Debug, Clone)]
pub struct Node {
    pub line: u32,
    pub kind: Kind,
}

#[derive(Debug, Clone)]
pub enum Kind {
    Label {
        name: String,
        params: String,
        hide: bool,
        body: Vec<Node>,
    },
    Say {
        who: Option<String>,
        what: String,
        attrs: Vec<String>,
    },
    Menu {
        name: Option<String>,
        preface: Option<String>,
        items: Vec<MenuItem>,
    },
    Jump {
        target: String,
    },
    Call {
        label: String,
        arguments: Option<String>,
        from_label: Option<String>,
    },
    Return {
        expr: Option<String>,
    },
    If {
        entries: Vec<(Option<String>, Vec<Node>)>,
    },
    While {
        cond: String,
        body: Vec<Node>,
    },
    Pass,
    Python {
        code: String,
        early: bool,
        one_line: bool,
        init: Option<i64>,
        /// `python hide:` — the block does not see the store.
        hide: bool,
        /// `python in name:` when the store is not the default.
        store: Option<String>,
    },
    Define {
        keyword: &'static str,
        priority: i64,
        name: String,
        expr: String,
    },
    Image {
        name: String,
        expr: String,
        atl: Option<String>,
    },
    /// `show` / `scene` / `hide` / `with`, already rendered as one statement.
    Present {
        text: String,
    },
    /// A `with` statement. `paired` is set on the placeholder Ren'Py inserts
    /// in front of `scene`/`show`/`hide ... with`.
    With {
        expr: Option<String>,
        paired: bool,
    },
    /// Exact source line from a `UserStatement` (`play`, `pause`, ...).
    User {
        line: String,
    },
    Init {
        priority: i64,
        body: Vec<Node>,
    },
    Screen {
        name: String,
        params: String,
        body: String,
    },
    Translate {
        lang: String,
        ident: Option<String>,
        body: Vec<Node>,
    },
    TranslateString {
        lang: String,
        old: String,
        new: String,
    },
    Unknown {
        class: String,
    },
}

#[derive(Debug, Clone)]
pub struct MenuItem {
    pub caption: String,
    /// A menu caption has no block. A choice always has one, even when empty.
    pub caption_only: bool,
    pub cond: Option<String>,
    pub body: Vec<Node>,
}

#[derive(Debug)]
pub struct Tree {
    pub nodes: Vec<Node>,
    pub partial: bool,
    pub reasons: Vec<String>,
}

pub fn tree_from(nodes: &[Value]) -> Tree {
    let mut partial = false;
    let mut reasons = Vec::new();
    let mut nodes = fold_same_line(fold_with(omit_synthetic(
        nodes
            .iter()
            .map(|n| convert(n, &mut partial, &mut reasons))
            .collect(),
    )));
    // The parser appends an expressionless return after the last statement.
    // It is not in the source.
    if matches!(
        nodes.last().map(|n| &n.kind),
        Some(Kind::Return { expr: None })
    ) {
        nodes.pop();
    }
    Tree {
        nodes,
        partial,
        reasons,
    }
}

/// Stable description of the recovered statements, ignoring line numbers.
/// An engine round trip compares this before and after compiling the decompiled text.
pub fn sequence(nodes: &[Node]) -> Vec<String> {
    let mut out = Vec::new();
    walk_seq(nodes, &mut out);
    out
}

fn walk_seq(nodes: &[Node], out: &mut Vec<String>) {
    for n in nodes {
        match &n.kind {
            Kind::Label { name, body, .. } => {
                out.push(format!("label {name}"));
                walk_seq(body, out);
            }
            Kind::Say { who, what, .. } => {
                out.push(format!("say {} {what}", who.as_deref().unwrap_or("")))
            }
            Kind::Menu { name, items, .. } => {
                out.push(format!("menu {}", name.as_deref().unwrap_or("")));
                for item in items {
                    if item.caption_only {
                        out.push(format!("caption {}", item.caption));
                    } else {
                        out.push(format!(
                            "choice {} {}",
                            item.caption,
                            item.cond.as_deref().unwrap_or("")
                        ));
                        walk_seq(&item.body, out);
                    }
                }
            }
            Kind::Jump { target } => out.push(format!("jump {target}")),
            Kind::Call { label, .. } => out.push(format!("call {label}")),
            Kind::Return { expr } => out.push(format!("return {}", expr.as_deref().unwrap_or(""))),
            Kind::If { entries } => {
                out.push("if".into());
                for (_, body) in entries {
                    walk_seq(body, out);
                }
            }
            Kind::While { body, .. } => {
                out.push("while".into());
                walk_seq(body, out);
            }
            Kind::Pass => out.push("pass".into()),
            Kind::Python {
                early,
                one_line,
                init,
                ..
            } => {
                out.push(format!("python early={early} one={one_line} init={init:?}"));
            }
            Kind::Define {
                keyword,
                name,
                expr,
                ..
            } => out.push(format!("{keyword} {name} = {expr}")),
            Kind::Image { name, expr, atl } => {
                out.push(format!("image {name} = {expr}"));
                if let Some(atl) = atl {
                    out.push(normalize_block(atl));
                }
            }
            Kind::Present { text } => out.push(normalize_block(text)),
            Kind::With { expr, .. } => out.push(format!("with {}", expr.as_deref().unwrap_or(""))),
            Kind::User { line } => out.push(format!("user {line}")),
            Kind::Init { priority, body } => {
                out.push(format!("init {priority}"));
                walk_seq(body, out);
            }
            Kind::Screen { name, body, .. } => {
                out.push(format!("screen {name}"));
                out.push(normalize_block(body));
            }
            Kind::Translate { lang, ident, body } => {
                out.push(format!(
                    "translate {lang} {}",
                    ident.as_deref().unwrap_or("")
                ));
                walk_seq(body, out);
            }
            Kind::TranslateString { lang, old, new } => {
                out.push(format!("tstring {lang} {old} -> {new}"))
            }
            Kind::Unknown { class } => out.push(format!("unknown {class}")),
        }
    }
}

fn normalize_block(text: &str) -> String {
    text.lines()
        .map(|l| l.trim_end())
        .collect::<Vec<_>>()
        .join("\n")
}

fn convert(v: &Value, partial: &mut bool, reasons: &mut Vec<String>) -> Node {
    let class = class_of(v).unwrap_or("").to_string();
    let line = int_field(v, "linenumber").unwrap_or(1).max(0) as u32;
    let short = class.rsplit('.').next().unwrap_or("");
    let kind = match short {
        "Label" => Kind::Label {
            name: text_field(v, "_name")
                .or_else(|| text_field(v, "name"))
                .unwrap_or_else(|| "unknown".into()),
            params: params_of(v, partial, reasons),
            hide: field(v, "hide").is_some_and(|h| matches!(h, Value::Bool(true))),
            body: block(v, "block", partial, reasons),
        },
        "Say" | "TranslateSay" => Kind::Say {
            who: text_field(v, "who").filter(|s| s != "None"),
            what: text_field(v, "what").unwrap_or_default(),
            attrs: string_list(v, "attributes"),
        },
        "Menu" => Kind::Menu {
            name: menu_name(v),
            preface: None,
            items: menu_items(v, partial, reasons),
        },
        "Jump" => Kind::Jump {
            target: text_field(v, "target").unwrap_or_default(),
        },
        "Call" => Kind::Call {
            label: call_label(v),
            arguments: call_arguments(v, partial, reasons),
            from_label: None,
        },
        "Return" => Kind::Return {
            expr: text_field(v, "expression").or_else(|| text_field(v, "expr")),
        },
        "If" => Kind::If {
            entries: if_entries(v, partial, reasons),
        },
        "While" => Kind::While {
            cond: text_field(v, "condition")
                .or_else(|| text_field(v, "cond"))
                .unwrap_or_else(|| "True".into()),
            body: block(v, "block", partial, reasons),
        },
        "Pass" => Kind::Pass,
        "Python" | "EarlyPython" => python_kind(short, v),
        "Define" | "Default" => define_kind(short, v),
        "Image" => image_kind(v, partial, reasons),
        "Show" | "Scene" | "Hide" => Kind::Present {
            text: present(short, v, partial, reasons),
        },
        "ShowLayer" | "Camera" => Kind::Present {
            text: layer_stmt(short, v, partial, reasons),
        },
        "With" => Kind::With {
            expr: text_field(v, "expr"),
            paired: field(v, "paired").is_some_and(|p| !is_none(p)),
        },
        "UserStatement" | "PostUserStatement" => Kind::User {
            line: text_field(v, "line").unwrap_or_else(|| "pass".into()),
        },
        "Init" => init_kind(v, line, partial, reasons),
        "Screen" => screen_kind(v, partial, reasons),
        "Translate" | "TranslateBlock" | "EndTranslate" => Kind::Translate {
            lang: text_field(v, "language").unwrap_or_else(|| "None".into()),
            ident: text_field(v, "identifier"),
            body: block(v, "block", partial, reasons),
        },
        "TranslateString" => Kind::TranslateString {
            lang: text_field(v, "language").unwrap_or_else(|| "None".into()),
            old: text_field(v, "old").unwrap_or_default(),
            new: text_field(v, "new").unwrap_or_default(),
        },
        "Style" => Kind::Present {
            text: style_stmt(v),
        },
        "Transform" => Kind::Present {
            text: transform_stmt(v, partial, reasons),
        },
        "RPY" => Kind::Present { text: rpy_stmt(v) },
        "TranslatePython" => python_kind("Python", v),
        "TranslateEarlyBlock" => {
            let code = code_source(field(v, "code")).unwrap_or_default();
            if code.is_empty() {
                note(partial, reasons, short);
                Kind::Unknown { class }
            } else {
                python_kind("EarlyPython", v)
            }
        }
        _ if class.contains("slast.") || class.contains("atl.") => {
            note(partial, reasons, &class);
            Kind::Unknown { class }
        }
        _ => {
            if !class.is_empty() {
                note(partial, reasons, &class);
            }
            Kind::Unknown {
                class: if class.is_empty() {
                    "value".into()
                } else {
                    class
                },
            }
        }
    };
    Node { line, kind }
}

fn note(partial: &mut bool, reasons: &mut Vec<String>, why: &str) {
    *partial = true;
    if reasons.len() < 12 && !reasons.iter().any(|r| r == why) {
        reasons.push(why.to_string());
    }
}

fn block(v: &Value, key: &str, partial: &mut bool, reasons: &mut Vec<String>) -> Vec<Node> {
    match field(v, key) {
        Some(Value::List(items)) => child_nodes(items, partial, reasons),
        _ => Vec::new(),
    }
}

fn child_nodes(items: &[Value], partial: &mut bool, reasons: &mut Vec<String>) -> Vec<Node> {
    fold_same_line(fold_with(omit_synthetic(
        items
            .iter()
            .filter(|n| !matches!(n, Value::None))
            .map(|n| convert(n, partial, reasons))
            .collect(),
    )))
}

fn omit_synthetic(nodes: Vec<Node>) -> Vec<Node> {
    nodes
        .into_iter()
        .filter(|n| !synthetic_call_label(n))
        .collect()
}

/// A `call` is stored as the call, an optional `from` label, and a pass, all
/// on one line. `menu name:` is an empty label, an optional say, and the menu.
fn fold_same_line(mut nodes: Vec<Node>) -> Vec<Node> {
    let mut i = 0;
    while i < nodes.len() {
        if let Some(preface) = menu_say(&nodes, i) {
            let menu_at = if matches!(nodes[i].kind, Kind::Label { .. }) {
                i + 2
            } else {
                i + 1
            };
            if let Kind::Menu { preface: slot, .. } = &mut nodes[menu_at].kind {
                *slot = Some(preface);
            }
            let remove_label = menu_at == i + 2;
            nodes.remove(menu_at - 1);
            if remove_label {
                nodes.remove(i);
            }
            continue;
        }
        if matches!(nodes[i].kind, Kind::Call { .. }) {
            let mut from = None;
            let mut end = i + 1;
            if end < nodes.len() && nodes[end].line == nodes[i].line {
                if let Kind::Label { name, body, .. } = &nodes[end].kind {
                    if body.iter().all(|child| matches!(child.kind, Kind::Pass)) {
                        from = Some(name.clone());
                        end += 1;
                    }
                }
            }
            if end < nodes.len()
                && nodes[end].line == nodes[i].line
                && matches!(nodes[end].kind, Kind::Pass)
            {
                end += 1;
            }
            if let Kind::Call { from_label, .. } = &mut nodes[i].kind {
                *from_label = from;
            }
            for _ in (i + 1)..end {
                nodes.remove(i + 1);
            }
        }
        let drop_label = matches!(&nodes[i].kind, Kind::Label { body, .. } if body.is_empty())
            && nodes.get(i + 1).is_some_and(|next| {
                next.line == nodes[i].line && matches!(next.kind, Kind::Menu { .. })
            });
        if drop_label {
            let name = match &nodes[i].kind {
                Kind::Label { name, .. } => Some(name.clone()),
                _ => None,
            };
            if let Kind::Menu { name: slot, .. } = &mut nodes[i + 1].kind {
                if slot.is_none() {
                    *slot = name;
                }
            }
            nodes.remove(i);
            continue;
        }
        i += 1;
    }
    nodes
}

fn menu_say(nodes: &[Node], i: usize) -> Option<String> {
    let (say_at, menu_at) = if matches!(&nodes.get(i)?.kind, Kind::Label { body, .. } if body.is_empty())
    {
        (i + 1, i + 2)
    } else if matches!(&nodes.get(i)?.kind, Kind::Say { .. }) {
        (i, i + 1)
    } else {
        return None;
    };
    let say = nodes.get(say_at)?;
    let menu = nodes.get(menu_at)?;
    if !matches!(say.kind, Kind::Say { .. }) || !matches!(menu.kind, Kind::Menu { .. }) {
        return None;
    }
    if matches!(nodes[i].kind, Kind::Label { .. }) && nodes[i].line != menu.line {
        return None;
    }
    if !matches!(nodes[i].kind, Kind::Label { .. }) && say.line != menu.line {
        return None;
    }
    Some(say_text(say))
}

fn say_text(node: &Node) -> String {
    let Kind::Say { who, what, attrs } = &node.kind else {
        return String::new();
    };
    let mut head = String::new();
    if let Some(who) = who {
        head.push_str(who);
        for attr in attrs {
            head.push_str(" @ ");
            head.push_str(attr);
        }
        head.push(' ');
    }
    head.push_str(&super::decompile::quote(what));
    head
}

fn synthetic_call_label(n: &Node) -> bool {
    match &n.kind {
        Kind::Label { name, body, .. } if name.starts_with("_call_") => {
            body.iter().all(|c| matches!(c.kind, Kind::Pass))
        }
        _ => false,
    }
}

fn python_kind(short: &str, v: &Value) -> Kind {
    let code = code_source(field(v, "code")).unwrap_or_default();
    Kind::Python {
        one_line: !code.contains('\n'),
        early: short == "EarlyPython",
        init: None,
        code,
        hide: field(v, "hide").is_some_and(|h| matches!(h, Value::Bool(true))),
        store: python_store(v),
    }
}

fn python_store(v: &Value) -> Option<String> {
    let store = text_field(v, "store")?;
    if store.is_empty() || store == "store" {
        return None;
    }
    let rest = store.strip_prefix("store.").unwrap_or(&store);
    if rest.is_empty() {
        None
    } else {
        Some(rest.to_string())
    }
}

fn define_kind(short: &str, v: &Value) -> Kind {
    Kind::Define {
        keyword: if short == "Default" {
            "default"
        } else {
            "define"
        },
        priority: 0,
        name: define_name(v),
        expr: code_source(field(v, "code")).unwrap_or_else(|| "None".into()),
    }
}

fn init_kind(v: &Value, line: u32, partial: &mut bool, reasons: &mut Vec<String>) -> Kind {
    let priority = int_field(v, "priority").unwrap_or(0);
    let body = block(v, "block", partial, reasons);
    if let Some(kind) = unwrap_same_line_init(line, priority, &body) {
        return kind;
    }
    Kind::Init { priority, body }
}

/// `define`, `image`, `screen`, `style`, `transform` and `init python` are
/// stored as an Init node on the same line as the statement. Emitting both
/// would push every later line down.
fn unwrap_same_line_init(line: u32, priority: i64, body: &[Node]) -> Option<Kind> {
    let [only] = body else { return None };
    if only.line != line {
        return None;
    }
    match &only.kind {
        Kind::Define {
            keyword,
            name,
            expr,
            ..
        } => Some(Kind::Define {
            keyword,
            priority,
            name: name.clone(),
            expr: expr.clone(),
        }),
        Kind::Python {
            code,
            early,
            one_line,
            hide,
            store,
            ..
        } if !one_line => Some(Kind::Python {
            code: code.clone(),
            early: *early,
            one_line: false,
            init: Some(priority),
            hide: *hide,
            store: store.clone(),
        }),
        Kind::Image { .. } if priority == 500 => Some(only.kind.clone()),
        Kind::Screen { .. } if priority == -500 => Some(only.kind.clone()),
        Kind::Present { text }
            if priority == 0 && (text.starts_with("style ") || text.starts_with("transform ")) =>
        {
            Some(only.kind.clone())
        }
        Kind::Present { text } if text.starts_with("transform ") => {
            let mut text = text.clone();
            if priority != 0 {
                text.insert_str("transform ".len(), &format!("{priority} "));
            }
            Some(Kind::Present { text })
        }
        _ => None,
    }
}

/// `scene corridor with fade` is stored as With(None, paired), Scene, With(fade),
/// all on the same line. Put it back on one line so later statements keep
/// their original line numbers.
fn fold_with(mut nodes: Vec<Node>) -> Vec<Node> {
    let mut i = 0;
    while i + 2 < nodes.len() {
        let triple = matches!(
            nodes[i].kind,
            Kind::With {
                expr: None,
                paired: true
            }
        ) && matches!(nodes[i + 1].kind, Kind::Present { .. })
            && matches!(nodes[i + 2].kind, Kind::With { expr: Some(_), .. })
            && nodes[i].line == nodes[i + 1].line
            && nodes[i].line == nodes[i + 2].line;
        if triple {
            let expr = match &nodes[i + 2].kind {
                Kind::With {
                    expr: Some(expr), ..
                } => expr.clone(),
                _ => String::new(),
            };
            if let Kind::Present { text } = &mut nodes[i + 1].kind {
                text.push_str(" with ");
                text.push_str(&expr);
            }
            nodes.remove(i + 2);
            nodes.remove(i);
        }
        i += 1;
    }
    nodes
}

fn menu_items(v: &Value, partial: &mut bool, reasons: &mut Vec<String>) -> Vec<MenuItem> {
    let Some(Value::List(items)) = field(v, "items") else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let Value::Tuple(parts) = item else {
                return None;
            };
            let caption = parts.first().and_then(expr_text).unwrap_or_default();
            let cond = parts
                .get(1)
                .and_then(expr_text)
                .filter(|s| s != "True" && s != "None");
            let (caption_only, body) = match parts.get(2) {
                Some(Value::List(nodes)) => (false, child_nodes(nodes, partial, reasons)),
                Some(Value::None) | None => (true, Vec::new()),
                Some(_) => (false, Vec::new()),
            };
            Some(MenuItem {
                caption,
                caption_only,
                cond,
                body,
            })
        })
        .collect()
}

fn if_entries(
    v: &Value,
    partial: &mut bool,
    reasons: &mut Vec<String>,
) -> Vec<(Option<String>, Vec<Node>)> {
    let Some(Value::List(items)) = field(v, "entries") else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let Value::Tuple(parts) = item else {
                return None;
            };
            let cond = parts.first().and_then(expr_text);
            let body = match parts.get(1) {
                Some(Value::List(nodes)) => child_nodes(nodes, partial, reasons),
                _ => Vec::new(),
            };
            let cond = match cond.as_deref() {
                None | Some("None") => None,
                Some(other) => Some(other.to_string()),
            };
            Some((cond, body))
        })
        .collect()
}

fn present(cmd: &str, v: &Value, partial: &mut bool, reasons: &mut Vec<String>) -> String {
    let spec = field(v, "imspec")
        .map(|s| imspec(cmd, s))
        .unwrap_or_else(|| cmd.to_lowercase());
    with_atl(spec, field(v, "atl"), partial, reasons)
}

fn imspec(cmd: &str, v: &Value) -> String {
    let Value::Tuple(items) = v else {
        return cmd.to_lowercase();
    };
    let (name, expression, tag, at_list, layer) = if items.len() >= 7 {
        (
            &items[0],
            Some(&items[1]),
            Some(&items[2]),
            Some(&items[3]),
            Some(&items[4]),
        )
    } else if items.len() >= 3 {
        (&items[0], None, None, Some(&items[1]), Some(&items[2]))
    } else {
        (
            items.first().unwrap_or(&Value::None),
            None,
            None,
            None,
            None,
        )
    };
    let mut out = String::new();
    let verb = if cmd == "Hide" {
        "hide"
    } else if cmd == "Scene" {
        "scene"
    } else {
        "show"
    };
    out.push_str(verb);
    if let Some(expr) = expression
        .and_then(expr_text)
        .filter(|s| !s.is_empty() && s != "None")
    {
        out.push_str(" expression ");
        out.push_str(&expr);
    } else {
        let n = tuple_words(name);
        if !n.is_empty() {
            out.push(' ');
            out.push_str(&n);
        }
    }
    if let Some(tag) = tag.and_then(expr_text).filter(|s| s != "None") {
        out.push_str(" as ");
        out.push_str(&tag);
    }
    if let Some(Value::List(ats)) = at_list {
        let ats: Vec<String> = ats
            .iter()
            .filter_map(expr_text)
            .filter(|s| s != "None")
            .collect();
        if !ats.is_empty() {
            out.push_str(" at ");
            out.push_str(&ats.join(", "));
        }
    }
    if let Some(layer) = layer
        .and_then(expr_text)
        .filter(|s| s != "None" && s != "master")
    {
        out.push_str(" onlayer ");
        out.push_str(&layer);
    }
    out
}

fn layer_stmt(cmd: &str, v: &Value, partial: &mut bool, reasons: &mut Vec<String>) -> String {
    let layer = text_field(v, "layer").unwrap_or_else(|| "master".into());
    let mut head = if cmd == "Camera" {
        format!("camera {layer}")
    } else {
        format!("show layer {layer}")
    };
    if let Some(Value::List(ats)) = field(v, "at_list") {
        let ats: Vec<String> = ats
            .iter()
            .filter_map(expr_text)
            .filter(|s| s != "None")
            .collect();
        if !ats.is_empty() {
            head.push_str(" at ");
            head.push_str(&ats.join(", "));
        }
    }
    with_atl(head, field(v, "atl"), partial, reasons)
}

fn with_atl(
    mut head: String,
    atl: Option<&Value>,
    partial: &mut bool,
    reasons: &mut Vec<String>,
) -> String {
    let Some(atl) = atl.filter(|a| !is_none(a)) else {
        return head;
    };
    match super::atl::render(atl) {
        Ok(body) if body.trim().is_empty() => head,
        Ok(body) => {
            head.push(':');
            for src in body.lines() {
                head.push('\n');
                head.push_str("    ");
                head.push_str(src);
            }
            head
        }
        Err(why) => {
            note(partial, reasons, &why);
            head
        }
    }
}

fn image_kind(v: &Value, partial: &mut bool, reasons: &mut Vec<String>) -> Kind {
    let name = image_name(v);
    if let Some(atl) = field(v, "atl").filter(|a| !is_none(a)) {
        match super::atl::render(atl) {
            Ok(body) => Kind::Image {
                name,
                expr: String::new(),
                atl: Some(body),
            },
            Err(why) => {
                note(partial, reasons, &why);
                Kind::Image {
                    name,
                    expr: "Null()".into(),
                    atl: None,
                }
            }
        }
    } else {
        Kind::Image {
            name,
            expr: code_source(field(v, "code")).unwrap_or_else(|| "Null()".into()),
            atl: None,
        }
    }
}

fn transform_stmt(v: &Value, partial: &mut bool, reasons: &mut Vec<String>) -> String {
    let name = text_field(v, "varname")
        .or_else(|| text_field(v, "name"))
        .unwrap_or_else(|| "transform".into());
    let params = params_of(v, partial, reasons);
    let params = if params.is_empty() {
        String::new()
    } else {
        format!("({params})")
    };
    let head = format!("transform {name}{params}");
    match field(v, "atl") {
        Some(atl) if !is_none(atl) => match super::atl::render(atl) {
            Ok(body) => {
                let mut text = format!("{head}:");
                for src in body.lines() {
                    text.push('\n');
                    text.push_str("    ");
                    text.push_str(src);
                }
                text
            }
            Err(why) => {
                note(partial, reasons, &why);
                head
            }
        },
        _ => {
            note(partial, reasons, "transform");
            head
        }
    }
}

fn screen_kind(v: &Value, partial: &mut bool, reasons: &mut Vec<String>) -> Kind {
    let screen = field(v, "screen").unwrap_or(v);
    let name = text_field(screen, "name").unwrap_or_else(|| "screen".into());
    match super::sl::render_screen(screen) {
        Ok((params, body)) => Kind::Screen { name, params, body },
        Err(why) => {
            note(partial, reasons, &why);
            Kind::Screen {
                name,
                params: String::new(),
                body: String::new(),
            }
        }
    }
}

fn style_stmt(v: &Value) -> String {
    let name = text_field(v, "style_name").unwrap_or_else(|| "style".into());
    let mut head = format!("style {name}");
    if let Some(parent) = text_field(v, "parent") {
        head.push_str(" is ");
        head.push_str(&parent);
    }
    let mut lines = vec![format!("{head}:")];
    if field(v, "clear").is_some_and(|c| matches!(c, Value::Bool(true))) {
        lines.push("    clear".into());
    }
    if let Some(take) = text_field(v, "take") {
        lines.push(format!("    take {take}"));
    }
    if let Some(Value::List(attrs)) = field(v, "delattr") {
        for attr in attrs.iter().filter_map(expr_text) {
            lines.push(format!("    delattr {attr}"));
        }
    }
    if let Some(variant) = text_field(v, "variant") {
        lines.push(format!("    variant {variant}"));
    }
    if let Some(Value::Dict(props)) = field(v, "properties") {
        for (key, value) in props {
            let Some(key) = expr_text(key) else { continue };
            let Some(value) = expr_text(value) else {
                continue;
            };
            lines.push(format!("    {key} {value}"));
        }
    }
    if lines.len() == 1 {
        lines.push("    pass".into());
    }
    lines.join("\n")
}

fn rpy_stmt(v: &Value) -> String {
    let rest = match field(v, "rest") {
        Some(Value::Tuple(items) | Value::List(items)) => items
            .iter()
            .filter_map(expr_text)
            .collect::<Vec<_>>()
            .join(" "),
        Some(other) => expr_text(other).unwrap_or_default(),
        None => String::new(),
    };
    if rest.is_empty() {
        "rpy".into()
    } else {
        format!("rpy {rest}")
    }
}

fn params_of(v: &Value, partial: &mut bool, reasons: &mut Vec<String>) -> String {
    match field(v, "parameters") {
        None | Some(Value::None) => String::new(),
        Some(params) => match super::sl::signature(params) {
            Some(text) => text,
            None => {
                note(partial, reasons, "parameters");
                String::new()
            }
        },
    }
}

fn call_label(v: &Value) -> String {
    let label = text_field(v, "label").unwrap_or_default();
    if field(v, "expression").is_some_and(|e| matches!(e, Value::Bool(true))) {
        format!("expression {label}")
    } else {
        label
    }
}

fn call_arguments(v: &Value, partial: &mut bool, reasons: &mut Vec<String>) -> Option<String> {
    match field(v, "arguments") {
        None | Some(Value::None) => None,
        Some(args) => match super::sl::arguments(args) {
            Some(text) => Some(text),
            None => {
                note(partial, reasons, "arguments");
                None
            }
        },
    }
}

fn menu_name(v: &Value) -> Option<String> {
    match field(v, "statement_start") {
        Some(start) if class_of(start).is_some_and(|c| c.ends_with("Label")) => {
            text_field(start, "_name").or_else(|| text_field(start, "name"))
        }
        _ => None,
    }
}

fn define_name(v: &Value) -> String {
    let var = text_field(v, "varname").unwrap_or_else(|| "name".into());
    match text_field(v, "store").as_deref() {
        Some("store") | None => var,
        Some(store) => {
            let rest = store.strip_prefix("store.").unwrap_or(store);
            if rest.is_empty() {
                var
            } else {
                format!("{rest}.{var}")
            }
        }
    }
}

fn image_name(v: &Value) -> String {
    match field(v, "imgname") {
        Some(v) => tuple_words(v),
        None => text_field(v, "name").unwrap_or_else(|| "image".into()),
    }
}

fn tuple_words(v: &Value) -> String {
    match v {
        Value::Tuple(items) | Value::List(items) => items
            .iter()
            .filter_map(expr_text)
            .collect::<Vec<_>>()
            .join(" "),
        other => expr_text(other).unwrap_or_default(),
    }
}

fn string_list(v: &Value, key: &str) -> Vec<String> {
    match field(v, key) {
        Some(Value::List(items)) | Some(Value::Tuple(items)) => {
            items.iter().filter_map(expr_text).collect()
        }
        _ => Vec::new(),
    }
}

fn code_source(v: Option<&Value>) -> Option<String> {
    let v = v?;
    if let Some(text) = expr_text(v) {
        if !class_of(v).is_some_and(|c| c.ends_with("PyCode") || c.ends_with("PyExpr")) {
            return Some(text);
        }
    }
    match v {
        Value::Object { args, state, .. } => {
            if let Some(Value::Tuple(items)) = state.as_deref() {
                if let Some(src) = items.get(1).and_then(expr_text) {
                    return Some(src);
                }
            }
            args.first().and_then(expr_text)
        }
        other => expr_text(other),
    }
}

pub(crate) fn class_of(v: &Value) -> Option<&str> {
    match v {
        Value::Object { class, .. } => Some(class.as_str()),
        _ => None,
    }
}

pub(crate) fn field<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    let dict = match v {
        Value::Object {
            state: Some(state), ..
        } => match state.as_ref() {
            Value::Tuple(items) => match items.get(1) {
                Some(Value::Dict(_)) => items.get(1),
                _ => Some(state.as_ref()),
            },
            _ => Some(state.as_ref()),
        },
        _ => None,
    }?;
    let Value::Dict(items) = dict else {
        return None;
    };
    items.iter().find(|(k, _)| key_eq(k, key)).map(|(_, v)| v)
}

pub(crate) fn text_field(v: &Value, key: &str) -> Option<String> {
    field(v, key).and_then(expr_text).filter(|s| s != "None")
}

pub(crate) fn int_field(v: &Value, key: &str) -> Option<i64> {
    match field(v, key)? {
        Value::Int(n) => i64::try_from(*n).ok(),
        Value::Bool(true) => Some(1),
        _ => None,
    }
}

pub(crate) fn expr_text(v: &Value) -> Option<String> {
    match v {
        Value::Str(s) => Some(s.clone()),
        Value::Int(n) => Some(n.to_string()),
        Value::Float(n) => Some(n.to_string()),
        Value::Bool(b) => Some(if *b { "True" } else { "False" }.into()),
        Value::None => None,
        Value::Bytes(b) => Some(String::from_utf8_lossy(b).into_owned()),
        Value::Object { class, args, state }
            if class.ends_with("PyExpr") || class.ends_with("PyCode") =>
        {
            if let Some(Value::Tuple(items)) = state.as_deref() {
                if let Some(src) = items.get(1).and_then(expr_text) {
                    return Some(src);
                }
                if let Some(src) = items.first().and_then(expr_text) {
                    return Some(src);
                }
            }
            if let Some(dict_text) = state.as_deref().and_then(|s| match s {
                Value::Dict(items) => items
                    .iter()
                    .find(|(k, _)| key_eq(k, "s") || key_eq(k, "source"))
                    .and_then(|(_, v)| expr_text(v)),
                _ => None,
            }) {
                return Some(dict_text);
            }
            args.first().and_then(expr_text)
        }
        _ => None,
    }
}

pub(crate) fn is_none(v: &Value) -> bool {
    matches!(v, Value::None)
}

/// Dict keys are text in protocol 5 and 8-bit strings in protocol 2.
pub(crate) fn key_eq(k: &Value, key: &str) -> bool {
    match k {
        Value::Str(s) => s == key,
        Value::Bytes(b) => b == key.as_bytes(),
        _ => false,
    }
}
