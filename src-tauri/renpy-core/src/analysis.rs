//! Project-wide analysis: label table, the story map and reachability.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use rayon::prelude::*;
use serde::Serialize;

use crate::ast::*;
use crate::diagnostics::{self, DiagReport, MissingRef};
use crate::engine::EngineDump;
use crate::flow::{visit_labels, GKind};
use crate::project::{ImageIndex, SourceFile};
use crate::screens::ScreenTable;

#[derive(Debug, Clone)]
pub struct LabelDef {
    pub name: String,
    pub file: usize,
    pub line: u32,
    pub end_line: u32,
    /// `"label"` or `"menu"` (named menus are jump targets too).
    pub kind: &'static str,
    /// First definition wins; later ones are duplicates.
    pub primary: bool,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub files: u32,
    pub lines: u64,
    pub labels: u32,
    pub menus: u32,
    pub says: u64,
    pub edges: u32,
    pub opaque: u32,
    pub dynamic_jumps: u32,
    pub missing_targets: u32,
    pub unreachable: u32,
    pub duplicates: u32,
    /// Labels known only from the engine dump (compiled scripts).
    pub compiled_labels: u32,
    pub screens: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MapNode {
    pub id: String,
    /// `label`, `menu` or `missing`.
    pub kind: &'static str,
    /// Index into the project's file list; -1 for missing targets.
    pub file: i32,
    pub line: u32,
    pub end_line: u32,
    pub stmts: u32,
    pub says: u32,
    pub menus: u32,
    pub choices: u32,
    pub in_degree: u32,
    pub out_degree: u32,
    pub reachable: bool,
    pub root: bool,
    /// Only reached through a label name kept as data (a string in code).
    pub indirect: bool,
    pub duplicate: bool,
    pub returns: bool,
    pub ends_script: bool,
    pub dynamic_out: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MapEdge {
    pub from: String,
    pub to: String,
    /// `jump`, `choice`, `fall` or `call`.
    pub kind: &'static str,
    pub count: u32,
    /// Set for a single conditional choice or screen button. Empty when several links share the edge.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub badge: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProjectMap {
    pub nodes: Vec<MapNode>,
    pub edges: Vec<MapEdge>,
}

#[derive(Default)]
pub struct Analysis {
    pub defs: Vec<LabelDef>,
    pub map: ProjectMap,
    pub diagnostics: DiagReport,
    pub stats: Stats,
    pub engine_diff: Option<EngineDiff>,
    pub screens: ScreenTable,
    pub catalog: crate::catalog::Catalog,
    /// Primary label or menu name to its index in `defs`.
    pub by_name: HashMap<String, usize>,
}

/// What the engine's dump and our parser disagree on.
#[derive(Debug, Clone, Default)]
pub struct EngineDiff {
    /// Parser labels the engine does not know: (name, file, line).
    pub parser_only: Vec<(String, String, u32)>,
    /// Engine labels in a file we parsed but did not find there.
    pub engine_only: Vec<(String, String, u32)>,
    /// Labels at a different place: (name, parser file+line, engine file+line).
    pub moved: Vec<MovedLabel>,
}

/// Label name, parser (file, line), engine (file, line).
pub type MovedLabel = (String, (String, u32), (String, u32));

impl EngineDiff {
    pub fn is_empty(&self) -> bool {
        self.parser_only.is_empty() && self.engine_only.is_empty() && self.moved.is_empty()
    }
}

/// Information that does not come from the `.rpy` files themselves.
#[derive(Default, Clone, Copy)]
pub struct External<'a> {
    pub engine: Option<&'a EngineDump>,
    /// `.rpyc` files with no `.rpy` next to them (paths relative to `game/`).
    pub compiled_only: &'a [String],
    /// Archives that could not be read, or that hold only compiled scripts.
    pub archives: &'a [String],
    /// Scripts present in more than one place; the lower-priority copy is not mapped.
    pub shadowed: &'a [String],
    /// Patch entries whose original archive changed after the patch was baked.
    pub stale: &'a [String],
}

impl External<'_> {
    /// True when parts of the game exist that we cannot read.
    pub fn source_less(&self) -> bool {
        !self.compiled_only.is_empty() || !self.archives.is_empty()
    }
}

const ROOT_LABELS: &[&str] = &[
    "start",
    "splashscreen",
    "before_main_menu",
    "main_menu",
    "after_load",
    "after_warp",
    "quit",
    "hide_windows",
    "before_start",
];

/// Screens the engine shows without any script statement.
const SCREEN_ROOTS: &[&str] = &["main_menu", "navigation", "quick_menu", "game_menu"];

struct RegionEdge {
    target: String,
    kind: &'static str,
    line: u32,
    caption: Option<String>,
    cond: Option<String>,
}

struct EdgeAcc {
    count: u32,
    badge: Option<String>,
}

const BADGE_MAX: usize = 48;

/// A short chip for one conditional link. Several links to the same place share no chip.
fn edge_badge(caption: Option<&str>, cond: Option<&str>) -> Option<String> {
    let cond = cond.map(str::trim).filter(|s| !s.is_empty())?;
    if cond.chars().count() > BADGE_MAX {
        return None;
    }
    let cap = caption
        .map(str::trim)
        .filter(|s| !s.is_empty() && s.chars().count() <= 24 && !s.contains(cond));
    match cap {
        Some(cap) => {
            let full = format!("{cap} · {cond}");
            if full.chars().count() <= BADGE_MAX + 8 {
                Some(full)
            } else {
                Some(cond.to_string())
            }
        }
        None => Some(cond.to_string()),
    }
}

fn push_edge(
    agg: &mut std::collections::BTreeMap<(String, String, &'static str), EdgeAcc>,
    from: String,
    to: String,
    kind: &'static str,
    badge: Option<String>,
) {
    let entry = agg.entry((from, to, kind)).or_insert(EdgeAcc {
        count: 0,
        badge: None,
    });
    if entry.count == 0 {
        entry.badge = badge;
    } else if entry.badge != badge {
        entry.badge = None;
    }
    entry.count += 1;
}

struct Region {
    def: usize,
    edges: Vec<RegionEdge>,
    dynamic: u32,
    returns: bool,
    ends: bool,
    stmts: u32,
    says: u32,
    menus: u32,
    choices: u32,
    end_line: u32,
}

/// Visit every statement, descending into all nested bodies.
pub fn walk_all(stmts: &[Stmt], f: &mut dyn FnMut(&Stmt)) {
    for s in stmts {
        f(s);
        match &s.kind {
            Kind::Label { body, .. } | Kind::While { body, .. } => walk_all(body, f),
            Kind::Menu { choices, .. } => {
                for c in choices {
                    walk_all(&c.body, f);
                }
            }
            Kind::If { branches } => {
                for b in branches {
                    walk_all(&b.body, f);
                }
            }
            _ => {}
        }
    }
}

pub fn analyze(files: &[SourceFile], auto_images: &ImageIndex, ext: &External) -> Analysis {
    let mut defs: Vec<LabelDef> = Vec::new();
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut primaries: Vec<Primary> = Vec::new();
    let screens = ScreenTable::build(files);

    for (fi, file) in files.iter().enumerate() {
        visit_labels(&file.stmts, None, &mut |stmt, _cont| {
            let (name, kind) = match &stmt.kind {
                Kind::Label { name, .. } => (name.clone(), "label"),
                Kind::Menu { name: Some(n), .. } => (n.clone(), "menu"),
                _ => return,
            };
            let primary = !seen.contains_key(&name);
            let idx = defs.len();
            defs.push(LabelDef {
                name: name.clone(),
                file: fi,
                line: stmt.line,
                end_line: stmt.end_line,
                kind,
                primary,
            });
            if !primary {
                return;
            }
            seen.insert(name, idx);
            primaries.push(Primary {
                def: idx,
                file: fi,
                line: stmt.line,
                end_line: stmt.end_line,
            });
        });
    }

    let regions: Vec<Region> = primaries
        .par_iter()
        .map(|p| build_region(files, &screens, p))
        .collect();

    // Names that can be jumped to besides labels: `call x from y` return labels.
    let mut known: HashSet<String> = seen.keys().cloned().collect();
    for f in files {
        walk_all(&f.stmts, &mut |s| {
            if let Kind::Call {
                from: Some(name), ..
            } = &s.kind
            {
                known.insert(name.clone());
            }
        });
    }

    // Labels only the engine knows about (compiled scripts, files we do not read).
    let empty_labels: BTreeMap<String, crate::engine::Location> = BTreeMap::new();
    let engine_labels = ext.engine.map(|d| &d.labels).unwrap_or(&empty_labels);
    let engine_only: Vec<(&String, &crate::engine::Location)> = engine_labels
        .iter()
        .filter(|(n, _)| !seen.contains_key(n.as_str()) && !known.contains(n.as_str()))
        .collect();

    // Aggregate edges, find missing targets.
    let mut agg: BTreeMap<(String, String, &'static str), EdgeAcc> = BTreeMap::new();
    let mut missing: Vec<MissingRef> = Vec::new();
    let mut missing_names: HashSet<String> = HashSet::new();
    for r in &regions {
        let from = &defs[r.def];
        for e in &r.edges {
            let badge = edge_badge(e.caption.as_deref(), e.cond.as_deref());
            if e.kind == "screen" {
                push_edge(&mut agg, from.name.clone(), e.target.clone(), e.kind, badge);
                continue;
            }
            if known.contains(&e.target) && !seen.contains_key(&e.target) {
                // Exists only as a `call ... from` marker; not a map node.
                continue;
            }
            if !seen.contains_key(&e.target) && !engine_labels.contains_key(e.target.as_str()) {
                missing_names.insert(e.target.clone());
                missing.push(MissingRef {
                    file: from.file,
                    line: e.line,
                    target: e.target.clone(),
                    from: from.name.clone(),
                });
            }
            push_edge(&mut agg, from.name.clone(), e.target.clone(), e.kind, badge);
        }
    }

    // Screens that lead somewhere: action edges to labels, use/Show edges to screens.
    let screen_names: Vec<&String> = screens
        .order
        .iter()
        .filter(|n| screens.navigational(n))
        .collect();
    for name in &screen_names {
        let info = &screens.by_name[name.as_str()];
        let id = format!("screen:{name}");
        for action in &info.actions {
            let target = &action.name;
            if !seen.contains_key(target)
                && !engine_labels.contains_key(target.as_str())
                && !known.contains(target)
            {
                missing_names.insert(target.clone());
                missing.push(MissingRef {
                    file: info.file,
                    line: action.line,
                    target: target.clone(),
                    from: id.clone(),
                });
            }
            let kind = if action.how == "call" { "call" } else { "jump" };
            push_edge(
                &mut agg,
                id.clone(),
                target.clone(),
                kind,
                edge_badge(action.caption.as_deref(), action.cond.as_deref()),
            );
        }
        for used in &info.uses {
            if screens.navigational(used) && used != name.as_str() {
                push_edge(
                    &mut agg,
                    id.clone(),
                    format!("screen:{used}"),
                    "screen",
                    None,
                );
            }
        }
    }

    // Reachability.
    let mut adjacency: HashMap<&str, Vec<&str>> = HashMap::new();
    for (from, to, _) in agg.keys() {
        adjacency
            .entry(from.as_str())
            .or_default()
            .push(to.as_str());
    }
    let mut action_refs: HashSet<&str> = HashSet::new();
    for f in files {
        for n in &f.meta.action_refs {
            action_refs.insert(n.as_str());
        }
    }
    let mut data_refs: HashSet<&str> = HashSet::new();
    for f in files {
        for n in &f.meta.data_refs {
            data_refs.insert(n.as_str());
        }
    }
    let mut roots: HashSet<&str> = HashSet::new();
    let mut indirect: HashSet<&str> = HashSet::new();
    for name in seen.keys() {
        if ROOT_LABELS.contains(&name.as_str()) || action_refs.contains(name.as_str()) {
            roots.insert(name.as_str());
        } else if data_refs.contains(name.as_str()) {
            indirect.insert(name.as_str());
        }
    }
    // Screens the engine or the game shows by itself start out reachable.
    let screen_ids: Vec<(String, bool)> = screen_names
        .iter()
        .map(|n| {
            let by_engine = SCREEN_ROOTS.contains(&n.as_str());
            (
                format!("screen:{n}"),
                by_engine || data_refs.contains(n.as_str()),
            )
        })
        .collect();
    let mut reachable: HashSet<&str> = roots.union(&indirect).copied().collect();
    for (id, start) in &screen_ids {
        if *start {
            reachable.insert(id.as_str());
        }
    }
    let mut queue: VecDeque<&str> = reachable.iter().copied().collect();
    while let Some(cur) = queue.pop_front() {
        if let Some(next) = adjacency.get(cur) {
            for &n in next {
                if reachable.insert(n) {
                    queue.push_back(n);
                }
            }
        }
    }

    // Degrees.
    let mut in_deg: HashMap<&str, u32> = HashMap::new();
    let mut out_deg: HashMap<&str, u32> = HashMap::new();
    for (from, to, _) in agg.keys() {
        *out_deg.entry(from.as_str()).or_insert(0) += 1;
        *in_deg.entry(to.as_str()).or_insert(0) += 1;
    }

    let mut dup_names: HashSet<&str> = HashSet::new();
    for d in &defs {
        if !d.primary {
            dup_names.insert(d.name.as_str());
        }
    }

    let mut nodes: Vec<MapNode> = Vec::new();
    let mut unreachable_defs: Vec<usize> = Vec::new();
    for r in &regions {
        let d = &defs[r.def];
        let is_reach = reachable.contains(d.name.as_str());
        if !is_reach {
            unreachable_defs.push(r.def);
        }
        nodes.push(MapNode {
            id: d.name.clone(),
            kind: d.kind,
            file: d.file as i32,
            line: d.line,
            end_line: r.end_line.max(d.line),
            stmts: r.stmts,
            says: r.says,
            menus: r.menus,
            choices: r.choices,
            in_degree: in_deg.get(d.name.as_str()).copied().unwrap_or(0),
            out_degree: out_deg.get(d.name.as_str()).copied().unwrap_or(0),
            reachable: is_reach,
            root: roots.contains(d.name.as_str()),
            indirect: indirect.contains(d.name.as_str()),
            duplicate: dup_names.contains(d.name.as_str()),
            returns: r.returns,
            ends_script: r.ends,
            dynamic_out: r.dynamic,
        });
    }
    // Screen nodes.
    for ((id, start), name) in screen_ids.iter().zip(&screen_names) {
        let info = &screens.by_name[name.as_str()];
        nodes.push(MapNode {
            id: id.clone(),
            kind: "screen",
            file: info.file as i32,
            line: info.line,
            end_line: info.end_line,
            stmts: 0,
            says: 0,
            menus: 0,
            choices: 0,
            in_degree: in_deg.get(id.as_str()).copied().unwrap_or(0),
            out_degree: out_deg.get(id.as_str()).copied().unwrap_or(0),
            reachable: reachable.contains(id.as_str()),
            root: *start,
            indirect: false,
            duplicate: false,
            returns: false,
            ends_script: false,
            dynamic_out: 0,
        });
    }

    // Engine-only labels: compiled scripts (or files outside our scan).
    let file_index: HashMap<&str, usize> = files
        .iter()
        .enumerate()
        .map(|(i, f)| (f.rel.as_str(), i))
        .collect();
    let mut engine_unparsed: Vec<(String, usize, u32)> = Vec::new();
    for (name, (path, line)) in &engine_only {
        let (file, ln) = match file_index.get(path.as_str()) {
            Some(&fi) => {
                engine_unparsed.push(((*name).clone(), fi, *line));
                (fi as i32, *line)
            }
            None => (-1, 0),
        };
        nodes.push(MapNode {
            id: (*name).clone(),
            kind: "compiled",
            file,
            line: ln,
            end_line: ln,
            stmts: 0,
            says: 0,
            menus: 0,
            choices: 0,
            in_degree: in_deg.get(name.as_str()).copied().unwrap_or(0),
            out_degree: 0,
            reachable: reachable.contains(name.as_str()),
            root: ROOT_LABELS.contains(&name.as_str()),
            indirect: false,
            duplicate: false,
            returns: false,
            ends_script: false,
            dynamic_out: 0,
        });
    }

    let mut missing_sorted: Vec<&String> = missing_names.iter().collect();
    missing_sorted.sort();
    for name in missing_sorted {
        nodes.push(MapNode {
            id: name.clone(),
            kind: "missing",
            file: -1,
            line: 0,
            end_line: 0,
            stmts: 0,
            says: 0,
            menus: 0,
            choices: 0,
            in_degree: in_deg.get(name.as_str()).copied().unwrap_or(0),
            out_degree: 0,
            reachable: false,
            root: false,
            indirect: false,
            duplicate: false,
            returns: false,
            ends_script: false,
            dynamic_out: 0,
        });
    }
    let edges: Vec<MapEdge> = agg
        .into_iter()
        .map(|((from, to, kind), acc)| MapEdge {
            from,
            to,
            kind,
            count: acc.count,
            badge: acc.badge,
        })
        .collect();

    // Compare the parser with the engine (only meaningful when it reported labels).
    let engine_diff = match ext.engine {
        Some(dump) if !dump.labels.is_empty() => {
            let mut diff = EngineDiff::default();
            for d in defs.iter().filter(|d| d.primary) {
                if d.name.starts_with('_') {
                    continue;
                }
                let rel = files[d.file].rel.clone();
                match dump.labels.get(&d.name) {
                    None => diff.parser_only.push((d.name.clone(), rel, d.line)),
                    Some((p, l)) if *p != rel || *l != d.line => {
                        diff.moved
                            .push((d.name.clone(), (rel, d.line), (p.clone(), *l)))
                    }
                    _ => {}
                }
            }
            for (name, fi, line) in &engine_unparsed {
                diff.engine_only
                    .push((name.clone(), files[*fi].rel.clone(), *line));
            }
            Some(diff)
        }
        _ => None,
    };

    let mut diagnostics = diagnostics::build(
        files,
        &defs,
        &missing,
        &unreachable_defs,
        &known,
        auto_images,
        ext,
        engine_diff.as_ref(),
    );
    let catalog = crate::catalog::build(files);
    diagnostics.extend_sorted(catalog.diags.clone());
    diagnostics.extend_sorted(crate::catalog::flow_diags(&nodes, files));

    let stats = Stats {
        files: files.len() as u32,
        lines: files.iter().map(|f| f.total_lines as u64).sum(),
        labels: defs
            .iter()
            .filter(|d| d.primary && d.kind == "label")
            .count() as u32,
        menus: defs
            .iter()
            .filter(|d| d.primary && d.kind == "menu")
            .count() as u32,
        says: regions.iter().map(|r| r.says as u64).sum(),
        edges: edges.len() as u32,
        opaque: files.iter().map(|f| f.opaque).sum(),
        dynamic_jumps: regions.iter().map(|r| r.dynamic).sum(),
        missing_targets: missing_names.len() as u32,
        unreachable: unreachable_defs.len() as u32,
        duplicates: defs.iter().filter(|d| !d.primary).count() as u32,
        compiled_labels: engine_only.len() as u32,
        screens: screen_names.len() as u32,
    };

    Analysis {
        defs,
        map: ProjectMap { nodes, edges },
        diagnostics,
        stats,
        engine_diff,
        screens,
        catalog,
        by_name: seen,
    }
}

struct Primary {
    def: usize,
    file: usize,
    line: u32,
    end_line: u32,
}

fn build_region(files: &[SourceFile], screens: &ScreenTable, primary: &Primary) -> Region {
    let mut graph = None;
    if let Some(file) = files.get(primary.file) {
        visit_labels(&file.stmts, None, &mut |stmt, cont| {
            if graph.is_none() && stmt.line == primary.line && stmt.is_label_like() {
                graph = Some(crate::flow::outline_graph(stmt, cont, screens));
            }
        });
    }
    let Some(g) = graph else {
        return Region {
            def: primary.def,
            edges: Vec::new(),
            dynamic: 0,
            returns: false,
            ends: false,
            stmts: 0,
            says: 0,
            menus: 0,
            choices: 0,
            end_line: primary.end_line,
        };
    };
    let mut r = Region {
        def: primary.def,
        edges: Vec::new(),
        dynamic: 0,
        returns: false,
        ends: false,
        stmts: 0,
        says: 0,
        menus: 0,
        choices: 0,
        end_line: primary.end_line,
    };
    for n in &g.nodes {
        r.stmts += n.stmts;
        r.says += n.says;
        match n.kind {
            GKind::Jump => match (&n.target, n.dynamic) {
                (Some(t), false) => r.edges.push(RegionEdge {
                    target: t.clone(),
                    kind: if n.choice_label.is_some() {
                        "choice"
                    } else {
                        "jump"
                    },
                    line: n.line,
                    caption: n.choice_label.clone(),
                    cond: n.choice_cond.clone(),
                }),
                _ => r.dynamic += 1,
            },
            GKind::Call => match (&n.target, n.dynamic) {
                (Some(t), false) => r.edges.push(RegionEdge {
                    target: t.clone(),
                    kind: "call",
                    line: n.line,
                    caption: n.choice_label.clone(),
                    cond: n.choice_cond.clone(),
                }),
                _ => r.dynamic += 1,
            },
            GKind::Screen => {
                if let Some(t) = &n.target {
                    r.edges.push(RegionEdge {
                        target: format!("screen:{t}"),
                        kind: "screen",
                        line: n.line,
                        caption: None,
                        cond: None,
                    });
                }
            }
            GKind::Fall => {
                if let Some(t) = &n.target {
                    r.edges.push(RegionEdge {
                        target: t.clone(),
                        kind: "fall",
                        line: n.line,
                        caption: None,
                        cond: None,
                    });
                }
            }
            GKind::Return => r.returns = true,
            GKind::End => r.ends = true,
            GKind::Menu => {
                r.menus += 1;
                r.choices += n.choices;
            }
            _ => {}
        }
    }
    r
}
