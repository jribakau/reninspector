//! The decompiled text has to describe the same story flow as the compiled tree.
//! A mismatch keeps the file visible but read-only.

use crate::ast::{self, Stmt};
use crate::lexer;
use crate::parser;

use super::ast::{Kind, Node, Tree};

#[derive(Debug, Default, PartialEq, Eq)]
struct Flow {
    labels: Vec<(String, u32)>,
    jumps: Vec<(String, u32)>,
    calls: Vec<(String, u32)>,
    says: u32,
    choices: Vec<String>,
}

/// Why the decompiled text does not match the compiled tree.
pub fn explain(tree: &Tree, text: &str) -> Option<String> {
    if tree.partial {
        return Some("partial".into());
    }
    let lexed = lexer::lex(text);
    if let Some(issue) = lexed.issues.first() {
        return Some(format!("line {}: {}", issue.line, issue.message));
    }
    let parsed = parser::parse(&lexed.lines);
    if let Some(issue) = parsed.issues.first() {
        return Some(format!("line {}: {}", issue.line, issue.message));
    }
    let left = flow_nodes(&tree.nodes);
    let right = flow_stmts(&parsed.stmts);
    if same_flow(&left, &right) {
        return None;
    }
    Some(diff(&left, &right))
}

fn same_flow(left: &Flow, right: &Flow) -> bool {
    left.says == right.says
        && left.labels == right.labels
        && left.jumps == right.jumps
        && left.calls == right.calls
        && left.choices.len() == right.choices.len()
        && left
            .choices
            .iter()
            .zip(right.choices.iter())
            .all(|(a, b)| same_choice(a, b))
}

/// The project parser keeps 160 characters of a long line and adds an ellipsis.
fn same_choice(tree: &str, parsed: &str) -> bool {
    if tree == parsed {
        return true;
    }
    let Some(prefix) = parsed.strip_suffix('…') else {
        return false;
    };
    prefix.chars().count() >= 160 && tree.starts_with(prefix)
}

fn diff(left: &Flow, right: &Flow) -> String {
    if left.says != right.says {
        return format!("dialogue {} vs {}", left.says, right.says);
    }
    if left.labels.len() != right.labels.len() {
        return format!("labels {} vs {}", left.labels.len(), right.labels.len());
    }
    for (a, b) in left.labels.iter().zip(right.labels.iter()) {
        if a != b {
            return format!("label {:?} vs {:?}", a, b);
        }
    }
    if left.jumps.len() != right.jumps.len() {
        return format!("jumps {} vs {}", left.jumps.len(), right.jumps.len());
    }
    for (a, b) in left.jumps.iter().zip(right.jumps.iter()) {
        if a != b {
            return format!("jump {:?} vs {:?}", a, b);
        }
    }
    if left.calls.len() != right.calls.len() {
        return format!("calls {} vs {}", left.calls.len(), right.calls.len());
    }
    for (a, b) in left.calls.iter().zip(right.calls.iter()) {
        if a != b {
            return format!("call {:?} vs {:?}", a, b);
        }
    }
    if left.choices.len() != right.choices.len() {
        return format!("choices {} vs {}", left.choices.len(), right.choices.len());
    }
    for (a, b) in left.choices.iter().zip(right.choices.iter()) {
        if !same_choice(a, b) {
            return format!("choice {:?} vs {:?}", a, b);
        }
    }
    "flow".into()
}

fn flow_nodes(nodes: &[Node]) -> Flow {
    let mut flow = Flow::default();
    walk_nodes(nodes, &mut flow);
    flow
}

fn walk_nodes(nodes: &[Node], flow: &mut Flow) {
    for node in nodes {
        match &node.kind {
            Kind::Label { name, body, .. } => {
                flow.labels.push((name.clone(), node.line));
                walk_nodes(body, flow);
            }
            Kind::Say { .. } => flow.says += 1,
            Kind::Menu { items, .. } => {
                for item in items {
                    if item.caption_only {
                        continue;
                    }
                    flow.choices.push(item.caption.clone());
                    walk_nodes(&item.body, flow);
                }
            }
            Kind::Jump { target } => flow.jumps.push((target.clone(), node.line)),
            Kind::Call { label, .. } => flow.calls.push((label.clone(), node.line)),
            Kind::If { entries } => {
                for (_, body) in entries {
                    walk_nodes(body, flow);
                }
            }
            Kind::While { body, .. } | Kind::Init { body, .. } | Kind::Translate { body, .. } => {
                walk_nodes(body, flow);
            }
            Kind::Screen { .. } => {}
            _ => {}
        }
    }
}

fn flow_stmts(nodes: &[Stmt]) -> Flow {
    let mut flow = Flow::default();
    walk_stmts(nodes, &mut flow);
    flow
}

fn walk_stmts(nodes: &[Stmt], flow: &mut Flow) {
    for node in nodes {
        match &node.kind {
            ast::Kind::Label { name, body } => {
                flow.labels.push((name.clone(), node.line));
                walk_stmts(body, flow);
            }
            ast::Kind::Say { .. } => flow.says += 1,
            ast::Kind::Menu { choices, .. } => {
                for choice in choices {
                    flow.choices.push(choice.text.clone());
                    walk_stmts(&choice.body, flow);
                }
            }
            ast::Kind::Jump { target, dynamic } if !dynamic => {
                flow.jumps.push((target.clone(), node.line))
            }
            ast::Kind::Call {
                target, dynamic, ..
            } if !dynamic => flow.calls.push((target.clone(), node.line)),
            ast::Kind::If { branches } => {
                for branch in branches {
                    walk_stmts(&branch.body, flow);
                }
            }
            ast::Kind::While { body, .. } => walk_stmts(body, flow),
            ast::Kind::Screen { .. } => {}
            _ => {}
        }
    }
}
