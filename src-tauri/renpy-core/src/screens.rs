//! Screens that lead somewhere: `Jump("x")` buttons, `renpy.call(...)`, `use` and
//! `Show(...)` chains. Many games (sandboxes, phone menus, maps) navigate only
//! through screens, so they belong on the map next to labels.

use std::collections::{BTreeSet, HashMap, HashSet};

use crate::analysis::walk_all;
use crate::ast::{Kind, ScreenAction};
use crate::project::SourceFile;

#[derive(Debug, Clone)]
pub struct ScreenInfo {
    pub name: String,
    pub file: usize,
    pub line: u32,
    pub end_line: u32,
    /// Labels named directly by this screen's actions.
    pub labels: Vec<String>,
    /// Direct transfers, with jump versus call and any condition.
    pub actions: Vec<ScreenAction>,
    /// Other screens it uses or opens.
    pub uses: Vec<String>,
    /// Every label reachable through this screen and the screens it uses.
    pub reach: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ScreenTable {
    pub by_name: HashMap<String, ScreenInfo>,
    /// Definition order, for stable output.
    pub order: Vec<String>,
}

fn unique_names<'a>(names: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for name in names {
        if seen.insert(name) {
            out.push(name.to_string());
        }
    }
    out
}

impl ScreenTable {
    pub fn build(files: &[SourceFile]) -> ScreenTable {
        let mut table = ScreenTable::default();
        for (fi, f) in files.iter().enumerate() {
            walk_all(&f.stmts, &mut |s| {
                if let Kind::Screen { name, labels, uses } = &s.kind {
                    if table.by_name.contains_key(name) {
                        return; // first definition wins, like labels
                    }
                    table.order.push(name.clone());
                    table.by_name.insert(
                        name.clone(),
                        ScreenInfo {
                            name: name.clone(),
                            file: fi,
                            line: s.line,
                            end_line: s.end_line,
                            labels: unique_names(labels.iter().map(|a| a.name.as_str())),
                            actions: labels.clone(),
                            uses: unique_names(uses.iter().map(|u| u.name.as_str())),
                            reach: Vec::new(),
                        },
                    );
                }
            });
        }
        let names = table.order.clone();
        for name in names {
            let mut seen: BTreeSet<String> = BTreeSet::new();
            let mut stack = vec![name.clone()];
            let mut visited: BTreeSet<String> = BTreeSet::new();
            while let Some(cur) = stack.pop() {
                if !visited.insert(cur.clone()) {
                    continue;
                }
                if let Some(info) = table.by_name.get(&cur) {
                    seen.extend(info.labels.iter().cloned());
                    stack.extend(info.uses.iter().cloned());
                }
            }
            if let Some(info) = table.by_name.get_mut(&name) {
                info.reach = seen.into_iter().collect();
            }
        }
        table
    }

    /// True when the screen can take the player to a label.
    pub fn navigational(&self, name: &str) -> bool {
        self.by_name
            .get(name)
            .map(|s| !s.reach.is_empty())
            .unwrap_or(false)
    }

    pub fn reach(&self, name: &str) -> &[String] {
        self.by_name
            .get(name)
            .map(|s| s.reach.as_slice())
            .unwrap_or(&[])
    }

    /// Jump and call actions on this screen and the screens it uses.
    pub fn reach_actions(&self, name: &str) -> Vec<ScreenAction> {
        let mut out = Vec::new();
        let mut seen_screens = BTreeSet::new();
        let mut stack = vec![name.to_string()];
        while let Some(cur) = stack.pop() {
            if !seen_screens.insert(cur.clone()) {
                continue;
            }
            let Some(info) = self.by_name.get(&cur) else {
                continue;
            };
            for action in &info.actions {
                let seen = out.iter().any(|e: &ScreenAction| {
                    e.name == action.name && e.how == action.how && e.cond == action.cond
                });
                if !seen {
                    out.push(action.clone());
                }
            }
            stack.extend(info.uses.iter().cloned());
        }
        out
    }
}
