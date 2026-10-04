//! Screens that lead somewhere: `Jump("x")` buttons, `renpy.call(...)`, `use` and
//! `Show(...)` chains. Many games (sandboxes, phone menus, maps) navigate only
//! through screens, so they belong on the map next to labels.

use std::collections::{BTreeSet, HashMap, HashSet};

use crate::analysis::walk_all;
use crate::ast::{Kind, NameAt};
use crate::project::SourceFile;

#[derive(Debug, Clone)]
pub struct ScreenInfo {
    pub name: String,
    pub file: usize,
    pub line: u32,
    pub end_line: u32,
    /// Labels named directly by this screen's actions.
    pub labels: Vec<String>,
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

fn unique_names(sites: &[NameAt]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for site in sites {
        if seen.insert(site.name.as_str()) {
            out.push(site.name.clone());
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
                            labels: unique_names(labels),
                            uses: unique_names(uses),
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
}
