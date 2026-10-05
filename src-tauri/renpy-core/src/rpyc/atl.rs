//! Print Ren'Py ATL (`renpy.atl.Raw*`) back to source text.
//!
//! A statement we do not recognise is an error, so the file stays read-only
//! instead of silently dropping that block.

use super::ast::{self, expr_text, field, text_field};
use crate::pickle::Value;

pub fn render(v: &Value) -> Result<String, String> {
    let mut out = String::new();
    write_stmt(v, 0, &mut out)?;
    Ok(out)
}

fn write_stmt(v: &Value, indent: usize, out: &mut String) -> Result<(), String> {
    let class = ast::class_of(v).unwrap_or("").to_string();
    let short = class.rsplit('.').next().unwrap_or("");
    match short {
        "RawBlock" => write_block(v, indent, out),
        "RawMultipurpose" => {
            push(out, indent, &multipurpose(v)?);
            Ok(())
        }
        "RawRepeat" => {
            let line = match text_field(v, "repeats") {
                Some(n) => format!("repeat {n}"),
                None => "repeat".into(),
            };
            push(out, indent, &line);
            Ok(())
        }
        "RawParallel" => write_grouped(v, "blocks", "parallel:", indent, out),
        "RawChoice" => {
            let Some(Value::List(choices)) = field(v, "choices") else {
                return Err(class);
            };
            for choice in choices {
                let Value::Tuple(parts) = choice else {
                    return Err(class);
                };
                let chance = parts
                    .first()
                    .and_then(expr_text)
                    .unwrap_or_else(|| "1.0".into());
                let block = parts.get(1).ok_or_else(|| class.clone())?;
                push(out, indent, &format!("choice {chance}:"));
                write_stmt(block, indent + 4, out)?;
            }
            Ok(())
        }
        "RawTime" => {
            let time = text_field(v, "time").ok_or_else(|| class.clone())?;
            push(out, indent, &format!("time {time}"));
            Ok(())
        }
        "RawOn" => {
            let Some(Value::Dict(handlers)) = field(v, "handlers") else {
                return Err(class);
            };
            for (name, block) in handlers {
                let name = expr_text(name).ok_or_else(|| class.clone())?;
                push(out, indent, &format!("on {name}:"));
                write_stmt(block, indent + 4, out)?;
            }
            Ok(())
        }
        "RawEvent" => {
            let name = text_field(v, "name").ok_or_else(|| class.clone())?;
            push(out, indent, &format!("event {name}"));
            Ok(())
        }
        "RawFunction" => {
            let expr = text_field(v, "expr").ok_or_else(|| class.clone())?;
            push(out, indent, &format!("function {expr}"));
            Ok(())
        }
        "RawContains" | "RawContainsExpr" => {
            let expr = text_field(v, "expression")
                .or_else(|| text_field(v, "expr"))
                .ok_or_else(|| class.clone())?;
            push(out, indent, &format!("contains {expr}"));
            Ok(())
        }
        "RawChild" => write_grouped(v, "children", "contains:", indent, out),
        _ => Err(if class.is_empty() {
            "atl".into()
        } else {
            class
        }),
    }
}

fn write_block(v: &Value, indent: usize, out: &mut String) -> Result<(), String> {
    if field(v, "animation").is_some_and(|a| matches!(a, Value::Bool(true))) {
        push(out, indent, "animation");
    }
    let Some(Value::List(statements)) = field(v, "statements") else {
        return Err(ast::class_of(v).unwrap_or("RawBlock").to_string());
    };
    for stmt in statements {
        write_stmt(stmt, indent, out)?;
    }
    Ok(())
}

fn write_grouped(
    v: &Value,
    key: &str,
    head: &str,
    indent: usize,
    out: &mut String,
) -> Result<(), String> {
    let class = ast::class_of(v).unwrap_or(key).to_string();
    let Some(Value::List(blocks)) = field(v, key) else {
        return Err(class);
    };
    for block in blocks {
        push(out, indent, head);
        write_stmt(block, indent + 4, out)?;
    }
    Ok(())
}

fn multipurpose(v: &Value) -> Result<String, String> {
    let class = ast::class_of(v).unwrap_or("RawMultipurpose").to_string();
    let mut parts = Vec::new();
    if let Some(warper) = text_field(v, "warper") {
        match text_field(v, "duration") {
            Some(duration) => parts.push(format!("{warper} {duration}")),
            None => parts.push(warper),
        }
    } else if let Some(warp) = field(v, "warp_function").filter(|w| !ast::is_none(w)) {
        match expr_text(warp) {
            Some(expr) if !expr.is_empty() && expr != "None" => parts.push(format!("warp {expr}")),
            _ => return Err(class),
        }
    }
    if let Some(props) = field(v, "properties") {
        parts.extend(pairs(props, "", &class)?);
    }
    if let Some(splines) = field(v, "splines") {
        parts.extend(pairs(splines, "spline", &class)?);
    }
    if let Some(revolution) = text_field(v, "revolution") {
        parts.push(revolution);
    }
    if let Some(circles) = text_field(v, "circles") {
        if circles != "0" {
            parts.push(format!("circles {circles}"));
        }
    }
    if let Some(Value::List(items)) = field(v, "expressions") {
        for item in items {
            let (expr, with) = match item {
                Value::Tuple(parts) => (
                    parts
                        .first()
                        .and_then(expr_text)
                        .ok_or_else(|| class.clone())?,
                    parts.get(1).and_then(expr_text).filter(|s| s != "None"),
                ),
                other => (expr_text(other).ok_or_else(|| class.clone())?, None),
            };
            match with {
                Some(with) => parts.push(format!("{expr} with {with}")),
                None => parts.push(expr),
            }
        }
    }
    if parts.is_empty() {
        return Err(class);
    }
    Ok(parts.join(" "))
}

fn pairs(v: &Value, prefix: &str, class: &str) -> Result<Vec<String>, String> {
    let Value::List(items) = v else {
        return Err(class.to_string());
    };
    let mut out = Vec::new();
    for item in items {
        let Value::Tuple(parts) = item else {
            return Err(class.to_string());
        };
        let name = parts
            .first()
            .and_then(expr_text)
            .ok_or_else(|| class.to_string())?;
        let exprs = match parts.get(1) {
            Some(Value::List(list) | Value::Tuple(list)) => list
                .iter()
                .filter_map(expr_text)
                .collect::<Vec<_>>()
                .join(" "),
            Some(other) => expr_text(other).unwrap_or_default(),
            None => String::new(),
        };
        if prefix.is_empty() {
            if exprs.is_empty() {
                out.push(name);
            } else {
                out.push(format!("{name} {exprs}"));
            }
        } else if exprs.is_empty() {
            out.push(format!("{prefix} {name}"));
        } else {
            out.push(format!("{prefix} {name} {exprs}"));
        }
    }
    Ok(out)
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
