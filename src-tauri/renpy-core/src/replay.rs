//! Path from `label start` to a source line, for replay.
//!
//! Replay runs the real script, so the plan only chooses menu answers. An `if`
//! is not controllable: the plan records which branch it needs, and the game
//! may leave the path if the saved state disagrees.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};
use std::sync::Mutex;

use serde::Serialize;

use crate::ast::*;
use crate::project::{Origin, Project};

const MAX_CALLS: usize = 8;
const MAX_STATES: usize = 200_000;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MenuDecision {
    pub file: String,
    pub menu_line: u32,
    pub index: u32,
    pub caption: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReplayPlan {
    /// Script the user clicked, relative to `game/`.
    pub file: String,
    /// Line the user clicked.
    pub line: u32,
    /// Statement Ren'Py will actually execute. Its `linenumber`.
    pub stop_line: u32,
    /// Label that contains the stop, when there is one.
    pub label: Option<String>,
    /// Line numbers in a decompiled file do not match the game.
    pub decompiled: bool,
    pub entry: String,
    pub decisions: Vec<MenuDecision>,
    /// `if` / loop conditions this path needs. The game decides them.
    pub assumptions: Vec<String>,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Cost {
    assumes: u32,
    menus: u32,
    len: u32,
}

impl Cost {
    fn zero() -> Self {
        Self {
            assumes: 0,
            menus: 0,
            len: 0,
        }
    }

    fn step(self, assumes: u32, menus: u32) -> Self {
        Self {
            assumes: self.assumes.saturating_add(assumes),
            menus: self.menus.saturating_add(menus),
            len: self.len.saturating_add(1),
        }
    }
}

struct ChoiceEdge {
    index: u32,
    caption: String,
    next: Option<u32>,
}

struct BranchEdge {
    cond: String,
    next: Option<u32>,
    line: u32,
    /// `if`, `elif`, `else`, `fall`, `loop` or `skip`.
    how: &'static str,
}

enum NKind {
    Next {
        next: Option<u32>,
    },
    Menu {
        choices: Vec<ChoiceEdge>,
    },
    If {
        branches: Vec<BranchEdge>,
    },
    Jump {
        target: String,
        dynamic: bool,
    },
    Call {
        target: String,
        dynamic: bool,
        ret: Option<u32>,
    },
    Return,
}

struct Node {
    file: String,
    line: u32,
    end_line: u32,
    label: String,
    /// Set when this statement is not part of the story (init, screen, define).
    skip: Option<&'static str>,
    kind: NKind,
}

struct Graph {
    nodes: Vec<Node>,
    labels: HashMap<String, u32>,
}

/// `None` when the statement runs as part of the story.
fn skip_reason(stmt: &Stmt) -> Option<&'static str> {
    match &stmt.kind {
        Kind::Define { .. } => Some("a define"),
        Kind::Image { .. } => Some("an image"),
        Kind::Transform { .. } => Some("a transform"),
        Kind::Screen { .. } => Some("a screen"),
        Kind::Python { text, .. } if text.starts_with("init") => Some("an init python block"),
        Kind::Opaque { kind } => match kind.as_str() {
            "init" => Some("an init block"),
            "translate" => Some("a translate block"),
            "screen" => Some("a screen"),
            "transform" => Some("a transform"),
            "style" => Some("a style"),
            "testcase" => Some("a testcase"),
            "layeredimage" => Some("a layered image"),
            "rpy" => Some("an rpy header"),
            _ => None,
        },
        _ => None,
    }
}

impl Graph {
    fn push(&mut self, file: &str, stmt: &Stmt, label: &str, kind: NKind) -> u32 {
        let id = self.nodes.len() as u32;
        self.nodes.push(Node {
            file: file.to_string(),
            line: stmt.line,
            end_line: stmt.end_line,
            label: label.to_string(),
            skip: skip_reason(stmt),
            kind,
        });
        id
    }

    fn lower_list(
        &mut self,
        file: &str,
        stmts: &[Stmt],
        mut cont: Option<u32>,
        label: &str,
    ) -> Option<u32> {
        for stmt in stmts.iter().rev() {
            cont = Some(self.lower_stmt(file, stmt, cont, label));
        }
        cont
    }

    fn lower_stmt(&mut self, file: &str, stmt: &Stmt, cont: Option<u32>, label: &str) -> u32 {
        match &stmt.kind {
            Kind::Label { name, body } => {
                let body_entry = self.lower_list(file, body, cont, name);
                let id = self.push(file, stmt, name, NKind::Next { next: body_entry });
                self.labels.entry(name.clone()).or_insert(id);
                id
            }
            Kind::Menu { name, choices, .. } => {
                let menu_label = name.clone().unwrap_or_else(|| label.to_string());
                let mut edges = Vec::with_capacity(choices.len());
                for (i, choice) in choices.iter().enumerate() {
                    let next = self.lower_list(file, &choice.body, cont, &menu_label);
                    edges.push(ChoiceEdge {
                        index: i as u32,
                        caption: choice.text.clone(),
                        next,
                    });
                }
                let id = self.push(file, stmt, &menu_label, NKind::Menu { choices: edges });
                if let Some(name) = name {
                    self.labels.entry(name.clone()).or_insert(id);
                }
                id
            }
            Kind::If { branches } => {
                let has_else = branches.iter().any(|b| b.kind == BranchKind::Else);
                let mut edges = Vec::with_capacity(branches.len() + 1);
                for branch in branches {
                    let how = match branch.kind {
                        BranchKind::If => "if",
                        BranchKind::Elif => "elif",
                        BranchKind::Else => "else",
                    };
                    let next = self.lower_list(file, &branch.body, cont, label);
                    edges.push(BranchEdge {
                        cond: branch.cond.clone(),
                        next,
                        line: branch.line,
                        how,
                    });
                }
                if !has_else {
                    edges.push(BranchEdge {
                        cond: String::new(),
                        next: cont,
                        line: stmt.line,
                        how: "fall",
                    });
                }
                self.push(file, stmt, label, NKind::If { branches: edges })
            }
            Kind::While { cond, body } => {
                let id = self.push(
                    file,
                    stmt,
                    label,
                    NKind::If {
                        branches: vec![
                            BranchEdge {
                                cond: cond.clone(),
                                next: None,
                                line: stmt.line,
                                how: "loop",
                            },
                            BranchEdge {
                                cond: cond.clone(),
                                next: cont,
                                line: stmt.line,
                                how: "skip",
                            },
                        ],
                    },
                );
                let body_entry = self.lower_list(file, body, Some(id), label);
                if let NKind::If { branches } = &mut self.nodes[id as usize].kind {
                    if let Some(loop_edge) = branches.first_mut() {
                        loop_edge.next = body_entry;
                    }
                }
                id
            }
            Kind::Jump { target, dynamic } => self.push(
                file,
                stmt,
                label,
                NKind::Jump {
                    target: target.clone(),
                    dynamic: *dynamic,
                },
            ),
            Kind::Call {
                target, dynamic, ..
            } => self.push(
                file,
                stmt,
                label,
                NKind::Call {
                    target: target.clone(),
                    dynamic: *dynamic,
                    ret: cont,
                },
            ),
            Kind::Return => self.push(file, stmt, label, NKind::Return),
            Kind::Python {
                block: false, refs, ..
            } => {
                if let Some(jump) = refs.iter().find(|r| r.kind == RefKind::RenpyJump) {
                    return self.push(
                        file,
                        stmt,
                        label,
                        NKind::Jump {
                            target: jump.name.clone().unwrap_or_default(),
                            dynamic: jump.name.is_none(),
                        },
                    );
                }
                if let Some(call) = refs.iter().find(|r| r.kind == RefKind::RenpyCall) {
                    return self.push(
                        file,
                        stmt,
                        label,
                        NKind::Call {
                            target: call.name.clone().unwrap_or_default(),
                            dynamic: call.name.is_none(),
                            ret: cont,
                        },
                    );
                }
                self.push(file, stmt, label, NKind::Next { next: cont })
            }
            _ => self.push(file, stmt, label, NKind::Next { next: cont }),
        }
    }
}

fn build(project: &Project) -> Graph {
    let mut graph = Graph {
        nodes: Vec::new(),
        labels: HashMap::new(),
    };
    for file in &project.files {
        graph.lower_list(&file.rel, &file.stmts, None, "");
    }
    graph
}

fn norm_rel(file: &str) -> String {
    let s = file.replace('\\', "/");
    if let Some(i) = s.rfind("/game/") {
        return s[i + "/game/".len()..].to_string();
    }
    s.strip_prefix("game/").unwrap_or(&s).to_string()
}

pub struct Prepared {
    graph: Graph,
    /// Node ids in one file, sorted by source line.
    by_file: HashMap<String, Vec<usize>>,
    /// Dijkstra frontier from `label start`. Later lines continue it.
    search: Mutex<SearchState>,
}

pub fn prepare(project: &Project) -> Prepared {
    let graph = build(project);
    let mut by_file: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, node) in graph.nodes.iter().enumerate() {
        by_file.entry(node.file.clone()).or_default().push(i);
    }
    for ids in by_file.values_mut() {
        ids.sort_by_key(|&i| (graph.nodes[i].line, graph.nodes[i].end_line));
    }
    Prepared {
        graph,
        by_file,
        search: Mutex::new(SearchState::new()),
    }
}

fn covering(graph: &Graph, ids: &[usize], line: u32) -> Option<usize> {
    ids.iter()
        .copied()
        .filter(|&i| {
            let n = &graph.nodes[i];
            n.line <= line && line <= n.end_line
        })
        .min_by(|&a, &b| {
            let na = &graph.nodes[a];
            let nb = &graph.nodes[b];
            (na.end_line - na.line)
                .cmp(&(nb.end_line - nb.line))
                .then(nb.line.cmp(&na.line))
        })
}

fn snap(graph: &Graph, ids: &[usize], line: u32) -> Option<usize> {
    let from = ids.partition_point(|&i| graph.nodes[i].line < line);
    ids[from..]
        .iter()
        .copied()
        .find(|&i| graph.nodes[i].skip.is_none())
        .or_else(|| {
            ids[..from]
                .iter()
                .rev()
                .copied()
                .find(|&i| graph.nodes[i].skip.is_none())
        })
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Key {
    node: u32,
    depth: u8,
    stack: [u32; MAX_CALLS],
}

/// How the walk arrived. The menu caption and the assumption text are built
/// only for the winning path, from the graph.
#[derive(Clone, Copy)]
enum Step {
    Menu { index: u32 },
    Assume { branch: u16 },
}

struct Walk {
    node: u32,
    stack: [u32; MAX_CALLS],
    depth: u8,
    prev: Option<u32>,
    step: Option<Step>,
}

struct Item {
    cost: Cost,
    index: u32,
}

/// One Dijkstra run from `label start`, kept so the next line can continue.
struct SearchState {
    start: Option<u32>,
    walks: Vec<Walk>,
    /// Best cost at depth 0, indexed by node. The call stack is empty there.
    best_root: Vec<Option<Cost>>,
    best_deep: HashMap<Key, Cost>,
    /// Walk index of the first time this node was popped. That is its cheapest path.
    settled: Vec<Option<u32>>,
    heap: BinaryHeap<Item>,
    saw_dynamic: bool,
}

impl SearchState {
    fn new() -> Self {
        Self {
            start: None,
            walks: Vec::new(),
            best_root: Vec::new(),
            best_deep: HashMap::new(),
            settled: Vec::new(),
            heap: BinaryHeap::new(),
            saw_dynamic: false,
        }
    }
}

impl PartialEq for Item {
    fn eq(&self, other: &Self) -> bool {
        self.cost == other.cost && self.index == other.index
    }
}
impl Eq for Item {}
impl PartialOrd for Item {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Item {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .cost
            .cmp(&self.cost)
            .then(self.index.cmp(&other.index))
    }
}

fn assume_text(file: &str, edge: &BranchEdge) -> String {
    match edge.how {
        "else" => format!("{file}:{} assumes the else branch", edge.line),
        "fall" => format!("{file}:{} assumes none of the conditions match", edge.line),
        "loop" => format!(
            "{file}:{} assumes the loop runs (`{}`)",
            edge.line, edge.cond
        ),
        "skip" => format!(
            "{file}:{} assumes the loop is skipped (`{}`)",
            edge.line, edge.cond
        ),
        _ => format!("{file}:{} assumes `{}`", edge.line, edge.cond),
    }
}

struct Found {
    decisions: Vec<MenuDecision>,
    assumptions: Vec<String>,
    /// Node ids from `start` through the target, in visit order.
    path: Vec<u32>,
}

fn seed(state: &mut SearchState, nodes: usize, start: u32) {
    if state.start == Some(start) && state.settled.len() == nodes {
        return;
    }
    let empty = [0u32; MAX_CALLS];
    let mut best_root = vec![None; nodes];
    let mut settled = vec![None; nodes];
    if (start as usize) < nodes {
        best_root[start as usize] = Some(Cost::zero());
        settled[start as usize] = Some(0);
    }
    let mut heap = BinaryHeap::new();
    heap.push(Item {
        cost: Cost::zero(),
        index: 0,
    });
    *state = SearchState {
        start: Some(start),
        walks: vec![Walk {
            node: start,
            stack: empty,
            depth: 0,
            prev: None,
            step: None,
        }],
        best_root,
        best_deep: HashMap::new(),
        settled,
        heap,
        saw_dynamic: false,
    };
}

fn search(state: &mut SearchState, graph: &Graph, start: u32, target: u32) -> Result<Found, bool> {
    seed(state, graph.nodes.len(), start);
    if let Some(idx) = state.settled.get(target as usize).and_then(|slot| *slot) {
        return Ok(rebuild(graph, &state.walks, idx));
    }
    let limit = state.walks.len().saturating_add(MAX_STATES);
    while let Some(item) = state.heap.pop() {
        let walk_i = item.index as usize;
        if walk_i >= state.walks.len() {
            continue;
        }
        let node_id = state.walks[walk_i].node;
        let depth = state.walks[walk_i].depth;
        let stack = state.walks[walk_i].stack;
        if is_stale(state, node_id, depth, stack, item.cost) {
            continue;
        }
        if state
            .settled
            .get(node_id as usize)
            .and_then(|slot| *slot)
            .is_none()
        {
            if let Some(slot) = state.settled.get_mut(node_id as usize) {
                *slot = Some(item.index);
            }
        }
        let index = item.index;
        let expanded = if state.walks.len() < limit {
            expand(state, graph, node_id, depth, stack, item.cost, index, limit)
        } else {
            false
        };
        if !expanded {
            state.heap.push(item);
        }
        if node_id == target {
            return Ok(rebuild(graph, &state.walks, index));
        }
        if !expanded {
            break;
        }
    }
    Err(state.saw_dynamic)
}

fn is_stale(
    state: &SearchState,
    node: u32,
    depth: u8,
    stack: [u32; MAX_CALLS],
    cost: Cost,
) -> bool {
    if depth == 0 {
        return state
            .best_root
            .get(node as usize)
            .and_then(|c| *c)
            .is_some_and(|old| cost > old);
    }
    let key = Key { node, depth, stack };
    state.best_deep.get(&key).is_some_and(|old| cost > *old)
}

/// `false` when this query's walk budget ran out before every edge was pushed.
fn expand(
    state: &mut SearchState,
    graph: &Graph,
    node_id: u32,
    depth: u8,
    stack: [u32; MAX_CALLS],
    cost: Cost,
    prev: u32,
    limit: usize,
) -> bool {
    let tag = match &graph.nodes[node_id as usize].kind {
        NKind::Next { .. } => 0u8,
        NKind::Menu { .. } => 1,
        NKind::If { .. } => 2,
        NKind::Jump { .. } => 3,
        NKind::Call { .. } => 4,
        NKind::Return => 5,
    };
    match tag {
        0 => {
            let next = match &graph.nodes[node_id as usize].kind {
                NKind::Next { next } => *next,
                _ => None,
            };
            let Some(next) = next else { return true };
            push_state(
                state,
                next,
                stack,
                depth,
                cost.step(0, 0),
                prev,
                None,
                limit,
            )
        }
        1 => {
            let n = match &graph.nodes[node_id as usize].kind {
                NKind::Menu { choices } => choices.len(),
                _ => 0,
            };
            for i in 0..n {
                let (next, index) = match &graph.nodes[node_id as usize].kind {
                    NKind::Menu { choices } => (choices[i].next, choices[i].index),
                    _ => (None, 0),
                };
                let Some(next) = next else { continue };
                if !push_state(
                    state,
                    next,
                    stack,
                    depth,
                    cost.step(0, 1),
                    prev,
                    Some(Step::Menu { index }),
                    limit,
                ) {
                    return false;
                }
            }
            true
        }
        2 => {
            let n = match &graph.nodes[node_id as usize].kind {
                NKind::If { branches } => branches.len(),
                _ => 0,
            };
            let many = n > 1;
            for i in 0..n {
                let next = match &graph.nodes[node_id as usize].kind {
                    NKind::If { branches } => branches[i].next,
                    _ => None,
                };
                let Some(next) = next else { continue };
                let step = if many {
                    Some(Step::Assume { branch: i as u16 })
                } else {
                    None
                };
                let assumes = if many { 1 } else { 0 };
                if !push_state(
                    state,
                    next,
                    stack,
                    depth,
                    cost.step(assumes, 0),
                    prev,
                    step,
                    limit,
                ) {
                    return false;
                }
            }
            true
        }
        3 => {
            let (dynamic, name) = match &graph.nodes[node_id as usize].kind {
                NKind::Jump { target, dynamic } => (*dynamic || target.is_empty(), target.clone()),
                _ => (false, String::new()),
            };
            if dynamic {
                state.saw_dynamic = true;
                return true;
            }
            let Some(id) = graph.labels.get(&name).copied() else {
                return true;
            };
            push_state(state, id, stack, depth, cost.step(0, 0), prev, None, limit)
        }
        4 => {
            let (dynamic, name, ret, too_deep) = match &graph.nodes[node_id as usize].kind {
                NKind::Call {
                    target,
                    dynamic,
                    ret,
                } => (
                    *dynamic || target.is_empty(),
                    target.clone(),
                    *ret,
                    depth as usize >= MAX_CALLS,
                ),
                _ => (false, String::new(), None, false),
            };
            if dynamic || too_deep {
                state.saw_dynamic = true;
                return true;
            }
            let Some(id) = graph.labels.get(&name).copied() else {
                return true;
            };
            let mut next_stack = stack;
            let mut next_depth = depth;
            if let Some(ret) = ret {
                next_stack[depth as usize] = ret;
                next_depth = depth + 1;
            }
            push_state(
                state,
                id,
                next_stack,
                next_depth,
                cost.step(0, 0),
                prev,
                None,
                limit,
            )
        }
        _ => {
            if depth == 0 {
                return true;
            }
            let next_depth = depth - 1;
            let mut next_stack = stack;
            let ret = stack[next_depth as usize];
            next_stack[next_depth as usize] = 0;
            push_state(
                state,
                ret,
                next_stack,
                next_depth,
                cost.step(0, 0),
                prev,
                None,
                limit,
            )
        }
    }
}

fn push_state(
    state: &mut SearchState,
    node: u32,
    stack: [u32; MAX_CALLS],
    depth: u8,
    cost: Cost,
    prev: u32,
    step: Option<Step>,
    limit: usize,
) -> bool {
    if state.walks.len() >= limit {
        return false;
    }
    if depth == 0 {
        let Some(slot) = state.best_root.get_mut(node as usize) else {
            return true;
        };
        if slot.is_some_and(|old| old <= cost) {
            return true;
        }
        *slot = Some(cost);
    } else {
        let key = Key { node, depth, stack };
        if state.best_deep.get(&key).is_some_and(|old| *old <= cost) {
            return true;
        }
        state.best_deep.insert(key, cost);
    }
    let index = state.walks.len() as u32;
    state.walks.push(Walk {
        node,
        stack,
        depth,
        prev: Some(prev),
        step,
    });
    state.heap.push(Item { cost, index });
    true
}

fn rebuild(graph: &Graph, walks: &[Walk], mut idx: u32) -> Found {
    let mut decisions = Vec::new();
    let mut assumptions = Vec::new();
    let mut path = Vec::new();
    let mut guard = 0u32;
    loop {
        let node = walks[idx as usize].node;
        let step = walks[idx as usize].step;
        let prev = walks[idx as usize].prev;
        path.push(node);
        if let (Some(step), Some(prev)) = (step, prev) {
            let src = walks[prev as usize].node as usize;
            match step {
                Step::Menu { index } => {
                    if let NKind::Menu { choices } = &graph.nodes[src].kind {
                        if let Some(choice) = choices.iter().find(|c| c.index == index) {
                            decisions.push(MenuDecision {
                                file: graph.nodes[src].file.clone(),
                                menu_line: graph.nodes[src].line,
                                index: choice.index,
                                caption: choice.caption.clone(),
                            });
                        }
                    }
                }
                Step::Assume { branch } => {
                    if let NKind::If { branches } = &graph.nodes[src].kind {
                        if let Some(edge) = branches.get(branch as usize) {
                            assumptions.push(assume_text(&graph.nodes[src].file, edge));
                        }
                    }
                }
            }
        }
        match prev {
            Some(prev) => idx = prev,
            None => break,
        }
        guard += 1;
        if guard > walks.len() as u32 {
            break;
        }
    }
    path.reverse();
    decisions.reverse();
    assumptions.reverse();
    Found {
        decisions,
        assumptions,
        path,
    }
}

fn unreachable(file: &str, line: u32, saw_dynamic: bool) -> String {
    let mut msg = format!("No path from label start reaches {file}:{line}.");
    if saw_dynamic {
        msg.push_str(" A jump or call on the way is computed in code, which replay cannot follow.");
    } else {
        msg.push_str(" It may only be reached from a screen, or not at all.");
    }
    msg.push_str(" Use Quick jump instead. It skips earlier statements, so a name that was never set is treated as false.");
    msg
}

struct Located<'a> {
    graph: &'a Graph,
    start: u32,
    target: u32,
    file: String,
    line: u32,
    stop_line: u32,
    label: Option<String>,
    decompiled: bool,
}

fn locate<'a>(
    prepared: &'a Prepared,
    project: &Project,
    file: &str,
    line: u32,
) -> Result<Located<'a>, String> {
    let file = norm_rel(file);
    if line == 0 || file.is_empty() {
        return Err("Pick a line in the script first.".into());
    }
    if project.files.iter().all(|f| f.rel != file) {
        return Err(format!("`{file}` is not a script of this project."));
    }
    let graph = &prepared.graph;
    let Some(&start) = graph.labels.get("start") else {
        return Err("This project has no start label, so replay cannot begin.".into());
    };
    let ids = prepared
        .by_file
        .get(&file)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let hit = covering(graph, ids, line)
        .or_else(|| snap(graph, ids, line))
        .ok_or_else(|| format!("Nothing in `{file}` can be replayed to line {line}."))?;
    let node = &graph.nodes[hit];
    if let Some(why) = node.skip {
        return Err(format!(
            "Line {line} of `{file}` is inside {why}, not the story. Replay runs from label start. Use Quick jump instead."
        ));
    }
    let decompiled = project
        .files
        .iter()
        .any(|f| f.rel == file && matches!(f.origin, Origin::Compiled { .. }));
    let (target, label) = if decompiled {
        if node.label.is_empty() {
            return Err(format!(
                "`{file}` was decompiled from a .rpyc, so its line numbers do not match the game, and no label covers line {line}."
            ));
        }
        let id = graph.labels.get(&node.label).copied().unwrap_or(hit as u32);
        (id, Some(node.label.clone()))
    } else {
        let label = if node.label.is_empty() {
            None
        } else {
            Some(node.label.clone())
        };
        (hit as u32, label)
    };
    let stop_line = graph.nodes[target as usize].line;
    Ok(Located {
        graph,
        start,
        target,
        file,
        line,
        stop_line,
        label,
        decompiled,
    })
}

/// Plan a replay from `start` to the statement covering `line` in `file`.
pub fn plan(project: &Project, file: &str, line: u32) -> Result<ReplayPlan, String> {
    let prepared = prepare(project);
    let located = locate(&prepared, project, file, line)?;
    let found = {
        let mut state = prepared.search.lock().unwrap_or_else(|e| e.into_inner());
        match search(&mut state, located.graph, located.start, located.target) {
            Ok(found) => found,
            Err(saw_dynamic) => return Err(unreachable(&located.file, located.line, saw_dynamic)),
        }
    };
    Ok(ReplayPlan {
        file: located.file,
        line: located.line,
        stop_line: located.stop_line,
        label: located.label,
        decompiled: located.decompiled,
        entry: "start".into(),
        decisions: found.decisions,
        assumptions: found.assumptions,
    })
}

/// The statements visited on the cheapest path from `label start` to `line`.
///
/// Each step is `(file relative to game/, statement line)`. `plan` stays the
/// menu-choice list; this is the same search with the statements kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoryPath {
    pub steps: Vec<(String, u32)>,
    pub decisions: Vec<MenuDecision>,
    pub assumptions: Vec<String>,
}

/// Walks the reused search has stored. A query for a line already settled
/// does not add any.
pub(crate) fn stored_walks(prepared: &Prepared) -> usize {
    prepared
        .search
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .walks
        .len()
}

pub fn path_to(project: &Project, file: &str, line: u32) -> Result<StoryPath, String> {
    path_in(&prepare(project), project, file, line)
}

pub fn path_in(
    prepared: &Prepared,
    project: &Project,
    file: &str,
    line: u32,
) -> Result<StoryPath, String> {
    let located = locate(prepared, project, file, line)?;
    let found = {
        let mut state = prepared.search.lock().unwrap_or_else(|e| e.into_inner());
        match search(&mut state, located.graph, located.start, located.target) {
            Ok(found) => found,
            Err(saw_dynamic) => return Err(unreachable(&located.file, located.line, saw_dynamic)),
        }
    };
    let steps = found
        .path
        .iter()
        .map(|id| {
            let node = &located.graph.nodes[*id as usize];
            (node.file.clone(), node.line)
        })
        .collect();
    Ok(StoryPath {
        steps,
        decisions: found.decisions,
        assumptions: found.assumptions,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn scratch(files: &[(&str, &str)]) -> (PathBuf, Project) {
        static N: AtomicU64 = AtomicU64::new(1);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("vnide-replay-{}-{n}", std::process::id()));
        let game = root.join("game");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&game).unwrap();
        for (rel, text) in files {
            let path = game.join(rel);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(path, text).unwrap();
        }
        let project = Project::open(&root).unwrap();
        (root, project)
    }

    #[test]
    fn menu_path_picks_the_branch_that_reaches_the_line() {
        let (root, project) = scratch(&[(
            "script.rpy",
            "\
label start:
    menu:
        \"left\":
            jump finale
        \"right\":
            \"nope\"
    \"after\"

label finale:
    \"done\"
",
        )]);
        let done = plan(&project, "script.rpy", 10).unwrap();
        assert_eq!(done.stop_line, 10);
        assert_eq!(done.decisions.len(), 1);
        assert_eq!(done.decisions[0].index, 0);
        assert_eq!(done.decisions[0].caption, "left");
        assert_eq!(done.decisions[0].menu_line, 2);

        let after = plan(&project, "script.rpy", 7).unwrap();
        assert_eq!(after.decisions[0].index, 1);
        assert_eq!(after.decisions[0].caption, "right");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn call_and_return_reaches_inside_and_after() {
        let (root, project) = scratch(&[(
            "script.rpy",
            "\
label start:
    call helper
    \"back\"

label helper:
    \"inside\"
    return
",
        )]);
        let inside = plan(&project, "script.rpy", 6).unwrap();
        assert!(inside.decisions.is_empty());
        assert_eq!(inside.label.as_deref(), Some("helper"));
        let back = plan(&project, "script.rpy", 3).unwrap();
        assert_eq!(back.label.as_deref(), Some("start"));
        assert!(back.decisions.is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn jump_crosses_files() {
        let (root, project) = scratch(&[
            ("script.rpy", "label start:\n    jump other\n"),
            ("ch/b.rpy", "label other:\n    \"there\"\n"),
        ]);
        let plan = plan(&project, "ch/b.rpy", 2).unwrap();
        assert_eq!(plan.file, "ch/b.rpy");
        assert!(plan.decisions.is_empty());
        assert_eq!(plan.label.as_deref(), Some("other"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn if_branch_is_recorded_and_not_controllable() {
        let (root, project) = scratch(&[(
            "script.rpy",
            "\
label start:
    if points > 0:
        \"rich\"
    else:
        \"poor\"
    \"end\"
",
        )]);
        let rich = plan(&project, "script.rpy", 3).unwrap();
        assert!(rich.assumptions.iter().any(|a| a.contains("points > 0")));
        assert!(rich.decisions.is_empty());
        let end = plan(&project, "script.rpy", 6).unwrap();
        assert_eq!(end.assumptions.len(), 1);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn fewer_conditions_beats_a_shorter_menu() {
        let (root, project) = scratch(&[(
            "script.rpy",
            "\
label start:
    jump choice

label choice:
    menu:
        \"go\":
            jump land
        \"slow\":
            if flag:
                jump land

label land:
    \"here\"
",
        )]);
        let here = plan(&project, "script.rpy", 12).unwrap();
        assert_eq!(here.decisions.len(), 1);
        assert_eq!(here.decisions[0].caption, "go");
        assert!(here.assumptions.is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn return_blocks_a_later_label() {
        let (root, project) = scratch(&[(
            "script.rpy",
            "\
label start:
    \"only\"
    return

label hidden:
    \"secret\"
",
        )]);
        let err = plan(&project, "script.rpy", 6).unwrap_err();
        assert!(err.contains("No path"), "{err}");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn init_and_python_jump() {
        let (root, project) = scratch(&[(
            "script.rpy",
            "\
init:
    $ x = 1

label start:
    $ renpy.jump(\"land\")

label land:
    \"here\"
",
        )]);
        let err = plan(&project, "script.rpy", 2).unwrap_err();
        assert!(err.contains("init"), "{err}");
        let here = plan(&project, "script.rpy", 8).unwrap();
        assert_eq!(here.label.as_deref(), Some("land"));
        assert!(here.decisions.is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_resumed_search_matches_a_fresh_path() {
        let (root, project) = scratch(&[(
            "script.rpy",
            "\
label start:
    menu:
        \"left\":
            jump finale
        \"right\":
            \"nope\"
    \"after\"

label finale:
    \"done\"
",
        )]);
        let prepared = prepare(&project);
        for line in [10u32, 7, 2] {
            let shared = path_in(&prepared, &project, "script.rpy", line).unwrap();
            let fresh = path_to(&project, "script.rpy", line).unwrap();
            assert_eq!(shared, fresh, "line {line}");
        }
        let _ = fs::remove_dir_all(root);
    }
}
