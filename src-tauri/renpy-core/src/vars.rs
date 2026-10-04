//! Literal variable values for the stage preview.
//!
//! This is not a Python interpreter. A `default` or `define` with a literal
//! seeds the state, and `$` lines on the path to the caret update it. Anything
//! else — a call, a loop, an unpacking — makes that name unknown.

use std::collections::HashMap;

use crate::ast::{Kind, Stmt};

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Str(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    None,
    Unknown,
}

impl Value {
    /// Text Ren'Py would interpolate for this value inside `[name]`.
    pub fn interpolate(&self) -> Option<String> {
        match self {
            Value::Str(s) => Some(s.clone()),
            Value::Int(n) => Some(n.to_string()),
            Value::Float(n) => Some(format_float(*n)),
            Value::Bool(true) => Some("True".into()),
            Value::Bool(false) => Some("False".into()),
            Value::None => Some("None".into()),
            Value::Unknown => None,
        }
    }
}

fn format_float(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{n:.1}")
    } else {
        n.to_string()
    }
}

/// A quoted string, number, `True`, `False` or `None`. Anything else is not a literal.
pub fn parse_literal(text: &str) -> Option<Value> {
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    match t {
        "True" => return Some(Value::Bool(true)),
        "False" => return Some(Value::Bool(false)),
        "None" => return Some(Value::None),
        _ => {}
    }
    if let Some(s) = unquote(t) {
        return Some(Value::Str(s));
    }
    if is_number_token(t) {
        if !t.contains('.') && !t.contains('e') && !t.contains('E') {
            if let Ok(n) = t.parse::<i64>() {
                return Some(Value::Int(n));
            }
        }
        if let Ok(n) = t.parse::<f64>() {
            return Some(Value::Float(n));
        }
    }
    None
}

fn is_number_token(t: &str) -> bool {
    let mut chars = t.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if first == '-' || first == '+' {
        return chars.next().is_some_and(|c| c.is_ascii_digit() || c == '.');
    }
    first.is_ascii_digit() || first == '.'
}

fn unquote(t: &str) -> Option<String> {
    let bytes = t.as_bytes();
    if bytes.len() < 2 {
        return None;
    }
    let q = bytes[0];
    if (q != b'"' && q != b'\'') || bytes[bytes.len() - 1] != q {
        return None;
    }
    let inner = &t[1..t.len() - 1];
    let mut out = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    Some(out)
}

#[derive(Debug, Clone, Default)]
pub struct VarState {
    vars: HashMap<String, Value>,
}

impl VarState {
    /// Top-level `default` and `define` whose value is a literal.
    pub fn seed(&mut self, stmts: &[Stmt]) {
        for stmt in stmts {
            let Kind::Define { name, value, .. } = &stmt.kind else {
                continue;
            };
            if let Some(v) = parse_literal(value) {
                self.vars.insert(name.clone(), v);
            }
        }
    }

    pub fn get(&self, name: &str) -> Option<&Value> {
        self.vars.get(name)
    }

    /// Applies one statement or a block. `$` and `python:` headers are ignored.
    pub fn apply(&mut self, text: &str) {
        for line in text.lines() {
            let line = strip_comment(line);
            let line = line.trim();
            if line.is_empty() || line == "pass" {
                continue;
            }
            let line = line.strip_prefix('$').unwrap_or(line).trim();
            if line.is_empty() || is_python_header(line) {
                continue;
            }
            for piece in split_top(line, |c| c == ';') {
                self.apply_stmt(piece.trim());
            }
        }
    }

    fn apply_stmt(&mut self, line: &str) {
        if let Some((left, op, right)) = aug_assign(line) {
            let Some(name) = ident(left) else {
                self.mark_unknown_targets(left);
                return;
            };
            let Some(Value::Int(delta)) = parse_literal(right) else {
                self.vars.insert(name.to_string(), Value::Unknown);
                return;
            };
            match self.vars.get(name) {
                Some(Value::Int(cur)) => {
                    let next = if op == "+=" {
                        cur.saturating_add(delta)
                    } else {
                        cur.saturating_sub(delta)
                    };
                    self.vars.insert(name.to_string(), Value::Int(next));
                }
                Some(Value::Float(cur)) => {
                    let next = if op == "+=" {
                        *cur + delta as f64
                    } else {
                        *cur - delta as f64
                    };
                    self.vars.insert(name.to_string(), Value::Float(next));
                }
                _ => {
                    self.vars.insert(name.to_string(), Value::Unknown);
                }
            }
            return;
        }
        let Some(parts) = assign_parts(line) else {
            return;
        };
        if parts.len() < 2 {
            return;
        }
        let rhs = parts.last().map(|s| s.as_str()).unwrap_or("");
        let lefts = &parts[..parts.len() - 1];
        if let Some(value) = parse_literal(rhs) {
            if lefts.iter().all(|p| ident(p).is_some()) {
                for name in lefts {
                    let name = ident(name).unwrap();
                    self.vars.insert(name.to_string(), value.clone());
                }
                return;
            }
        }
        for left in lefts {
            self.mark_unknown_targets(left);
        }
    }

    fn mark_unknown_targets(&mut self, target: &str) {
        for piece in split_top(target, |c| c == ',') {
            if let Some(name) = ident(piece.trim()) {
                self.vars.insert(name.to_string(), Value::Unknown);
            }
        }
    }

    /// Python truthiness. `None` means a name was unknown or the syntax is not supported.
    pub fn eval(&self, cond: &str) -> Option<bool> {
        let mut p = Eval {
            s: cond,
            i: 0,
            state: self,
        };
        let value = p.parse_or()?;
        p.skip();
        if p.i != p.s.len() {
            return None;
        }
        truth(&value)
    }

    /// Names in `cond` that are missing or unknown. Keywords are skipped.
    pub fn unknown_names(&self, cond: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut chars = cond.char_indices().peekable();
        let mut quote: Option<char> = None;
        while let Some((i, c)) = chars.next() {
            if let Some(q) = quote {
                if c == '\\' {
                    chars.next();
                    continue;
                }
                if c == q {
                    quote = None;
                }
                continue;
            }
            if c == '"' || c == '\'' {
                quote = Some(c);
                continue;
            }
            if !(c.is_ascii_alphabetic() || c == '_') {
                continue;
            }
            let rest = &cond[i..];
            let end = rest
                .find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
                .unwrap_or(rest.len());
            let name = &rest[..end];
            for _ in name.chars().skip(1) {
                chars.next();
            }
            if is_keyword(name) {
                continue;
            }
            match self.vars.get(name) {
                Some(Value::Unknown) | None => {
                    if !out.iter().any(|n| n == name) {
                        out.push(name.to_string());
                    }
                }
                Some(_) => {}
            }
        }
        out
    }
}

fn is_keyword(name: &str) -> bool {
    matches!(
        name,
        "and" | "or" | "not" | "is" | "in" | "True" | "False" | "None"
    )
}

fn is_python_header(line: &str) -> bool {
    let rest = line.strip_prefix("init").unwrap_or(line).trim_start();
    let Some(rest) = rest.strip_prefix("python") else {
        return false;
    };
    let rest = rest.trim_start();
    rest.is_empty() || rest.starts_with(':')
}

fn ident(text: &str) -> Option<&str> {
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    let mut chars = t.chars();
    let first = chars.next()?;
    if !(first.is_ascii_alphabetic() || first == '_') {
        return None;
    }
    if chars.all(|c| c.is_ascii_alphanumeric() || c == '_') {
        Some(t)
    } else {
        None
    }
}

fn strip_comment(line: &str) -> String {
    let mut quote: Option<char> = None;
    let mut out = String::new();
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if let Some(q) = quote {
            out.push(c);
            if c == '\\' {
                if let Some(n) = chars.next() {
                    out.push(n);
                }
                continue;
            }
            if c == q {
                quote = None;
            }
            continue;
        }
        if c == '"' || c == '\'' {
            quote = Some(c);
            out.push(c);
            continue;
        }
        if c == '#' {
            break;
        }
        out.push(c);
    }
    out
}

/// `a += 1` or `a -= 1` at the top level, outside quotes.
fn aug_assign(line: &str) -> Option<(&str, &str, &str)> {
    let bytes = line.as_bytes();
    let mut i = 0;
    let mut quote: Option<u8> = None;
    let mut depth = 0i32;
    while i + 1 < bytes.len() {
        let c = bytes[i];
        if let Some(q) = quote {
            if c == b'\\' {
                i += 2;
                continue;
            }
            if c == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        if c == b'"' || c == b'\'' {
            quote = Some(c);
            i += 1;
            continue;
        }
        if c == b'(' || c == b'[' || c == b'{' {
            depth += 1;
        } else if c == b')' || c == b']' || c == b'}' {
            depth -= 1;
        }
        if depth == 0 && (c == b'+' || c == b'-') && bytes[i + 1] == b'=' {
            let op = if c == b'+' { "+=" } else { "-=" };
            return Some((line[..i].trim(), op, line[i + 2..].trim()));
        }
        i += 1;
    }
    None
}

/// Pieces of `a = b = "x"`. Comparisons (`==`, `!=`, `<=`, `>=`) are not splits.
fn assign_parts(line: &str) -> Option<Vec<String>> {
    let bytes = line.as_bytes();
    let mut parts = Vec::new();
    let mut start = 0;
    let mut i = 0;
    let mut quote: Option<u8> = None;
    let mut depth = 0i32;
    while i < bytes.len() {
        let c = bytes[i];
        if let Some(q) = quote {
            if c == b'\\' {
                i += 2;
                continue;
            }
            if c == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        if c == b'"' || c == b'\'' {
            quote = Some(c);
            i += 1;
            continue;
        }
        if c == b'(' || c == b'[' || c == b'{' {
            depth += 1;
            i += 1;
            continue;
        }
        if c == b')' || c == b']' || c == b'}' {
            depth -= 1;
            i += 1;
            continue;
        }
        if depth == 0 && c == b'=' {
            let prev = if i > 0 { bytes[i - 1] } else { b' ' };
            let next = bytes.get(i + 1).copied().unwrap_or(b' ');
            let comparison =
                matches!(prev, b'=' | b'!' | b'<' | b'>' | b'+' | b'-') || next == b'=';
            if !comparison {
                parts.push(line[start..i].trim().to_string());
                start = i + 1;
            }
        }
        i += 1;
    }
    if parts.is_empty() {
        return None;
    }
    parts.push(line[start..].trim().to_string());
    Some(parts)
}

fn split_top(line: &str, is_sep: impl Fn(char) -> bool) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut quote: Option<char> = None;
    let mut depth = 0i32;
    for (i, c) in line.char_indices() {
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
            continue;
        }
        if c == '"' || c == '\'' {
            quote = Some(c);
            continue;
        }
        if c == '(' || c == '[' || c == '{' {
            depth += 1;
            continue;
        }
        if c == ')' || c == ']' || c == '}' {
            depth -= 1;
            continue;
        }
        if depth == 0 && is_sep(c) {
            out.push(&line[start..i]);
            start = i + c.len_utf8();
        }
    }
    out.push(&line[start..]);
    out
}

fn truth(value: &Value) -> Option<bool> {
    match value {
        Value::Unknown => None,
        Value::None | Value::Bool(false) => Some(false),
        Value::Bool(true) => Some(true),
        Value::Int(0) => Some(false),
        Value::Int(_) => Some(true),
        Value::Float(n) if *n == 0.0 => Some(false),
        Value::Float(_) => Some(true),
        Value::Str(s) if s.is_empty() => Some(false),
        Value::Str(_) => Some(true),
    }
}

struct Eval<'a> {
    s: &'a str,
    i: usize,
    state: &'a VarState,
}

impl<'a> Eval<'a> {
    fn skip(&mut self) {
        let rest = &self.s[self.i..];
        let n = rest
            .chars()
            .take_while(|c| c.is_whitespace())
            .map(|c| c.len_utf8())
            .sum::<usize>();
        self.i += n;
    }

    fn parse_or(&mut self) -> Option<Value> {
        let mut left = self.parse_and()?;
        while self.eat_word("or") {
            let right = self.parse_and()?;
            left = match truth(&left) {
                Some(true) => left,
                Some(false) => right,
                None => Value::Unknown,
            };
        }
        Some(left)
    }

    fn parse_and(&mut self) -> Option<Value> {
        let mut left = self.parse_not()?;
        while self.eat_word("and") {
            let right = self.parse_not()?;
            left = match truth(&left) {
                Some(false) => left,
                Some(true) => right,
                None => Value::Unknown,
            };
        }
        Some(left)
    }

    fn parse_not(&mut self) -> Option<Value> {
        if self.eat_word("not") {
            let inner = self.parse_not()?;
            return match truth(&inner) {
                Some(b) => Some(Value::Bool(!b)),
                None => Some(Value::Unknown),
            };
        }
        self.parse_cmp()
    }

    fn parse_cmp(&mut self) -> Option<Value> {
        let left = self.parse_atom()?;
        let op = self.cmp_op()?;
        if op.is_empty() {
            return Some(left);
        }
        let right = self.parse_atom()?;
        Some(compare(&left, op, &right))
    }

    fn cmp_op(&mut self) -> Option<&'static str> {
        self.skip();
        for op in ["==", "!=", "<=", ">=", "<", ">"] {
            if self.s[self.i..].starts_with(op) {
                self.i += op.len();
                return Some(op);
            }
        }
        if self.eat_word("is") {
            return Some(if self.eat_word("not") { "!=" } else { "==" });
        }
        if self.eat_word("not") && self.eat_word("in") {
            return Some("not in");
        }
        if self.eat_word("in") {
            return Some("in");
        }
        Some("")
    }

    fn parse_atom(&mut self) -> Option<Value> {
        self.skip();
        if self.i >= self.s.len() {
            return None;
        }
        let rest = &self.s[self.i..];
        if rest.starts_with('(') {
            self.i += 1;
            let inner = self.parse_or()?;
            self.skip();
            if !self.s[self.i..].starts_with(')') {
                return None;
            }
            self.i += 1;
            return Some(inner);
        }
        if rest.starts_with('\'') || rest.starts_with('"') {
            let q = rest.chars().next()?;
            let mut end = None;
            let mut chars = rest[1..].char_indices();
            while let Some((n, c)) = chars.next() {
                if c == '\\' {
                    chars.next();
                    continue;
                }
                if c == q {
                    end = Some(n + 1);
                    break;
                }
            }
            let end = end?;
            let lit = &rest[..=end];
            self.i += lit.len();
            return parse_literal(lit);
        }
        if rest.starts_with('-') || rest.starts_with('+') {
            return self.parse_number();
        }
        let name_len = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '.')
            .map(|c| c.len_utf8())
            .sum::<usize>();
        if name_len == 0 {
            return None;
        }
        let word = &rest[..name_len];
        if is_number_token(word) {
            return self.parse_number();
        }
        self.i += name_len;
        match word {
            "True" => Some(Value::Bool(true)),
            "False" => Some(Value::Bool(false)),
            "None" => Some(Value::None),
            other if is_keyword(other) || other.contains('.') => None,
            other => Some(self.state.get(other).cloned().unwrap_or(Value::Unknown)),
        }
    }

    fn parse_number(&mut self) -> Option<Value> {
        self.skip();
        let rest = &self.s[self.i..];
        let len = rest
            .chars()
            .take_while(|c| c.is_ascii_digit() || matches!(*c, '.' | '-' | '+' | 'e' | 'E'))
            .map(|c| c.len_utf8())
            .sum::<usize>();
        if len == 0 {
            return None;
        }
        let token = &rest[..len];
        self.i += len;
        parse_literal(token)
    }

    fn eat_word(&mut self, word: &str) -> bool {
        self.skip();
        let rest = &self.s[self.i..];
        if !rest.starts_with(word) {
            return false;
        }
        let after = rest[word.len()..].chars().next();
        if after.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_') {
            return false;
        }
        self.i += word.len();
        true
    }
}

fn compare(left: &Value, op: &str, right: &Value) -> Value {
    if matches!(left, Value::Unknown) || matches!(right, Value::Unknown) {
        return Value::Unknown;
    }
    let result = match op {
        "in" => contains(right, left),
        "not in" => contains(right, left).map(|b| !b),
        "==" => Some(values_eq(left, right)),
        "!=" => Some(!values_eq(left, right)),
        "<" | "<=" | ">" | ">=" => order(left, right).map(|ord| match op {
            "<" => ord.is_lt(),
            "<=" => ord.is_le(),
            ">" => ord.is_gt(),
            _ => ord.is_ge(),
        }),
        _ => None,
    };
    match result {
        Some(b) => Value::Bool(b),
        None => Value::Unknown,
    }
}

fn values_eq(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Int(a), Value::Float(b)) | (Value::Float(b), Value::Int(a)) => *a as f64 == *b,
        (Value::Int(a), Value::Int(b)) => a == b,
        (Value::Float(a), Value::Float(b)) => a == b,
        (Value::Str(a), Value::Str(b)) => a == b,
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::None, Value::None) => true,
        _ => false,
    }
}

fn order(left: &Value, right: &Value) -> Option<std::cmp::Ordering> {
    match (left, right) {
        (Value::Int(a), Value::Int(b)) => Some(a.cmp(b)),
        (Value::Float(a), Value::Float(b)) => a.partial_cmp(b),
        (Value::Int(a), Value::Float(b)) => (*a as f64).partial_cmp(b),
        (Value::Float(a), Value::Int(b)) => a.partial_cmp(&(*b as f64)),
        (Value::Str(a), Value::Str(b)) => Some(a.cmp(b)),
        _ => None,
    }
}

fn contains(hay: &Value, needle: &Value) -> Option<bool> {
    match (hay, needle) {
        (Value::Str(h), Value::Str(n)) => Some(h.contains(n.as_str())),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(lines: &str) -> VarState {
        let mut s = VarState::default();
        s.apply(lines);
        s
    }

    #[test]
    fn literals_chained_and_increment_and_a_call() {
        assert_eq!(parse_literal("\"smile\""), Some(Value::Str("smile".into())));
        assert_eq!(parse_literal("'n'"), Some(Value::Str("n".into())));
        assert_eq!(parse_literal("1"), Some(Value::Int(1)));
        assert_eq!(parse_literal("True"), Some(Value::Bool(true)));
        assert_eq!(parse_literal("None"), Some(Value::None));
        assert_eq!(parse_literal("pick()"), None);

        let s = state("fian = \"n\"\na = b = \"smile\"\nian_look = 1\nian_look += 2\nian_look -= 1\nother = pick()\n");
        assert_eq!(s.get("fian"), Some(&Value::Str("n".into())));
        assert_eq!(s.get("a"), Some(&Value::Str("smile".into())));
        assert_eq!(s.get("b"), Some(&Value::Str("smile".into())));
        assert_eq!(s.get("ian_look"), Some(&Value::Int(2)));
        assert_eq!(s.get("other"), Some(&Value::Unknown));
    }

    #[test]
    fn eval_and_or_not_comparisons_and_an_unknown_name() {
        let s = state("fian = \"smile\"\nchapter = 0\nian_look = 1\n");
        assert_eq!(s.eval("fian == 'smile'"), Some(true));
        assert_eq!(s.eval("fian == 'sad'"), Some(false));
        assert_eq!(s.eval("ian_look == 1 and chapter > 9"), Some(false));
        assert_eq!(s.eval("chapter > 9 or fian == 'smile'"), Some(true));
        assert_eq!(s.eval("not fian == 'sad'"), Some(true));
        assert_eq!(s.eval("True"), Some(true));
        assert_eq!(s.eval("ian_fit == 1"), None);
        assert_eq!(
            s.unknown_names("ian_fit == 1 and chapter > 9"),
            vec!["ian_fit".to_string()]
        );
        assert_eq!(s.eval("'mi' in fian"), Some(true));
        assert_eq!(s.eval("fian is None"), Some(false));
    }
}
