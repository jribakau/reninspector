//! Control-flow graph of a single label region.
//!
//! A region starts at a `label` (or named `menu`) and runs until control
//! leaves it by `jump`/`return`, or falls into the next label. Fall-through is
//! modelled with a continuation chain: when a block ends, execution continues
//! after its parent statement, so the walk can climb up until it reaches the
//! next label.

use serde::Serialize;

use crate::ast::*;
use crate::screens::ScreenTable;

/// What happens after the end of a statement list.
pub struct Cont<'a> {
    pub rest: &'a [Stmt],
    pub parent: Option<&'a Cont<'a>>,
}

/// Call `f` for every label-like statement together with its continuation.
pub fn visit_labels<'a>(
    stmts: &'a [Stmt],
    parent: Option<&'a Cont<'a>>,
    f: &mut dyn FnMut(&Stmt, &Cont),
) {
    for (i, s) in stmts.iter().enumerate() {
        let after = Cont {
            rest: &stmts[i + 1..],
            parent,
        };
        match &s.kind {
            Kind::Label { body, .. } => {
                f(s, &after);
                visit_labels(body, Some(&after), f);
            }
            Kind::Menu { name, choices, .. } => {
                if name.is_some() {
                    f(s, &after);
                }
                for c in choices {
                    visit_labels(&c.body, Some(&after), f);
                }
            }
            Kind::If { branches } => {
                for b in branches {
                    visit_labels(&b.body, Some(&after), f);
                }
            }
            Kind::While { body, .. } => visit_labels(body, Some(&after), f),
            _ => {}
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum GKind {
    Dialogue,
    Menu,
    /// One menu option. Only produced when the graph is built in detail mode.
    Choice,
    Cond,
    Jump,
    Fall,
    Call,
    Return,
    End,
    /// call screen x / show screen x for a screen that navigates somewhere.
    Screen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum EKind {
    Next,
    Choice,
    Branch,
    /// The statement after a `call`. Decorative: it is not a second trip through the callee.
    Return,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GNode {
    pub id: u32,
    pub kind: GKind,
    pub line: u32,
    pub end_line: u32,
    /// Display text: condition, menu caption, jump target, ...
    pub title: String,
    pub target: Option<String>,
    pub dynamic: bool,
    pub says: u32,
    pub stmts: u32,
    /// Cosmetic conditionals (only show/hide/scene/with) folded into this node.
    pub variants: u32,
    /// Other conditionals that do not change control flow.
    pub conds: u32,
    pub opaque: u32,
    pub python: u32,
    pub choices: u32,
    pub speakers: Vec<String>,
    pub preview: Vec<String>,
    /// Spoken line or choice caption. Empty on overview runs and beat cards.
    pub body: String,
    /// `body` was cut. Writing it back would drop the rest of the line.
    pub clipped: bool,
    /// Line of a choice's jump, when that jump is the whole choice body.
    pub target_line: u32,
    /// For screen nodes: labels the screen can lead to (also drawn as jump-out nodes).
    pub targets: Vec<String>,
    /// Set when this jump or call sits inside a menu choice.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub choice_label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub choice_cond: Option<String>,
    /// Detail mode, beat cards: the staging statements on the card, in order.
    pub beats: Vec<BeatLine>,
}

/// One staging statement (show, scene, play, ...) on a beat card.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BeatLine {
    pub line: u32,
    pub end_line: u32,
    /// First word: `show`, `scene`, `play`, `$`, ...
    pub cmd: String,
    /// The rest of the statement.
    pub text: String,
    /// The flow editor can rewrite this line.
    pub editable: bool,
}

const MAX_BEATS: usize = 12;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GEdge {
    pub from: u32,
    pub to: u32,
    pub kind: EKind,
    pub label: Option<String>,
    pub cond: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LabelGraph {
    pub name: String,
    pub kind: String,
    pub line: u32,
    pub end_line: u32,
    pub root: u32,
    pub nodes: Vec<GNode>,
    pub edges: Vec<GEdge>,
}

struct Open {
    from: u32,
    kind: EKind,
    label: Option<String>,
    cond: Option<String>,
}

const ROOT: u32 = u32::MAX;
const MAX_SCREEN_TARGETS: usize = 12;

struct Builder<'a> {
    screens: &'a ScreenTable,
    nodes: Vec<GNode>,
    edges: Vec<GEdge>,
    lite: bool,
    /// Map pass: keep jump, call, and fall nodes, and skip next-edge records.
    outline: bool,
    /// One node per say, and a node per menu choice. Overview stays folded.
    detail: bool,
    root_stmt: *const Stmt,
    last_run: Option<u32>,
    /// Menu choice the walk is inside: caption and `if` condition.
    choice: Option<(String, Option<String>)>,
}

/// Build the graph for the label-like statement `stmt`.
/// With `lite`, no preview strings are produced (used for the project map).
pub fn build_graph(stmt: &Stmt, cont: &Cont, lite: bool) -> LabelGraph {
    build_graph_with(stmt, cont, lite, &ScreenTable::default(), false)
}

/// Like `build_graph`, with the project's screens so call screen can be shown as navigation.
/// `detail` splits each say and each menu choice into its own node.
pub fn build_graph_with(
    stmt: &Stmt,
    cont: &Cont,
    lite: bool,
    screens: &ScreenTable,
    detail: bool,
) -> LabelGraph {
    walk_graph(stmt, cont, lite, screens, detail, false)
}

/// Lite walk that records jump, call, fall, and screen nodes and skips next edges.
pub fn outline_graph(stmt: &Stmt, cont: &Cont, screens: &ScreenTable) -> LabelGraph {
    walk_graph(stmt, cont, true, screens, false, true)
}

fn walk_graph(
    stmt: &Stmt,
    cont: &Cont,
    lite: bool,
    screens: &ScreenTable,
    detail: bool,
    outline: bool,
) -> LabelGraph {
    let mut b = Builder {
        screens,
        nodes: Vec::new(),
        edges: Vec::new(),
        lite,
        outline,
        detail,
        root_stmt: stmt as *const Stmt,
        last_run: None,
        choice: None,
    };
    let (name, kind): (String, &str) = match &stmt.kind {
        Kind::Label { name, .. } => (name.clone(), "label"),
        Kind::Menu { name, .. } => (name.clone().unwrap_or_default(), "menu"),
        _ => (String::new(), "label"),
    };
    let entry = vec![Open {
        from: ROOT,
        kind: EKind::Next,
        label: None,
        cond: None,
    }];
    let mut open = match &stmt.kind {
        Kind::Label { body, .. } => b.seq(body, entry),
        // A named menu is its own region: walk the menu statement itself.
        _ => b.seq(std::slice::from_ref(stmt), entry),
    };
    let mut c: Option<&Cont> = Some(cont);
    while !open.is_empty() {
        match c {
            Some(cur) => {
                open = b.seq(cur.rest, open);
                c = cur.parent;
            }
            None => break,
        }
    }
    if !open.is_empty() {
        let id = b.new_node(GKind::End, stmt.end_line, "end of script".into());
        b.connect(open, id);
    }
    if b.nodes.is_empty() {
        b.new_node(GKind::End, stmt.line, "empty".into());
    }
    LabelGraph {
        name,
        kind: kind.into(),
        line: stmt.line,
        end_line: stmt.end_line,
        root: 0,
        nodes: b.nodes,
        edges: b.edges,
    }
}

impl Builder<'_> {
    fn new_node(&mut self, kind: GKind, line: u32, title: String) -> u32 {
        let title = if self.outline { String::new() } else { title };
        let id = self.nodes.len() as u32;
        self.nodes.push(GNode {
            id,
            kind,
            line,
            end_line: line,
            title,
            target: None,
            dynamic: false,
            says: 0,
            stmts: 0,
            variants: 0,
            conds: 0,
            opaque: 0,
            python: 0,
            choices: 0,
            speakers: Vec::new(),
            preview: Vec::new(),
            body: String::new(),
            clipped: false,
            target_line: 0,
            targets: Vec::new(),
            beats: Vec::new(),
            choice_label: None,
            choice_cond: None,
        });
        if kind != GKind::Dialogue {
            self.last_run = None;
        }
        id
    }

    fn connect(&mut self, open: Vec<Open>, to: u32) {
        for o in open {
            if o.from == ROOT
                || (self.outline && matches!(o.kind, EKind::Next | EKind::Return))
            {
                continue;
            }
            self.edges.push(GEdge {
                from: o.from,
                to,
                kind: o.kind,
                label: o.label,
                cond: o.cond,
            });
        }
    }

    fn next_from(id: u32) -> Vec<Open> {
        vec![Open {
            from: id,
            kind: EKind::Next,
            label: None,
            cond: None,
        }]
    }

    /// Continuation after `call`. The callee is not inlined; this edge is the return.
    fn return_from(id: u32) -> Vec<Open> {
        vec![Open {
            from: id,
            kind: EKind::Return,
            label: Some("return".into()),
            cond: None,
        }]
    }

    fn note_choice(&mut self, id: u32) {
        if let Some((label, cond)) = &self.choice {
            let n = &mut self.nodes[id as usize];
            n.choice_label = Some(label.clone());
            n.choice_cond = cond.clone();
        }
    }

    fn seq(&mut self, stmts: &[Stmt], mut open: Vec<Open>) -> Vec<Open> {
        for s in stmts {
            if open.is_empty() {
                break;
            }
            match &s.kind {
                Kind::Label { name, .. } => {
                    return self.fall_to(name, s, open);
                }
                Kind::Menu { name: Some(n), .. } if !std::ptr::eq(s, self.root_stmt) => {
                    return self.fall_to(n, s, open);
                }
                Kind::Menu {
                    name,
                    caption,
                    choices,
                } => {
                    let title = caption
                        .clone()
                        .or_else(|| name.clone())
                        .unwrap_or_else(|| "menu".into());
                    let id = self.new_node(GKind::Menu, s.line, title);
                    self.nodes[id as usize].end_line = s.end_line;
                    self.nodes[id as usize].choices = choices.len() as u32;
                    self.connect(open, id);
                    let mut next: Vec<Open> = Vec::new();
                    for c in choices {
                        let saved = self.choice.clone();
                        self.choice = Some((c.text.clone(), c.cond.clone()));
                        if self.detail {
                            next.extend(self.detail_choice(id, c));
                            self.choice = saved;
                            continue;
                        }
                        let entry = vec![Open {
                            from: id,
                            kind: EKind::Choice,
                            label: Some(c.text.clone()),
                            cond: c.cond.clone(),
                        }];
                        next.extend(self.seq(&c.body, entry));
                        self.choice = saved;
                    }
                    if choices.is_empty() {
                        next = Self::next_from(id);
                    }
                    open = next;
                }
                Kind::Jump { target, dynamic } => {
                    let id = self.new_node(GKind::Jump, s.line, target.clone());
                    let n = &mut self.nodes[id as usize];
                    n.target = Some(target.clone());
                    n.dynamic = *dynamic;
                    n.end_line = s.end_line;
                    self.note_choice(id);
                    self.connect(open, id);
                    return Vec::new();
                }
                Kind::Call {
                    target, dynamic, ..
                } => {
                    let id = self.new_node(GKind::Call, s.line, target.clone());
                    let n = &mut self.nodes[id as usize];
                    n.target = Some(target.clone());
                    n.dynamic = *dynamic;
                    n.end_line = s.end_line;
                    self.note_choice(id);
                    self.connect(open, id);
                    open = Self::return_from(id);
                }
                Kind::Return => {
                    let id = self.new_node(GKind::Return, s.line, "return".into());
                    self.connect(open, id);
                    return Vec::new();
                }
                Kind::If { branches } => {
                    if branches.iter().any(|b| contains_flow(&b.body)) {
                        open = self.cond_node(s, branches, open);
                    } else {
                        open = self.add_run(s, open);
                    }
                }
                Kind::While { cond, body } => {
                    if contains_flow(body) {
                        let id = self.new_node(GKind::Cond, s.line, format!("while {cond}"));
                        self.nodes[id as usize].end_line = s.end_line;
                        self.connect(open, id);
                        let entry = vec![Open {
                            from: id,
                            kind: EKind::Branch,
                            label: Some(format!("while {cond}")),
                            cond: None,
                        }];
                        let mut next = self.seq(body, entry);
                        next.push(Open {
                            from: id,
                            kind: EKind::Branch,
                            label: Some("exit loop".into()),
                            cond: None,
                        });
                        open = next;
                    } else {
                        open = self.add_run(s, open);
                    }
                }
                Kind::Python { block, refs, .. } => {
                    let jump = refs.iter().find(|r| r.kind == RefKind::RenpyJump);
                    let call = refs.iter().find(|r| r.kind == RefKind::RenpyCall);
                    if let Some(r) = jump {
                        let title = r.name.clone().unwrap_or_else(|| "(dynamic)".into());
                        let id = self.new_node(GKind::Jump, s.line, title);
                        let n = &mut self.nodes[id as usize];
                        n.target = r.name.clone();
                        n.dynamic = r.name.is_none();
                        n.end_line = s.end_line;
                        self.note_choice(id);
                        self.connect(open, id);
                        if *block {
                            open = Self::next_from(id);
                        } else {
                            return Vec::new();
                        }
                    } else if let Some(name) = refs
                        .iter()
                        .find(|r| r.kind == RefKind::Screen)
                        .and_then(|r| r.name.clone())
                        .filter(|n| self.screens.navigational(n) && call.is_none())
                    {
                        open = self.screen_node(s, "call", &name, open);
                    } else if let Some(r) = call {
                        let title = r.name.clone().unwrap_or_else(|| "(dynamic)".into());
                        let id = self.new_node(GKind::Call, s.line, title);
                        let n = &mut self.nodes[id as usize];
                        n.target = r.name.clone();
                        n.dynamic = r.name.is_none();
                        n.end_line = s.end_line;
                        self.note_choice(id);
                        self.connect(open, id);
                        open = Self::return_from(id);
                    } else {
                        open = self.add_run(s, open);
                    }
                }
                Kind::ScreenRef { how, name } if self.screens.navigational(name) => {
                    open = self.screen_node(s, how, name, open);
                }
                _ => {
                    open = self.add_run(s, open);
                }
            }
        }
        open
    }

    /// A screen that leads to labels: one node, plus a node per action.
    /// `Jump` leaves the screen. `Call` comes back, so that node is a call.
    fn screen_node(&mut self, s: &Stmt, how: &str, name: &str, open: Vec<Open>) -> Vec<Open> {
        let actions = self.screens.reach_actions(name);
        let id = self.new_node(GKind::Screen, s.line, format!("{how} screen {name}"));
        {
            let n = &mut self.nodes[id as usize];
            n.target = Some(name.to_string());
            n.end_line = s.end_line;
        }
        self.connect(open, id);
        if !self.lite {
            let mut targets = Vec::new();
            for action in &actions {
                if !targets.contains(&action.name) {
                    targets.push(action.name.clone());
                }
            }
            self.nodes[id as usize].targets = targets;
            for action in actions.iter().take(MAX_SCREEN_TARGETS) {
                let kind = if action.how == "call" {
                    GKind::Call
                } else {
                    GKind::Jump
                };
                let jid = self.new_node(kind, action.line, action.name.clone());
                self.nodes[jid as usize].target = Some(action.name.clone());
                self.edges.push(GEdge {
                    from: id,
                    to: jid,
                    kind: EKind::Branch,
                    label: action.caption.clone(),
                    cond: action.cond.clone(),
                });
            }
            if actions.len() > MAX_SCREEN_TARGETS {
                let jid = self.new_node(
                    GKind::Jump,
                    s.line,
                    format!("+{} more labels", actions.len() - MAX_SCREEN_TARGETS),
                );
                self.nodes[jid as usize].dynamic = true;
                self.edges.push(GEdge {
                    from: id,
                    to: jid,
                    kind: EKind::Branch,
                    label: None,
                    cond: None,
                });
            }
        }
        if how == "call" {
            Self::return_from(id)
        } else {
            Self::next_from(id)
        }
    }

    fn fall_to(&mut self, name: &str, s: &Stmt, open: Vec<Open>) -> Vec<Open> {
        let id = self.new_node(GKind::Fall, s.line, name.to_string());
        self.nodes[id as usize].target = Some(name.to_string());
        // Falling into the next label is not the return from a call.
        let open = open
            .into_iter()
            .map(|mut o| {
                if o.kind == EKind::Return {
                    o.kind = EKind::Next;
                    o.label = None;
                }
                o
            })
            .collect();
        self.connect(open, id);
        Vec::new()
    }

    fn cond_node(&mut self, s: &Stmt, branches: &[Branch], open: Vec<Open>) -> Vec<Open> {
        let first = branches.first().map(|b| b.cond.clone()).unwrap_or_default();
        let id = self.new_node(GKind::Cond, s.line, first);
        self.nodes[id as usize].end_line = s.end_line;
        self.connect(open, id);
        let mut next: Vec<Open> = Vec::new();
        let mut has_else = false;
        for b in branches {
            let label = match b.kind {
                BranchKind::Else => {
                    has_else = true;
                    "else".to_string()
                }
                BranchKind::If => b.cond.clone(),
                BranchKind::Elif => format!("elif {}", b.cond),
            };
            let entry = vec![Open {
                from: id,
                kind: EKind::Branch,
                label: Some(label),
                cond: None,
            }];
            next.extend(self.seq(&b.body, entry));
        }
        if !has_else {
            next.push(Open {
                from: id,
                kind: EKind::Branch,
                label: Some("else".into()),
                cond: None,
            });
        }
        next
    }

    /// One menu option as its own node. A body that is only `jump label` stays on the card.
    fn detail_choice(&mut self, menu: u32, c: &Choice) -> Vec<Open> {
        let (text, clipped) = clip_text(&c.text);
        let id = self.new_node(GKind::Choice, c.line, text.clone());
        let jump = sole_static_jump(&c.body);
        {
            let n = &mut self.nodes[id as usize];
            n.end_line = c.end_line;
            n.body = text;
            n.clipped = clipped;
            if let Some((target, target_line)) = &jump {
                n.target = Some(target.clone());
                n.target_line = *target_line;
            }
        }
        self.edges.push(GEdge {
            from: menu,
            to: id,
            kind: EKind::Choice,
            label: None,
            cond: c.cond.clone(),
        });
        if jump.is_some() {
            return Vec::new();
        }
        let entry = vec![Open {
            from: id,
            kind: EKind::Next,
            label: None,
            cond: None,
        }];
        self.seq(&c.body, entry)
    }

    /// Append a content statement to the current dialogue run (or start one).
    fn add_run(&mut self, s: &Stmt, open: Vec<Open>) -> Vec<Open> {
        let say = matches!(s.kind, Kind::Say { .. });
        let beat = self.detail && !say;
        let extend = if self.detail && say {
            false
        } else if beat {
            open.len() == 1
                && open[0].kind == EKind::Next
                && open[0].from != ROOT
                && self
                    .nodes
                    .get(open[0].from as usize)
                    .is_some_and(|n| n.kind == GKind::Dialogue && n.says == 0)
        } else {
            open.len() == 1
                && open[0].kind == EKind::Next
                && open[0].from != ROOT
                && self.last_run == Some(open[0].from)
        };
        let id = if extend {
            open[0].from
        } else {
            let title = if beat { beat_title(s) } else { String::new() };
            let id = self.new_node(GKind::Dialogue, s.line, title);
            self.last_run = Some(id);
            self.connect(open, id);
            id
        };
        let lite = self.lite;
        let detail = self.detail;
        let node = &mut self.nodes[id as usize];
        node.end_line = node.end_line.max(s.end_line);
        node.stmts += 1;
        if beat && !lite && node.beats.len() < MAX_BEATS {
            node.beats.push(beat_line(s));
        }
        match &s.kind {
            Kind::Say { who, text } => {
                node.says += 1;
                if detail {
                    let (body, clipped) = clip_text(text);
                    node.body = body;
                    node.clipped = clipped;
                    if let Some(w) = who {
                        if node.title.is_empty() {
                            node.title = w.clone();
                        }
                        if !node.speakers.contains(w) {
                            node.speakers.push(w.clone());
                        }
                    }
                }
                if !lite {
                    if let Some(w) = who {
                        if !node.speakers.contains(w) && node.speakers.len() < 8 {
                            node.speakers.push(w.clone());
                        }
                    }
                    if node.preview.len() < 3 {
                        let t: String = text.chars().take(100).collect();
                        node.preview.push(match who {
                            Some(w) => format!("{w}: {t}"),
                            None => t,
                        });
                    }
                }
            }
            Kind::Python { .. } => node.python += 1,
            Kind::Opaque { .. } | Kind::Transform { .. } | Kind::Style { .. } => node.opaque += 1,
            Kind::If { branches } => {
                if branches.iter().all(|b| is_cosmetic(&b.body)) {
                    node.variants += 1;
                } else {
                    node.conds += 1;
                    node.says += branches.iter().map(|b| count_says(&b.body)).sum::<u32>();
                }
            }
            Kind::While { body, .. } => {
                node.conds += 1;
                node.says += count_says(body);
            }
            _ => {}
        }
        Self::next_from(id)
    }
}

const MAX_SCRIPT_LINES: usize = 4_000;
const MAX_LINE_CHARS: usize = 200;

/// One say, choice, jump, or beat in a label region.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptLine {
    pub kind: String,
    pub speaker: String,
    pub text: String,
    /// Jump or call target. For a choice, the label its body jumps to when that is the whole body.
    pub target: String,
    pub line: u32,
    pub end_line: u32,
    /// Physical line of `target` when it lives on a different row (a choice's jump).
    pub target_line: u32,
    pub depth: u32,
    /// True when `text` was cut to fit the list. The scene writer must not write it back.
    pub clipped: bool,
}

/// Dialogue and choices in one label, in source order.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LabelLines {
    pub name: String,
    pub lines: Vec<ScriptLine>,
    pub truncated: bool,
}

fn beat_line(s: &Stmt) -> BeatLine {
    let (cmd, text, editable) = match &s.kind {
        Kind::Present { cmd, text, .. } => {
            let code = if text.is_empty() {
                (*cmd).to_string()
            } else {
                format!("{cmd} {text}")
            };
            let single = s.end_line <= s.line;
            let ok = single && crate::scene::parse_present(&code).is_some();
            ((*cmd).to_string(), text.clone(), ok)
        }
        _ => {
            let title = beat_title(s);
            let (cmd, rest) = match title.split_once(' ') {
                Some((c, r)) => (c.to_string(), r.to_string()),
                None => (title, String::new()),
            };
            (cmd, rest, false)
        }
    };
    BeatLine {
        line: s.line,
        end_line: s.end_line,
        cmd,
        text,
        editable,
    }
}

fn beat_title(s: &Stmt) -> String {
    match &s.kind {
        Kind::Present { text, cmd, .. } => {
            if text.is_empty() {
                (*cmd).to_string()
            } else {
                format!("{cmd} {text}")
            }
        }
        Kind::Python { block: true, .. } => "python".into(),
        Kind::Python { .. } => "$".into(),
        Kind::If { .. } => "if".into(),
        Kind::While { .. } => "while".into(),
        Kind::Opaque { kind } => kind.clone(),
        Kind::Define { keyword, name, .. } => format!("{keyword} {name}"),
        Kind::Image { name } => format!("image {name}"),
        Kind::Pass => "pass".into(),
        _ => String::new(),
    }
}

fn clip_text(text: &str) -> (String, bool) {
    let mut out = String::new();
    for (i, ch) in text.chars().enumerate() {
        if i == MAX_LINE_CHARS {
            out.push('…');
            return (out, true);
        }
        out.push(ch);
    }
    (out, false)
}

fn push_line(
    out: &mut Vec<ScriptLine>,
    truncated: &mut bool,
    kind: &str,
    speaker: &str,
    text: &str,
    target: &str,
    line: u32,
    end_line: u32,
    target_line: u32,
    depth: u32,
) -> bool {
    if out.len() >= MAX_SCRIPT_LINES {
        *truncated = true;
        return false;
    }
    let (text, clipped) = clip_text(text);
    out.push(ScriptLine {
        kind: kind.into(),
        speaker: speaker.into(),
        text,
        target: target.into(),
        line,
        end_line,
        target_line,
        depth,
        clipped,
    });
    true
}

/// A choice whose body is only `jump label`.
fn sole_static_jump(body: &[Stmt]) -> Option<(String, u32)> {
    if body.len() != 1 {
        return None;
    }
    match &body[0].kind {
        Kind::Jump {
            target,
            dynamic: false,
        } => Some((target.clone(), body[0].line)),
        _ => None,
    }
}

/// `true` when every path through the menu leaves the block (jump or return).
fn take_menu(s: &Stmt, depth: u32, out: &mut Vec<ScriptLine>, truncated: &mut bool) -> bool {
    let Kind::Menu {
        caption,
        name,
        choices,
    } = &s.kind
    else {
        return false;
    };
    let title = caption
        .clone()
        .or_else(|| name.clone())
        .unwrap_or_else(|| "menu".into());
    if !push_line(
        out, truncated, "menu", "", &title, "", s.line, s.end_line, 0, depth,
    ) {
        return true;
    }
    if choices.is_empty() {
        return false;
    }
    let mut all_stopped = true;
    for c in choices {
        let jump = sole_static_jump(&c.body);
        let (target, target_line) = jump.clone().unwrap_or_default();
        if !push_line(
            out,
            truncated,
            "choice",
            "",
            &c.text,
            &target,
            c.line,
            c.end_line,
            target_line,
            depth + 1,
        ) {
            return true;
        }
        if jump.is_none() && !walk_seq(&c.body, depth + 2, out, truncated) {
            all_stopped = false;
        }
    }
    all_stopped
}

/// `true` when this block ends (jump, return, or the next label) and must not fall through.
fn walk_seq(stmts: &[Stmt], depth: u32, out: &mut Vec<ScriptLine>, truncated: &mut bool) -> bool {
    for s in stmts {
        if *truncated {
            return true;
        }
        match &s.kind {
            Kind::Label { .. } => return true,
            Kind::Menu { name: Some(_), .. } => return true,
            Kind::Menu { .. } => {
                if take_menu(s, depth, out, truncated) {
                    return true;
                }
            }
            Kind::Say { who, text } => {
                if !push_line(
                    out,
                    truncated,
                    "say",
                    who.as_deref().unwrap_or(""),
                    text,
                    "",
                    s.line,
                    s.end_line,
                    0,
                    depth,
                ) {
                    return true;
                }
            }
            Kind::Present { cmd, text, .. } if *cmd == "show" || *cmd == "scene" => {
                let shown = if text.is_empty() {
                    (*cmd).to_string()
                } else {
                    format!("{cmd} {text}")
                };
                if !push_line(
                    out, truncated, "beat", cmd, &shown, "", s.line, s.end_line, 0, depth,
                ) {
                    return true;
                }
            }
            Kind::If { branches } => {
                for b in branches {
                    walk_seq(&b.body, depth + 1, out, truncated);
                }
            }
            Kind::While { body, .. } => {
                walk_seq(body, depth + 1, out, truncated);
            }
            Kind::Jump { target, dynamic } => {
                let shown = if *dynamic {
                    format!("expression {target}")
                } else {
                    target.clone()
                };
                let _ = push_line(
                    out, truncated, "jump", "", &shown, target, s.line, s.end_line, s.line, depth,
                );
                return true;
            }
            Kind::Call {
                target, dynamic, ..
            } => {
                let shown = if *dynamic {
                    format!("expression {target}")
                } else {
                    target.clone()
                };
                if !push_line(
                    out, truncated, "call", "", &shown, target, s.line, s.end_line, s.line, depth,
                ) {
                    return true;
                }
            }
            Kind::Return => {
                let _ = push_line(
                    out, truncated, "return", "", "return", "", s.line, s.end_line, 0, depth,
                );
                return true;
            }
            Kind::Python { block, refs, .. }
                if !*block && refs.iter().any(|r| r.kind == RefKind::RenpyJump) =>
            {
                return true;
            }
            _ => {}
        }
    }
    false
}

fn walk_cont(cont: &Cont, out: &mut Vec<ScriptLine>, truncated: &mut bool) {
    let mut c = Some(cont);
    while let Some(cur) = c {
        if walk_seq(cur.rest, 0, out, truncated) || *truncated {
            return;
        }
        c = cur.parent;
    }
}

/// Says, menu captions, and choices in the same region the flow graph draws.
pub fn collect_lines(stmt: &Stmt, cont: &Cont) -> LabelLines {
    let mut lines = Vec::new();
    let mut truncated = false;
    let (name, stopped) = match &stmt.kind {
        Kind::Label { name, body } => (name.clone(), walk_seq(body, 0, &mut lines, &mut truncated)),
        Kind::Menu { name, .. } => {
            let label = name.clone().unwrap_or_default();
            let stopped = take_menu(stmt, 0, &mut lines, &mut truncated);
            (label, stopped)
        }
        _ => (String::new(), true),
    };
    if !stopped && !truncated {
        walk_cont(cont, &mut lines, &mut truncated);
    }
    LabelLines {
        name,
        lines,
        truncated,
    }
}

fn count_says(stmts: &[Stmt]) -> u32 {
    stmts
        .iter()
        .map(|s| match &s.kind {
            Kind::Say { .. } => 1,
            Kind::If { branches } => branches.iter().map(|b| count_says(&b.body)).sum(),
            Kind::While { body, .. } => count_says(body),
            _ => 0,
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::lex;
    use crate::parser::parse;

    fn graphs(src: &str) -> Vec<LabelGraph> {
        let out = parse(&lex(src).lines);
        let mut gs = Vec::new();
        visit_labels(&out.stmts, None, &mut |s, c| {
            gs.push(build_graph(s, c, false))
        });
        gs
    }

    fn targets(g: &LabelGraph, kind: GKind) -> Vec<String> {
        g.nodes
            .iter()
            .filter(|n| n.kind == kind)
            .filter_map(|n| n.target.clone())
            .collect()
    }

    const SRC: &str = "label start:\n    e \"hi\"\n    menu:\n        \"A\":\n            jump a\n        \"B\":\n            jump b\n    label a:\n        e \"a\"\n        jump end\n    label b:\n        e \"b\"\nlabel end:\n    return\n";

    #[test]
    fn menu_all_jumps_means_no_false_fallthrough() {
        let gs = graphs(SRC);
        let start = gs.iter().find(|g| g.name == "start").unwrap();
        assert_eq!(targets(start, GKind::Jump), ["a", "b"]);
        assert!(targets(start, GKind::Fall).is_empty());
    }

    #[test]
    fn fallthrough_climbs_parents_to_next_label() {
        let gs = graphs(SRC);
        let b = gs.iter().find(|g| g.name == "b").unwrap();
        // `b` ends without jump: falls out of `start`'s block into `end`.
        assert_eq!(targets(b, GKind::Fall), ["end"]);
        let a = gs.iter().find(|g| g.name == "a").unwrap();
        assert_eq!(targets(a, GKind::Jump), ["end"]);
        assert!(targets(a, GKind::Fall).is_empty());
        let end = gs.iter().find(|g| g.name == "end").unwrap();
        assert!(end.nodes.iter().any(|n| n.kind == GKind::Return));
    }

    #[test]
    fn cosmetic_ifs_fold_into_dialogue() {
        let src = "label x:\n    if tatu:\n        show a1\n    elif sexy:\n        show a2\n    e \"hi\"\n    if good:\n        jump nice\n    jump meh\n";
        let gs = graphs(src);
        let g = &gs[0];
        let d = g.nodes.iter().find(|n| n.kind == GKind::Dialogue).unwrap();
        assert_eq!(d.variants, 1);
        assert_eq!(d.says, 1);
        assert_eq!(g.nodes.iter().filter(|n| n.kind == GKind::Cond).count(), 1);
        assert_eq!(targets(g, GKind::Jump), ["nice", "meh"]);
    }

    #[test]
    fn label_lines_lists_say_choice_and_if() {
        let src = "label start:\n    e \"hi\"\n    if late:\n        e \"wait\"\n    menu:\n        \"Stay\":\n            e \"ok\"\n        \"Go\":\n            jump end\nlabel end:\n    e \"bye\"\n";
        let out = parse(&lex(src).lines);
        let mut found = None;
        visit_labels(&out.stmts, None, &mut |s, c| {
            if found.is_none() && matches!(&s.kind, Kind::Label { name, .. } if name == "start") {
                found = Some(collect_lines(s, c));
            }
        });
        let lines = found.unwrap();
        let brief: Vec<(&str, &str, u32)> = lines
            .lines
            .iter()
            .map(|l| (l.kind.as_str(), l.text.as_str(), l.depth))
            .collect();
        assert_eq!(
            brief,
            vec![
                ("say", "hi", 0),
                ("say", "wait", 1),
                ("menu", "menu", 0),
                ("choice", "Stay", 1),
                ("say", "ok", 2),
                ("choice", "Go", 1),
            ]
        );
        assert_eq!(lines.lines[0].speaker, "e");
        assert!(!lines.truncated);
        assert!(lines.lines.iter().all(|l| l.text != "bye"));
        let go = lines.lines.iter().find(|l| l.text == "Go").unwrap();
        assert_eq!(go.target, "end");
        assert!(go.target_line > go.line);
    }

    #[test]
    fn label_lines_include_scene_jump_and_return() {
        let src = "label start:\n    scene bg room\n    e happy \"hi\"\n    jump end\nlabel end:\n    return\n";
        let out = parse(&lex(src).lines);
        let mut found = None;
        visit_labels(&out.stmts, None, &mut |s, c| {
            if found.is_none() && matches!(&s.kind, Kind::Label { name, .. } if name == "start") {
                found = Some(collect_lines(s, c));
            }
        });
        let lines = found.unwrap();
        let brief: Vec<(&str, &str)> = lines
            .lines
            .iter()
            .map(|l| (l.kind.as_str(), l.text.as_str()))
            .collect();
        assert_eq!(
            brief,
            vec![("beat", "scene bg room"), ("say", "hi"), ("jump", "end")]
        );
        assert_eq!(lines.lines[2].target, "end");
    }

    #[test]
    fn named_menu_is_a_region() {
        let src = "label start:\n    menu chat:\n        \"x\":\n            jump a\n        \"y\":\n            jump b\n";
        let gs = graphs(src);
        let chat = gs.iter().find(|g| g.name == "chat").unwrap();
        assert_eq!(chat.kind, "menu");
        assert_eq!(targets(chat, GKind::Jump), ["a", "b"]);
        let start = gs.iter().find(|g| g.name == "start").unwrap();
        assert_eq!(targets(start, GKind::Fall), ["chat"]);
    }

    #[test]
    fn detail_splits_says_beats_and_choices() {
        let src = "label start:\n    e \"one\"\n    scene bg room\n    e \"two\"\n    menu:\n        \"Stay\":\n            e \"ok\"\n        \"Go\":\n            jump end\nlabel end:\n    return\n";
        let out = parse(&lex(src).lines);
        let mut detail = None;
        let mut overview = None;
        visit_labels(&out.stmts, None, &mut |s, c| {
            if detail.is_none() && matches!(&s.kind, Kind::Label { name, .. } if name == "start") {
                detail = Some(build_graph_with(s, c, false, &ScreenTable::default(), true));
                overview = Some(build_graph(s, c, false));
            }
        });
        let d = detail.unwrap();
        let says: Vec<_> = d
            .nodes
            .iter()
            .filter(|n| n.kind == GKind::Dialogue && n.says == 1)
            .collect();
        assert_eq!(says.len(), 3);
        assert_eq!(says[0].body, "one");
        assert_eq!(says[0].speakers, ["e"]);
        let beat = d
            .nodes
            .iter()
            .find(|n| n.kind == GKind::Dialogue && n.says == 0)
            .unwrap();
        assert_eq!(beat.title, "scene bg room");
        let one = says[0].id;
        assert!(d.edges.iter().any(|e| e.from == one && e.to == beat.id));
        assert!(d
            .edges
            .iter()
            .any(|e| e.from == beat.id && e.to == says[1].id));
        let choices: Vec<_> = d.nodes.iter().filter(|n| n.kind == GKind::Choice).collect();
        assert_eq!(choices.len(), 2);
        let go = choices.iter().find(|n| n.body == "Go").unwrap();
        assert_eq!(go.target.as_deref(), Some("end"));
        assert!(go.target_line > go.line);
        assert!(d.nodes.iter().all(|n| n.kind != GKind::Jump));
        let stay = choices.iter().find(|n| n.body == "Stay").unwrap();
        assert!(d
            .edges
            .iter()
            .any(|e| e.from == stay.id && e.to == says[2].id));
        assert!(overview
            .unwrap()
            .nodes
            .iter()
            .all(|n| n.kind != GKind::Choice));
    }

    #[test]
    fn detail_beat_cards_list_their_statements() {
        let src = "label start:\n    scene bg room with fade\n    show eileen at left\n    show eileen:\n        xalign 0.5\n    $ x = 1\n    e \"hi\"\n";
        let out = parse(&lex(src).lines);
        let mut graph = None;
        visit_labels(&out.stmts, None, &mut |s, c| {
            if graph.is_none() {
                graph = Some(build_graph_with(s, c, false, &ScreenTable::default(), true));
            }
        });
        let g = graph.unwrap();
        let beat = g
            .nodes
            .iter()
            .find(|n| n.kind == GKind::Dialogue && n.says == 0)
            .unwrap();
        let rows: Vec<(&str, u32, bool)> = beat
            .beats
            .iter()
            .map(|b| (b.cmd.as_str(), b.line, b.editable))
            .collect();
        assert_eq!(
            rows,
            [
                ("scene", 2, true),
                ("show", 3, true),
                ("show", 4, false),
                ("$", 6, false)
            ]
        );
    }

    #[test]
    fn call_continues_on_a_return_edge_and_keeps_the_choice_condition() {
        let src = "label start:\n    call shop\n    \"back\"\n    menu:\n        \"Go\" if karma > 5:\n            \"wait\"\n            jump end\nlabel shop:\n    return\nlabel end:\n    return\n";
        let gs = graphs(src);
        let start = gs.iter().find(|g| g.name == "start").unwrap();
        let call = start.nodes.iter().find(|n| n.kind == GKind::Call).unwrap();
        let ret = start.edges.iter().find(|e| e.from == call.id).unwrap();
        assert_eq!(ret.kind, EKind::Return);
        assert_eq!(ret.label.as_deref(), Some("return"));
        let jump = start.nodes.iter().find(|n| n.kind == GKind::Jump).unwrap();
        assert_eq!(jump.choice_label.as_deref(), Some("Go"));
        assert_eq!(jump.choice_cond.as_deref(), Some("karma > 5"));
    }
}
