//! Print screen language 2 (`renpy.sl2.slast`) back to source text.

use super::ast::{self, expr_text, field, int_field, text_field};
use super::unpickle::Value;

/// `(parameters inside the parens, screen body without the header indent)`.
pub fn render_screen(screen: &Value) -> Result<(String, String), String> {
    let params = match field(screen, "parameters") {
        None | Some(Value::None) => String::new(),
        Some(params) => signature(params).ok_or_else(|| "parameters".to_string())?,
    };
    let mut body = String::new();
    write_screen_props(screen, &mut body);
    if let Some(doc) = text_field(screen, "docstring") {
        push(&mut body, 0, &doc);
    }
    if let Some(Value::List(children)) = field(screen, "children") {
        for child in children {
            write_node(child, 0, &mut body)?;
        }
    }
    Ok((params, body))
}

/// Parameter list without the surrounding parentheses. Empty when there are none.
pub fn signature(v: &Value) -> Option<String> {
    if ast::class_of(v).is_some_and(|c| c.ends_with("ParameterInfo")) {
        return parameter_info(v);
    }
    let params = field(v, "parameters")?;
    let Value::Dict(items) = params else {
        return None;
    };
    if items.is_empty() {
        return Some(String::new());
    }
    let mut rendered = Vec::new();
    let mut last_posonly = None;
    let mut first_kwonly = None;
    for (i, (name, param)) in items.iter().enumerate() {
        let name = expr_text(name)?;
        let kind = int_field(param, "kind").unwrap_or(1);
        let default = match field(param, "default") {
            None | Some(Value::None) => None,
            Some(other) => Some(expr_text(other)?),
        };
        let text = match kind {
            0 | 1 => match default {
                Some(default) => format!("{name}={default}"),
                None => name,
            },
            2 => format!("*{name}"),
            3 => match default {
                Some(default) => format!("{name}={default}"),
                None => name,
            },
            4 => format!("**{name}"),
            _ => return None,
        };
        if kind == 0 {
            last_posonly = Some(i);
        }
        if kind == 3 && first_kwonly.is_none() {
            first_kwonly = Some(rendered.len());
        }
        rendered.push(text);
    }
    if let Some(i) = last_posonly {
        rendered.insert(i + 1, "/".into());
    }
    if let Some(i) = first_kwonly {
        let at = if last_posonly.is_some() { i + 1 } else { i };
        if !rendered
            .iter()
            .take(at)
            .any(|p| p.starts_with('*') && !p.starts_with("**"))
        {
            rendered.insert(at, "*".into());
        }
    }
    Some(rendered.join(", "))
}

/// Ren'Py 7 stores parameters as a list of `(name, default)` plus positional names.
fn parameter_info(v: &Value) -> Option<String> {
    let pairs = match field(v, "parameters")? {
        Value::List(items) | Value::Tuple(items) => items,
        _ => return None,
    };
    let positional: Vec<String> = match field(v, "positional") {
        Some(Value::List(items) | Value::Tuple(items)) => {
            items.iter().filter_map(expr_text).collect()
        }
        _ => Vec::new(),
    };
    let extrapos = text_field(v, "extrapos");
    let extrakw = text_field(v, "extrakw");
    let mut parts = Vec::new();
    let mut starred = false;
    for pair in pairs {
        let (Value::Tuple(parts_of) | Value::List(parts_of)) = pair else {
            return None;
        };
        let name = parts_of.first().and_then(expr_text)?;
        let default = parts_of.get(1).and_then(expr_text);
        if !starred && !positional.iter().any(|p| p == &name) {
            starred = true;
            parts.push(match &extrapos {
                Some(n) => format!("*{n}"),
                None => "*".into(),
            });
        }
        match default {
            Some(default) => parts.push(format!("{name}={default}")),
            None => parts.push(name),
        }
    }
    if let Some(name) = extrapos {
        if !starred {
            parts.push(format!("*{name}"));
        }
    }
    if let Some(name) = extrakw {
        parts.push(format!("**{name}"));
    }
    Some(parts.join(", "))
}

/// Argument list without the surrounding parentheses.
pub fn arguments(v: &Value) -> Option<String> {
    let Value::List(items) = field(v, "arguments")? else {
        return None;
    };
    let starred = index_set(field(v, "starred_indexes"));
    let doubled = index_set(field(v, "doublestarred_indexes"));
    let mut parts = Vec::new();
    for (i, item) in items.iter().enumerate() {
        let Value::Tuple(pair) = item else {
            return None;
        };
        let keyword = pair.first().and_then(expr_text).filter(|s| s != "None");
        let expr = pair.get(1).and_then(expr_text)?;
        if starred.contains(&i) {
            parts.push(format!("*{expr}"));
        } else if doubled.contains(&i) {
            parts.push(format!("**{expr}"));
        } else if let Some(keyword) = keyword {
            parts.push(format!("{keyword}={expr}"));
        } else {
            parts.push(expr);
        }
    }
    if let Some(extra) = text_field(v, "extrapos") {
        parts.push(format!("*{extra}"));
    }
    if let Some(extra) = text_field(v, "extrakw") {
        parts.push(format!("**{extra}"));
    }
    Some(parts.join(", "))
}

fn index_set(v: Option<&Value>) -> Vec<usize> {
    let Some(v) = v else { return Vec::new() };
    let items = match v {
        Value::Set(items) | Value::List(items) | Value::Tuple(items) => items,
        _ => return Vec::new(),
    };
    items
        .iter()
        .filter_map(|n| match n {
            Value::Int(i) => usize::try_from(*i).ok(),
            _ => None,
        })
        .collect()
}

fn write_screen_props(screen: &Value, out: &mut String) {
    prop(
        out,
        "modal",
        text_field(screen, "modal"),
        &["False", "None"],
    );
    prop(out, "zorder", text_field(screen, "zorder"), &["0", "None"]);
    if let Some(tag) = text_field(screen, "tag") {
        push(out, 0, &format!("tag {tag}"));
    }
    prop(out, "variant", text_field(screen, "variant"), &["None"]);
    prop(out, "predict", text_field(screen, "predict"), &["None"]);
    prop(
        out,
        "sensitive",
        text_field(screen, "sensitive"),
        &["True", "None"],
    );
    prop(
        out,
        "layer",
        text_field(screen, "layer"),
        &["'screens'", "\"screens\"", "None"],
    );
}

fn prop(out: &mut String, name: &str, value: Option<String>, skip: &[&str]) {
    let Some(value) = value else { return };
    if skip.contains(&value.as_str()) {
        return;
    }
    push(out, 0, &format!("{name} {value}"));
}

fn write_node(v: &Value, indent: usize, out: &mut String) -> Result<(), String> {
    let class = ast::class_of(v).unwrap_or("").to_string();
    let short = class.rsplit('.').next().unwrap_or("");
    match short {
        "SLDisplayable" => write_displayable(v, indent, out),
        "SLIf" => write_if(v, "if", indent, out),
        "SLShowIf" => write_if(v, "showif", indent, out),
        "SLFor" => {
            let var = text_field(v, "variable").ok_or_else(|| class.clone())?;
            let expr = text_field(v, "expression").ok_or_else(|| class.clone())?;
            let index = text_field(v, "index_expression")
                .map(|i| format!(" index {i}"))
                .unwrap_or_default();
            push(out, indent, &format!("for {var}{index} in {expr}:"));
            write_children(v, indent + 4, out)
        }
        "SLPython" => {
            let code = field(v, "code").and_then(expr_text).unwrap_or_default();
            if !code.contains('\n') {
                push(out, indent, &format!("$ {}", code.trim()));
            } else {
                push(out, indent, "python:");
                for src in code.trim_matches('\n').lines() {
                    push(out, indent + 4, src);
                }
            }
            Ok(())
        }
        "SLPass" => {
            push(out, indent, "pass");
            Ok(())
        }
        "SLDefault" => {
            let var = text_field(v, "variable").ok_or_else(|| class.clone())?;
            let expr = text_field(v, "expression").unwrap_or_else(|| "None".into());
            push(out, indent, &format!("default {var} = {expr}"));
            Ok(())
        }
        "SLUse" => write_use(v, indent, out),
        "SLCustomUse" => write_custom_use(v, indent, out),
        "SLTransclude" => {
            push(out, indent, "transclude");
            Ok(())
        }
        "SLBreak" => {
            push(out, indent, "break");
            Ok(())
        }
        "SLContinue" => {
            push(out, indent, "continue");
            Ok(())
        }
        "SLBlock" => write_children(v, indent, out),
        _ => Err(if class.is_empty() {
            "screen".into()
        } else {
            class
        }),
    }
}

fn write_displayable(v: &Value, indent: usize, out: &mut String) -> Result<(), String> {
    let class = ast::class_of(v).unwrap_or("SLDisplayable").to_string();
    let name = statement_name(v).ok_or(class)?;
    let mut head = name;
    if let Some(pos) = field(v, "positional") {
        let text = expr_list(pos);
        if !text.is_empty() {
            head.push(' ');
            head.push_str(&text);
        }
    }
    head.push_str(&keywords(field(v, "keyword")));
    if let Some(var) = text_field(v, "variable") {
        head.push_str(" as ");
        head.push_str(&var);
    }
    let children = match field(v, "children") {
        Some(Value::List(items)) if !items.is_empty() => Some(items),
        _ => None,
    };
    let atl = field(v, "atl_transform").filter(|a| !ast::is_none(a));
    if children.is_none() && atl.is_none() {
        push(out, indent, &head);
        return Ok(());
    }
    push(out, indent, &format!("{head}:"));
    if let Some(atl) = atl {
        push(out, indent + 4, "at transform:");
        let body = super::atl::render(atl)?;
        for src in body.lines() {
            push(out, indent + 8, src);
        }
    }
    if let Some(children) = children {
        for child in children {
            write_node(child, indent + 4, out)?;
        }
    }
    Ok(())
}

fn write_if(v: &Value, word: &str, indent: usize, out: &mut String) -> Result<(), String> {
    let class = ast::class_of(v).unwrap_or(word).to_string();
    let Some(Value::List(entries)) = field(v, "entries") else {
        return Err(class);
    };
    for (i, entry) in entries.iter().enumerate() {
        let Value::Tuple(parts) = entry else {
            return Err(class.clone());
        };
        let cond = parts.first().and_then(expr_text).filter(|s| s != "None");
        let block = parts.get(1).ok_or_else(|| class.clone())?;
        let head = match cond {
            None if i > 0 => "else:".into(),
            Some(cond) if i == 0 => format!("{word} {cond}:"),
            Some(cond) => format!("elif {cond}:"),
            None => format!("{word} True:"),
        };
        push(out, indent, &head);
        write_node(block, indent + 4, out)?;
    }
    Ok(())
}

fn write_use(v: &Value, indent: usize, out: &mut String) -> Result<(), String> {
    let class = ast::class_of(v).unwrap_or("SLUse").to_string();
    let target = text_field(v, "target").ok_or(class)?;
    let mut head = format!("use {target}");
    if let Some(args) = field(v, "args").filter(|a| !ast::is_none(a)) {
        if let Some(text) = arguments(args) {
            if !text.is_empty() {
                head.push('(');
                head.push_str(&text);
                head.push(')');
            }
        }
    }
    if let Some(id) = text_field(v, "id") {
        head.push_str(" id ");
        head.push_str(&id);
    }
    if let Some(var) = text_field(v, "variable") {
        head.push_str(" as ");
        head.push_str(&var);
    }
    match field(v, "block") {
        Some(block) if !ast::is_none(block) => {
            push(out, indent, &format!("{head}:"));
            write_node(block, indent + 4, out)
        }
        _ => {
            push(out, indent, &head);
            Ok(())
        }
    }
}

fn write_custom_use(v: &Value, indent: usize, out: &mut String) -> Result<(), String> {
    let class = ast::class_of(v).unwrap_or("SLCustomUse").to_string();
    let mut head = text_field(v, "target").ok_or(class)?;
    if let Some(pos) = field(v, "positional") {
        let text = expr_list(pos);
        if !text.is_empty() {
            head.push(' ');
            head.push_str(&text);
        }
    }
    let block = field(v, "block").filter(|b| !ast::is_none(b));
    if let Some(block) = block {
        head.push_str(&keywords(field(block, "keyword")));
    }
    if let Some(var) = text_field(v, "variable") {
        head.push_str(" as ");
        head.push_str(&var);
    }
    let children = block.and_then(|b| match field(b, "children") {
        Some(Value::List(items)) if !items.is_empty() => Some(items),
        _ => None,
    });
    if children.is_none() {
        push(out, indent, &head);
        return Ok(());
    }
    push(out, indent, &format!("{head}:"));
    if let Some(children) = children {
        for child in children {
            write_node(child, indent + 4, out)?;
        }
    }
    Ok(())
}

fn write_children(v: &Value, indent: usize, out: &mut String) -> Result<(), String> {
    if let Some(Value::List(children)) = field(v, "children") {
        for child in children {
            write_node(child, indent, out)?;
        }
    }
    Ok(())
}

fn statement_name(v: &Value) -> Option<String> {
    let displayable = text_field(v, "displayable").or_else(|| text_field(v, "name"))?;
    let name = displayable.rsplit('.').next().unwrap_or(&displayable);
    let name = name.strip_prefix('_').unwrap_or(name);
    if name.is_empty() {
        None
    } else {
        Some(name.to_ascii_lowercase())
    }
}

fn keywords(v: Option<&Value>) -> String {
    let Some(Value::List(items)) = v else {
        return String::new();
    };
    let mut out = String::new();
    for item in items {
        let Value::Tuple(parts) = item else { continue };
        let Some(name) = parts.first().and_then(expr_text) else {
            continue;
        };
        let Some(value) = parts.get(1).and_then(keyword_value) else {
            continue;
        };
        out.push(' ');
        out.push_str(&name);
        if !value.is_empty() {
            out.push(' ');
            out.push_str(&value);
        }
    }
    out
}

fn keyword_value(v: &Value) -> Option<String> {
    match v {
        Value::List(items) | Value::Tuple(items) => {
            let parts: Vec<String> = items.iter().filter_map(expr_text).collect();
            if parts.len() == items.len() {
                Some(parts.join(", "))
            } else {
                None
            }
        }
        other => expr_text(other),
    }
}

fn expr_list(v: &Value) -> String {
    match v {
        Value::List(items) | Value::Tuple(items) => items
            .iter()
            .filter_map(expr_text)
            .collect::<Vec<_>>()
            .join(" "),
        other => expr_text(other).unwrap_or_default(),
    }
}

fn push(out: &mut String, indent: usize, text: &str) {
    if !out.is_empty() {
        out.push('\n');
    }
    for _ in 0..indent {
        out.push(' ');
    }
    out.push_str(text);
}
