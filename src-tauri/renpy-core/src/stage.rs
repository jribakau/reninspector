//! What is on screen at a source line.
//!
//! The cheapest path from `label start` (the same search replay uses) is
//! replayed for `scene`, `show` and `hide` only. When that path does not
//! exist, only the current label is replayed up to the line.

use std::collections::{HashMap, HashSet};
use std::fs;

use serde::Serialize;

use crate::ast::{Branch, BranchKind, Kind, Stmt};
use crate::parser::{collapse_ws, parse_say_full, QuotedPath};
use crate::project::{Project, SourceFile};
use crate::replay::{self, StoryPath};
use crate::vars::{self, NoteKind, Value, VarState};

const DEFAULT_W: u32 = 1920;
const DEFAULT_H: u32 = 1080;
const NOTE_CAP: usize = 12;
const VAR_CAP: usize = 24;
const USE_CAP: usize = 5;

/// One file inside a composite, at a pixel offset from the box's top left.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PictureLayer {
    pub path: String,
    pub x: f32,
    pub y: f32,
}

/// One step of an ATL frame animation. `seconds` is 0 when the script gives no pause.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AnimFrame {
    pub picture: Picture,
    pub seconds: f32,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Picture {
    File {
        path: String,
    },
    Color {
        hex: String,
    },
    /// A composite. `w` and `h` are its natural size, and `layers` are back to front.
    Layers {
        w: f32,
        h: f32,
        layers: Vec<PictureLayer>,
    },
    /// An ATL animation of several pictures. `repeat` loops it.
    Frames {
        frames: Vec<AnimFrame>,
        repeat: bool,
    },
    Unknown,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StageSprite {
    pub tag: String,
    pub name: String,
    pub layer: String,
    pub picture: Picture,
    pub xalign: Option<f32>,
    pub yalign: Option<f32>,
    pub xpos: Option<f32>,
    pub ypos: Option<f32>,
    pub xanchor: Option<f32>,
    pub yanchor: Option<f32>,
    pub zoom: Option<f32>,
    /// Size from the image's `Transform(xsize=, ysize=, size=)` or `im.Scale`, in screen pixels.
    pub size_w: Option<f32>,
    pub size_h: Option<f32>,
    /// `fill`, `contain`, `cover`, `scale-down` or `scale-up`.
    pub fit: Option<String>,
    /// `fit` from the `at` transform, applied after the image's own size.
    pub place_fit: Option<String>,
    /// A negative `xzoom` mirrors the sprite.
    pub flip: bool,
    pub shown_file: String,
    pub shown_line: u32,
    pub define_file: Option<String>,
    pub define_line: Option<u32>,
}

/// The say window at a dialogue line or a menu with a caption.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StageSay {
    pub who: Option<String>,
    pub who_color: Option<String>,
    pub what: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StageChoice {
    pub text: String,
    /// The choice has an `if`, so the game may hide it.
    pub conditional: bool,
    pub file: String,
    pub line: u32,
}

/// Say window and choice layout from `gui.rpy`, in screen pixels.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StageGui {
    pub textbox: Option<String>,
    pub namebox: Option<String>,
    pub choice: Option<String>,
    pub textbox_height: f32,
    pub textbox_yalign: f32,
    pub name_xpos: f32,
    pub name_ypos: f32,
    pub name_xalign: f32,
    pub dialogue_xpos: f32,
    pub dialogue_ypos: f32,
    pub dialogue_width: f32,
    pub text_size: f32,
    pub name_text_size: f32,
    pub text_color: String,
    pub accent_color: String,
    pub choice_width: f32,
    pub choice_text_size: f32,
    pub choice_spacing: f32,
    pub choice_text_color: String,
    /// Centre of the choice column.
    pub choice_ypos: f32,
}

/// One condition that mentions a variable the preview can pin.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StageVarUse {
    pub file: String,
    pub line: u32,
    pub cond: String,
}

/// A variable that chooses a picture. `origin` is `pinned`, `script` or `unset`.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StageVar {
    pub name: String,
    /// The literal the preview used, or empty when `origin` is `unset`.
    pub value: String,
    pub origin: String,
    pub uses: Vec<StageVarUse>,
    /// Uses of this name that were not listed.
    pub more: u32,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StageEstimate {
    pub width: u32,
    pub height: u32,
    pub sprites: Vec<StageSprite>,
    pub say: Option<StageSay>,
    pub choices: Vec<StageChoice>,
    pub gui: StageGui,
    /// `path` when the line is reached from `label start`, otherwise `label`.
    pub via: &'static str,
    pub decisions: u32,
    pub assumptions: Vec<String>,
    pub notes: Vec<String>,
    /// Variables in conditions that show or hide a picture, plus condition switches.
    pub vars: Vec<StageVar>,
    /// Variables beyond `vars`, nearest the caret first having been kept.
    pub vars_more: u32,
}

#[derive(Clone)]
struct Place {
    xalign: Option<f32>,
    yalign: Option<f32>,
    xpos: Option<f32>,
    ypos: Option<f32>,
    xanchor: Option<f32>,
    yanchor: Option<f32>,
    zoom: Option<f32>,
    /// `fit` from the `at` transform, applied to the screen area.
    fit: Option<String>,
    flip: bool,
}

#[derive(Clone)]
struct Size {
    w: Option<f32>,
    h: Option<f32>,
    fit: Option<String>,
    zoom: f32,
    /// A size was a variable or expression, so the full screen was assumed.
    guessed: bool,
}

impl Size {
    fn natural() -> Self {
        Self {
            w: None,
            h: None,
            fit: None,
            zoom: 1.0,
            guessed: false,
        }
    }
}

#[derive(Clone)]
struct Img {
    name: String,
    picture: Picture,
    size: Size,
    file: Option<String>,
    line: Option<u32>,
    /// Attribute words after the tag, lowercased.
    attrs: Vec<String>,
    /// Engine alias. Followed to another image's picture.
    reference: Option<String>,
    /// A displayable this preview cannot draw, such as `layered` or `atl`.
    undrawable: Option<String>,
    /// The image's own `xzoom` is negative.
    flip: bool,
    /// Composite, condition switch or `[variable]` pattern. Resolved after the replay.
    tree: Option<Node>,
}

impl Img {
    fn new(
        name: String,
        picture: Picture,
        size: Size,
        file: Option<String>,
        line: Option<u32>,
    ) -> Self {
        let attrs = attrs_of(&name);
        Self {
            name,
            picture,
            size,
            file,
            line,
            attrs,
            reference: None,
            undrawable: None,
            flip: false,
            tree: None,
        }
    }
}

/// A displayable the engine dump described, before variable values are known.
#[derive(Clone)]
enum Node {
    File(String),
    Color(String),
    Null,
    Ref(String),
    Composite {
        w: f32,
        h: f32,
        layers: Vec<PlacedNode>,
    },
    Switch(Vec<(String, Box<Node>)>),
    Pattern(String),
    /// ATL frames with their pauses in seconds, and whether the block repeats.
    Frames(Vec<(Node, f32)>, bool),
    Unknown,
}

#[derive(Clone)]
struct PlacedNode {
    x: f32,
    y: f32,
    node: Node,
}

#[derive(Clone)]
enum Talk {
    Say {
        who: Option<String>,
        span: String,
        fallback: String,
    },
    Menu {
        caption: Option<String>,
        choices: Vec<(String, bool, u32)>,
    },
}

struct Sprite {
    tag: String,
    name: String,
    layer: String,
    zorder: i32,
    seq: u32,
    place: Place,
    picture: Picture,
    size: Size,
    notes: Vec<String>,
    shown_file: String,
    shown_line: u32,
    define_file: Option<String>,
    define_line: Option<u32>,
    /// Attributes currently showing for this tag, used the way Ren'Py remembers them.
    attrs: Vec<String>,
    image_flip: bool,
    tree: Option<Node>,
}

struct ShowSpec {
    name: Option<String>,
    tag: Option<String>,
    at: Vec<String>,
    behind: Vec<String>,
    layer: Option<String>,
    zorder: Option<i32>,
    expression: bool,
}

/// Stage contents at `line` of `file` (`file` is relative to `game/`).
///
/// `pins` are session literals (`"12"`, `"True"`, `'"home"'`). A pin wins over
/// `default`, `define` and `$` lines. A value that is not a literal is ignored.
pub fn estimate(
    project: &Project,
    file: &str,
    line: u32,
    pins: &[(String, String)],
) -> Result<StageEstimate, String> {
    let file = file.trim().replace('\\', "/");
    let file = file.strip_prefix("game/").unwrap_or(&file).to_string();
    if line == 0 || file.is_empty() {
        return Err("Pick a line in the script first.".into());
    }
    if project.files.iter().all(|f| f.rel != file) {
        return Err(format!("`{file}` is not a script of this project."));
    }
    let index = project.stage_index();
    let mut vars = index.vars.clone();
    let mut pin_notes = Vec::new();
    for (name, raw) in pins {
        match vars::parse_literal(raw) {
            Some(value) => vars.pin(name, value),
            None => pin_notes.push(format!(
                "Could not read the pinned value `{raw}` for `{name}`."
            )),
        }
    }
    let mut stage = Replay::new(&index, vars);
    for note in &pin_notes {
        stage.note(note);
    }
    let line = stage.statement_at(&file, line);
    let basis = stage.basis.clone();
    let (via, decisions, assumptions) = match replay::path_with(
        &index.prepared,
        project,
        &file,
        line,
        &basis,
    ) {
        Ok(path) => {
            stage.apply_path(&path);
            ("path", path.decisions.len() as u32, path.assumptions)
        }
        Err(_) => {
            stage.note("Earlier labels were not followed.");
            if let Some(source) = project.files.iter().find(|f| f.rel == file) {
                if let Some(label) = find_label(&source.stmts, line) {
                    if let Kind::Label { body, .. } = &label.kind {
                        replay_label(&mut stage, &source.rel, body, line);
                    }
                } else {
                    stage.note("This line is not inside a label.");
                }
            }
            ("label", 0, Vec::new())
        }
    };
    if let Some(note) = &index.size_note {
        stage.note(note);
    }
    let (say, choices) = stage.talk_at(&file, line);
    stage.resolve_pictures();
    stage.record_switches();
    stage.resolve_assets(project);
    let (vars, vars_more) = stage.stage_vars();
    Ok(StageEstimate {
        width: index.width,
        height: index.height,
        notes: stage.all_notes(),
        sprites: stage.finish(),
        say,
        choices,
        gui: index.gui.clone(),
        via,
        decisions,
        assumptions,
        vars,
        vars_more,
    })
}

/// Built once per project change and reused for every caret move.
pub struct StageIndex {
    lines: HashMap<String, Vec<String>>,
    stmts: HashMap<String, HashMap<u32, StmtRec>>,
    stmt_lines: HashMap<String, Vec<u32>>,
    talk: HashMap<String, HashMap<u32, Talk>>,
    images: HashMap<String, Img>,
    by_tag: HashMap<String, Vec<String>>,
    transforms: HashMap<String, Place>,
    characters: HashMap<String, (Option<String>, Option<String>)>,
    vars: VarState,
    width: u32,
    height: u32,
    size_note: Option<String>,
    screen: (f32, f32),
    gui: StageGui,
    prepared: replay::Prepared,
    notes: Vec<String>,
}

struct StmtRec {
    end: u32,
    kind: KindKey,
}

/// Enough of a statement to replay it without borrowing the project.
#[derive(Clone)]
struct IfArm {
    cond: String,
    line: u32,
}

enum KindKey {
    Show { cmd: &'static str, text: String },
    Python { text: String },
    If { arms: Vec<IfArm>, visual: bool },
    Other,
}

impl StageIndex {
    pub(crate) fn build(project: &Project) -> Self {
        let mut lines = HashMap::new();
        for file in &project.files {
            lines.insert(file.rel.clone(), file_lines(file));
        }
        let (width, height, size_note) = screen_of(project, &lines);
        let screen = (width as f32, height as f32);
        let mut vars = VarState::default();
        for file in &project.files {
            vars.seed(&file.stmts);
        }
        let gui = read_gui(project, &lines, screen.0, screen.1);
        let mut index = Self {
            lines,
            stmts: HashMap::new(),
            stmt_lines: HashMap::new(),
            talk: HashMap::new(),
            images: HashMap::new(),
            by_tag: HashMap::new(),
            transforms: HashMap::new(),
            characters: HashMap::new(),
            vars,
            width,
            height,
            size_note,
            screen,
            gui,
            prepared: replay::prepare(project),
            notes: Vec::new(),
        };
        index.collect(project);
        for lines in index.stmt_lines.values_mut() {
            lines.sort_unstable();
            lines.dedup();
        }
        index
    }

    fn note(&mut self, text: &str) {
        if self.notes.len() >= NOTE_CAP || self.notes.iter().any(|n| n == text) {
            return;
        }
        self.notes.push(text.to_string());
    }

    fn span(&self, rel: &str, start: u32, end: u32) -> String {
        let Some(lines) = self.lines.get(rel) else {
            return String::new();
        };
        let from = start.saturating_sub(1) as usize;
        let to = (end as usize).min(lines.len());
        if from >= lines.len() || from >= to {
            return String::new();
        }
        lines[from..to].join(" ")
    }

    fn statement_source(&self, rel: &str, start: u32, end: u32) -> String {
        let Some(lines) = self.lines.get(rel) else {
            return String::new();
        };
        let from = start.saturating_sub(1) as usize;
        let to = (end as usize).min(lines.len());
        if from >= lines.len() || from >= to {
            return String::new();
        }
        lines[from..to].join("\n")
    }

    fn body_lines(&self, rel: &str, start: u32, end: u32) -> Vec<String> {
        let Some(lines) = self.lines.get(rel) else {
            return Vec::new();
        };
        let from = start as usize;
        let to = (end as usize).min(lines.len());
        if from >= to {
            return Vec::new();
        }
        lines[from..to].to_vec()
    }

    fn put_image(&mut self, key: String, img: Img) {
        if let Some(tag) = image_tag(&key) {
            let list = self.by_tag.entry(tag).or_default();
            if !list.iter().any(|k| k == &key) {
                list.push(key.clone());
            }
        }
        self.images.insert(key, img);
    }

    fn collect(&mut self, project: &Project) {
        for (name, path) in &project.auto_images.files {
            if self.images.contains_key(name) {
                continue;
            }
            self.put_image(
                name.clone(),
                Img::new(
                    name.clone(),
                    Picture::File { path: path.clone() },
                    Size::natural(),
                    None,
                    None,
                ),
            );
        }
        for file in &project.files {
            self.walk(file, &file.stmts, &file.meta.quoted_paths);
            for name in &file.meta.images {
                let key = collapse_ws(&name.to_lowercase());
                if key.is_empty() || self.images.contains_key(&key) {
                    continue;
                }
                self.put_image(
                    key,
                    Img::new(
                        name.clone(),
                        Picture::Unknown,
                        Size::natural(),
                        Some(file.rel.clone()),
                        None,
                    ),
                );
            }
        }
        self.absorb_engine(project);
    }

    /// Engine images replace static ones. A stale dump loses to an `image`
    /// statement, which was edited after the dump. Aliases are followed after
    /// the merge so they see that statement.
    fn absorb_engine(&mut self, project: &Project) {
        let Some(run) = &project.stage_images else {
            return;
        };
        if run.dump.callbacks {
            self.note("This game changes image attributes in Python, so a sprite may differ from what Ren'Py shows.");
        }
        for ch in &run.dump.characters {
            let var = ch.var.trim();
            if var.is_empty() || var.starts_with('_') {
                continue;
            }
            self.characters
                .entry(var.to_string())
                .or_insert((ch.name.clone(), ch.color.clone()));
        }
        let stale = run.key != project.init_key();
        for raw in &run.dump.images {
            let key = collapse_ws(&raw.name.to_lowercase());
            if key.is_empty() {
                continue;
            }
            if stale {
                if let Some(existing) = self.images.get(&key) {
                    if existing.line.is_some() {
                        continue;
                    }
                }
            }
            let img = engine_img(raw);
            // The engine cannot describe every ATL block. Keep frames read from the script.
            if img.undrawable.is_some()
                && self
                    .images
                    .get(&key)
                    .is_some_and(|have| have.tree.is_some())
            {
                continue;
            }
            self.put_image(key, img);
        }
        let refs: Vec<String> = self
            .images
            .iter()
            .filter(|(_, img)| img.reference.is_some())
            .map(|(key, _)| key.clone())
            .collect();
        for key in refs {
            let mut seen = Vec::new();
            if let Some(resolved) = self.follow_ref(&key, &mut seen) {
                self.images.insert(key, resolved);
            } else if let Some(img) = self.images.get_mut(&key) {
                img.reference = None;
                img.picture = Picture::Unknown;
            }
        }
    }

    fn follow_ref(&self, key: &str, seen: &mut Vec<String>) -> Option<Img> {
        if seen.len() > 8 || seen.iter().any(|s| s == key) {
            return None;
        }
        seen.push(key.to_string());
        let img = self.images.get(key)?.clone();
        let Some(target) = img.reference.clone() else {
            return Some(img);
        };
        let target_key = collapse_ws(&target.to_lowercase());
        let mut inner = self.follow_ref(&target_key, seen)?;
        if img.size.w.is_some() {
            inner.size.w = img.size.w;
        }
        if img.size.h.is_some() {
            inner.size.h = img.size.h;
        }
        if img.size.fit.is_some() {
            inner.size.fit = img.size.fit.clone();
        }
        inner.size.zoom *= img.size.zoom;
        inner.flip ^= img.flip;
        inner.name = img.name;
        inner.attrs = img.attrs;
        if img.file.is_some() {
            inner.file = img.file;
            inner.line = img.line;
        }
        inner.reference = None;
        Some(inner)
    }

    fn walk(&mut self, file: &SourceFile, stmts: &[Stmt], quoted: &[QuotedPath]) {
        for stmt in stmts {
            self.stmts.entry(file.rel.clone()).or_default().insert(
                stmt.line,
                StmtRec {
                    end: stmt.end_line,
                    kind: kind_key(stmt),
                },
            );
            self.stmt_lines
                .entry(file.rel.clone())
                .or_default()
                .push(stmt.line);
            match &stmt.kind {
                Kind::Image { name } => {
                    let key = collapse_ws(&name.to_lowercase());
                    if key.is_empty() {
                        continue;
                    }
                    let span = self.span(&file.rel, stmt.line, stmt.end_line);
                    let picture = if let Some(q) = quoted
                        .iter()
                        .find(|q| q.line == stmt.line && q.kind == "image")
                    {
                        Picture::File {
                            path: q.path.replace('\\', "/"),
                        }
                    } else if let Some(hex) = solid_color(&span) {
                        Picture::Color { hex }
                    } else {
                        Picture::Unknown
                    };
                    let size = image_size(&span, self.screen);
                    let frames = if picture == Picture::Unknown {
                        atl_frames(&self.body_lines(&file.rel, stmt.line, stmt.end_line))
                    } else {
                        None
                    };
                    let mut img = Img::new(
                        name.clone(),
                        picture,
                        size,
                        Some(file.rel.clone()),
                        Some(stmt.line),
                    );
                    img.tree = frames;
                    self.put_image(key, img);
                }
                Kind::Define { name, value, .. } if value.contains("Character(") => {
                    self.characters.insert(name.clone(), character_bits(value));
                }
                Kind::Say { who, text } => {
                    let span = self.span(&file.rel, stmt.line, stmt.end_line);
                    self.talk.entry(file.rel.clone()).or_default().insert(
                        stmt.line,
                        Talk::Say {
                            who: who.clone(),
                            span,
                            fallback: text.clone(),
                        },
                    );
                }
                Kind::Transform { name } => {
                    let body = self.body_lines(&file.rel, stmt.line, stmt.end_line);
                    if let Some(place) = literal_transform(&body) {
                        self.transforms.insert(name.to_lowercase(), place);
                    }
                }
                Kind::Label { body, .. } => self.walk(file, body, quoted),
                Kind::Menu {
                    caption, choices, ..
                } => {
                    let talk = Talk::Menu {
                        caption: caption.clone(),
                        choices: choices
                            .iter()
                            .map(|c| (c.text.clone(), c.cond.is_some(), c.line))
                            .collect(),
                    };
                    let first = choices.first().map(|c| c.line).unwrap_or(stmt.line + 1);
                    {
                        let talk_at = self.talk.entry(file.rel.clone()).or_default();
                        for at in stmt.line..first.max(stmt.line + 1) {
                            talk_at.insert(at, talk.clone());
                        }
                        for choice in choices {
                            talk_at.insert(choice.line, talk.clone());
                        }
                    }
                    for choice in choices {
                        self.walk(file, &choice.body, quoted);
                    }
                }
                Kind::If { branches } => {
                    for branch in branches {
                        self.walk(file, &branch.body, quoted);
                    }
                }
                Kind::While { body, .. } => self.walk(file, body, quoted),
                _ => {}
            }
        }
    }
}

/// Sprites and variable values for one caret line. The index stays shared.
struct VarUse {
    name: String,
    file: String,
    line: u32,
    cond: String,
}

struct Replay<'a> {
    index: &'a StageIndex,
    sprites: Vec<Sprite>,
    seq: u32,
    notes: Vec<String>,
    /// Defaults, pins and `$` lines, for pictures that interpolate a name.
    vars: VarState,
    /// Defaults and pins only. Branch choice ignores `$` lines.
    basis: VarState,
    uses: Vec<VarUse>,
    seen: HashSet<(String, String, u32)>,
}

impl<'a> Replay<'a> {
    fn new(index: &'a StageIndex, vars: VarState) -> Self {
        Self {
            index,
            sprites: Vec::new(),
            seq: 0,
            notes: Vec::new(),
            basis: vars.clone(),
            vars,
            uses: Vec::new(),
            seen: HashSet::new(),
        }
    }

    fn note(&mut self, text: &str) {
        if self.notes.len() >= NOTE_CAP
            || self.notes.iter().any(|n| n == text)
            || self.index.notes.iter().any(|n| n == text)
        {
            return;
        }
        self.notes.push(text.to_string());
    }

    fn note_branch(&mut self, file: &str, branches: &[Branch], idx: usize, kind: NoteKind) {
        let Some(branch) = branches.get(idx) else {
            return;
        };
        let earlier: Vec<String> = branches[..idx].iter().map(|b| b.cond.clone()).collect();
        let text = vars::condition_note(
            file,
            branch.line,
            branch_how(branch.kind),
            &branch.cond,
            &earlier,
            kind,
            &self.basis,
        );
        self.note(&text);
    }

    fn record_names(&mut self, file: &str, line: u32, cond: &str) {
        if cond.is_empty() {
            return;
        }
        for name in VarState::names_in(cond) {
            if !self
                .seen
                .insert((name.clone(), file.to_string(), line))
            {
                continue;
            }
            self.uses.push(VarUse {
                name,
                file: file.to_string(),
                line,
                cond: cond.to_string(),
            });
        }
    }

    /// Condition switches on the pictures currently shown.
    fn record_switches(&mut self) {
        let found: Vec<(String, u32, Vec<String>)> = self
            .sprites
            .iter()
            .filter_map(|sprite| {
                let tree = sprite.tree.as_ref()?;
                let mut conds = Vec::new();
                switch_conds(tree, &mut conds);
                if conds.is_empty() {
                    return None;
                }
                let file = sprite
                    .define_file
                    .clone()
                    .unwrap_or_else(|| sprite.shown_file.clone());
                let line = sprite.define_line.unwrap_or(sprite.shown_line);
                Some((file, line, conds))
            })
            .collect();
        for (file, line, conds) in found {
            for cond in conds {
                self.record_names(&file, line, &cond);
            }
        }
    }

    /// Variables nearest the caret first, capped so the strip stays small.
    fn stage_vars(&self) -> (Vec<StageVar>, u32) {
        let mut grouped: HashMap<String, Vec<StageVarUse>> = HashMap::new();
        let mut order = Vec::new();
        for use_ in &self.uses {
            if !grouped.contains_key(&use_.name) {
                order.push(use_.name.clone());
            }
            grouped.entry(use_.name.clone()).or_default().push(StageVarUse {
                file: use_.file.clone(),
                line: use_.line,
                cond: use_.cond.clone(),
            });
        }
        let mut ranked: Vec<(u32, usize, String)> = order
            .iter()
            .enumerate()
            .map(|(i, name)| {
                let nearest = grouped
                    .get(name)
                    .and_then(|uses| uses.iter().map(|u| u.line).max())
                    .unwrap_or(0);
                (nearest, i, name.clone())
            })
            .collect();
        ranked.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        let vars_more = ranked.len().saturating_sub(VAR_CAP) as u32;
        ranked.truncate(VAR_CAP);
        let vars = ranked
            .into_iter()
            .map(|(_, _, name)| {
                let pinned = self.basis.is_pinned(&name);
                let known = self
                    .basis
                    .get(&name)
                    .filter(|v| !matches!(v, Value::Unknown));
                let origin = if pinned {
                    "pinned"
                } else if known.is_some() {
                    "script"
                } else {
                    "unset"
                };
                let mut uses = grouped.remove(&name).unwrap_or_default();
                uses.sort_by(|a, b| b.line.cmp(&a.line).then(a.file.cmp(&b.file)));
                let more = uses.len().saturating_sub(USE_CAP) as u32;
                uses.truncate(USE_CAP);
                StageVar {
                    value: known.map(Value::display).unwrap_or_default(),
                    name,
                    origin: origin.into(),
                    uses,
                    more,
                }
            })
            .collect();
        (vars, vars_more)
    }

    /// The say window and choices the game shows while it waits at `line`.
    fn talk_at(&self, file: &str, line: u32) -> (Option<StageSay>, Vec<StageChoice>) {
        match self.index.talk.get(file).and_then(|m| m.get(&line)) {
            Some(Talk::Say {
                who,
                span,
                fallback,
            }) => {
                let what = parse_say_full(span)
                    .map(|(_, what)| what)
                    .unwrap_or_else(|| fallback.clone());
                (Some(self.say_of(who.as_deref(), &what)), Vec::new())
            }
            Some(Talk::Menu { caption, choices }) => {
                let say = caption.as_ref().map(|text| self.say_of(None, text));
                let choices = choices
                    .iter()
                    .map(|(text, conditional, at)| StageChoice {
                        text: plain_text(text),
                        conditional: *conditional,
                        file: file.to_string(),
                        line: *at,
                    })
                    .collect();
                (say, choices)
            }
            None => (None, Vec::new()),
        }
    }

    fn say_of(&self, who: Option<&str>, what: &str) -> StageSay {
        let (name, color) = match who {
            Some(w) if w.starts_with('"') => (Some(w.trim_matches('"').to_string()), None),
            Some(w) => match self.index.characters.get(w) {
                Some((name, color)) => (name.clone(), color.clone()),
                None => (Some(w.to_string()), None),
            },
            None => self
                .index
                .characters
                .get("narrator")
                .cloned()
                .unwrap_or((None, None)),
        };
        StageSay {
            who: name.map(|n| plain_text(&n)).filter(|n| !n.is_empty()),
            who_color: color.and_then(|c| hex_of(&c)),
            what: plain_text(what),
        }
    }

    fn apply_path(&mut self, path: &StoryPath) {
        let mut prev: Option<(String, u32)> = None;
        for (file, line) in &path.steps {
            if prev.as_ref().is_some_and(|(f, l)| f == file && *l == *line) {
                continue;
            }
            prev = Some((file.clone(), *line));
            self.apply_line(file, *line);
        }
    }

    fn apply_line(&mut self, file: &str, line: u32) {
        let Some(rec) = self.index.stmts.get(file).and_then(|m| m.get(&line)) else {
            return;
        };
        match &rec.kind {
            KindKey::Show { cmd, text } => self.apply_show(file, line, *cmd, text),
            KindKey::If { arms, visual } => {
                if *visual {
                    for arm in arms {
                        self.record_names(file, arm.line, &arm.cond);
                    }
                }
            }
            KindKey::Python { text } => {
                let span = self.index.span(file, line, rec.end);
                let source = self.index.statement_source(file, line, rec.end);
                let body = if span.contains("renpy.") {
                    span
                } else {
                    text.clone()
                };
                self.note_python(file, line, &body);
                self.vars
                    .apply(if source.is_empty() { &body } else { &source });
            }
            KindKey::Other => {}
        }
    }

    fn note_python(&mut self, file: &str, line: u32, text: &str) {
        for call in ["renpy.show", "renpy.scene", "renpy.hide"] {
            if text.contains(call) {
                self.note(&format!(
                    "{file}:{line} calls {call}, which this preview does not run."
                ));
                return;
            }
        }
    }

    fn apply_show(&mut self, file: &str, line: u32, cmd: &'static str, text: &str) {
        let spec = parse_show(text);
        if spec.expression {
            self.note(&format!(
                "{file}:{line} shows an expression, which this preview does not run."
            ));
            return;
        }
        let layer = spec.layer.clone().unwrap_or_else(|| "master".into());
        if cmd == "scene" {
            self.sprites.retain(|s| s.layer != layer);
        }
        if cmd == "hide" {
            let tag = spec
                .tag
                .clone()
                .or_else(|| spec.name.as_ref().and_then(|n| first_word(n)));
            if let Some(tag) = tag {
                let only = spec.layer.clone();
                self.sprites
                    .retain(|s| s.tag != tag || only.as_ref().is_some_and(|l| s.layer != *l));
            }
            return;
        }
        if cmd != "show" && cmd != "scene" {
            return;
        }
        let Some(name) = spec.name.clone() else {
            return;
        };
        let tag = spec
            .tag
            .clone()
            .or_else(|| first_word(&name))
            .unwrap_or_else(|| name.clone());
        let prev_attrs = self
            .sprites
            .iter()
            .find(|s| s.tag == tag && s.layer == layer)
            .map(|s| s.attrs.clone())
            .unwrap_or_default();
        let chosen = choose_image(self.index, &name, &prev_attrs);
        let (
            picture,
            size,
            define_file,
            define_line,
            shown_name,
            attrs,
            image_flip,
            undrawable,
            tree,
        ) = match chosen {
            Choice::One(img) => (
                img.picture.clone(),
                img.size.clone(),
                img.file.clone(),
                img.line,
                img.name.clone(),
                img.attrs.clone(),
                img.flip,
                img.undrawable.clone(),
                img.tree.clone(),
            ),
            Choice::Missing => match builtin_picture(&name) {
                Some(picture) => (
                    picture,
                    Size::natural(),
                    None,
                    None,
                    name.clone(),
                    Vec::new(),
                    false,
                    None,
                    None,
                ),
                None => (
                    Picture::Unknown,
                    Size::natural(),
                    None,
                    None,
                    name.clone(),
                    prev_attrs,
                    false,
                    None,
                    None,
                ),
            },
            Choice::Ambiguous(_) => (
                Picture::Unknown,
                Size::natural(),
                None,
                None,
                name.clone(),
                prev_attrs,
                false,
                None,
                None,
            ),
        };
        let mut notes = Vec::new();
        if let Choice::Ambiguous(options) = &chosen {
            notes.push(format!(
                "Ren'Py would stop here: showing {name} is ambiguous ({}).",
                options.join(", ")
            ));
        } else if let Some(kind) = &undrawable {
            notes.push(format!(
                "{shown_name} is a {kind}, which this preview does not draw."
            ));
        } else if picture == Picture::Unknown && tree.is_none() {
            notes.push(format!("{file}:{line} has no image for {name}."));
        }
        if size.guessed {
            notes.push(format!(
                "{shown_name} is sized with a variable; the preview assumed the full screen."
            ));
        }
        let (place, place_note) = self.place_of(file, line, spec.at.last());
        notes.extend(place_note);
        let zorder = spec.zorder.unwrap_or(0);
        let picture = if undrawable.is_some() {
            Picture::Unknown
        } else {
            picture
        };
        let sprite = Sprite {
            tag: tag.clone(),
            name: shown_name,
            layer: layer.clone(),
            zorder,
            seq: self.seq,
            place,
            picture,
            size,
            shown_file: file.to_string(),
            shown_line: line,
            define_file,
            define_line,
            notes,
            attrs,
            image_flip,
            tree,
        };
        self.seq += 1;
        if let Some(pos) = self
            .sprites
            .iter()
            .position(|s| s.tag == tag && s.layer == layer)
        {
            let seq = self.sprites[pos].seq;
            let mut sprite = sprite;
            sprite.seq = seq;
            self.sprites[pos] = sprite;
            return;
        }
        if let Some(behind) = spec.behind.first() {
            if let Some(pos) = self
                .sprites
                .iter()
                .position(|s| s.tag == *behind && s.layer == layer)
            {
                self.sprites.insert(pos, sprite);
                self.renumber();
                return;
            }
        }
        self.sprites.push(sprite);
    }

    fn place_of(&self, file: &str, line: u32, at: Option<&String>) -> (Place, Option<String>) {
        let Some(name) = at else {
            return (default_place(), None);
        };
        let key = name.to_lowercase();
        if let Some(place) = builtin_place(&key) {
            return (place, None);
        }
        if let Some(place) = self.index.transforms.get(&key).cloned() {
            return (place, None);
        }
        let note = format!("{file}:{line} uses transform {name}, which this preview cannot read.");
        (default_place(), Some(note))
    }

    /// The statement that starts at `line`, or the closest one above it, so a
    /// blank line or a comment shows what the line before it left on screen.
    fn statement_at(&self, file: &str, line: u32) -> u32 {
        let Some(lines) = self.index.stmt_lines.get(file) else {
            return line;
        };
        match lines.binary_search(&line) {
            Ok(_) => line,
            Err(0) => line,
            Err(i) => lines[i - 1],
        }
    }

    /// Picks each composite layer from the variable values at the caret line.
    fn resolve_pictures(&mut self) {
        let sprites = std::mem::take(&mut self.sprites);
        self.sprites = sprites
            .into_iter()
            .map(|mut sprite| {
                let Some(tree) = sprite.tree.clone() else {
                    return sprite;
                };
                let outcome = resolve_node(&tree, &self.index.images, &self.vars, 0);
                if let Picture::Layers { w, h, .. } = &outcome.picture {
                    if *w > 0.0 {
                        sprite.size.w = Some(*w);
                    }
                    if *h > 0.0 {
                        sprite.size.h = Some(*h);
                    }
                }
                if let Some(note) = layer_note(&sprite.tag, &outcome) {
                    if !sprite.notes.contains(&note) {
                        sprite.notes.push(note);
                    }
                }
                sprite.picture = outcome.picture;
                sprite
            })
            .collect();
    }

    /// Rewrites every drawn file to the path the game would load (a bare name
    /// is also looked for under `images/`). A file that is nowhere keeps its
    /// name and is reported once.
    fn resolve_assets(&mut self, project: &Project) {
        let mut found: HashMap<String, Option<String>> = HashMap::new();
        let mut missing: Vec<String> = Vec::new();
        for sprite in &mut self.sprites {
            for_each_path(&mut sprite.picture, &mut |path: &mut String| {
                let hit = found
                    .entry(path.clone())
                    .or_insert_with(|| project.find_asset(path))
                    .clone();
                match hit {
                    Some(real) => *path = real,
                    None => push_unique(&mut missing, path.clone()),
                }
            });
        }
        for path in missing {
            self.note(&format!("`{path}` was not found in game/ or images/."));
        }
    }

    /// General notes, then the notes of sprites still on screen.
    fn all_notes(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for note in self
            .index
            .notes
            .iter()
            .chain(self.notes.iter())
            .chain(self.sprites.iter().flat_map(|s| s.notes.iter()))
        {
            if out.len() < NOTE_CAP && !out.contains(note) {
                out.push(note.clone());
            }
        }
        out
    }

    fn renumber(&mut self) {
        for (i, sprite) in self.sprites.iter_mut().enumerate() {
            sprite.seq = i as u32;
        }
        self.seq = self.sprites.len() as u32;
    }

    fn finish(self) -> Vec<StageSprite> {
        let mut sprites = self.sprites;
        sprites.sort_by(|a, b| {
            layer_rank(&a.layer)
                .cmp(&layer_rank(&b.layer))
                .then(a.zorder.cmp(&b.zorder))
                .then(a.seq.cmp(&b.seq))
        });
        sprites
            .into_iter()
            .map(|s| StageSprite {
                tag: s.tag,
                name: s.name,
                layer: s.layer,
                picture: s.picture,
                xalign: s.place.xalign,
                yalign: s.place.yalign,
                xpos: s.place.xpos,
                ypos: s.place.ypos,
                xanchor: s.place.xanchor,
                yanchor: s.place.yanchor,
                zoom: Some(s.place.zoom.unwrap_or(1.0) * s.size.zoom),
                size_w: s.size.w,
                size_h: s.size.h,
                fit: s.size.fit,
                place_fit: s.place.fit,
                flip: s.place.flip ^ s.image_flip,
                shown_file: s.shown_file,
                shown_line: s.shown_line,
                define_file: s.define_file,
                define_line: s.define_line,
            })
            .collect()
    }
}
fn kind_key(stmt: &Stmt) -> KindKey {
    match &stmt.kind {
        Kind::Present { cmd, text, .. } if *cmd == "show" || *cmd == "scene" || *cmd == "hide" => {
            KindKey::Show {
                cmd,
                text: text.clone(),
            }
        }
        Kind::Python { text, .. } => KindKey::Python { text: text.clone() },
        Kind::If { branches } => KindKey::If {
            arms: branches
                .iter()
                .map(|b| IfArm {
                    cond: b.cond.clone(),
                    line: b.line,
                })
                .collect(),
            visual: branches.iter().any(|b| presents(&b.body)),
        },
        _ => KindKey::Other,
    }
}

fn note_kind(verdicts: &[vars::Verdict], idx: usize) -> Option<NoteKind> {
    match verdicts.get(idx)? {
        vars::Verdict::True => None,
        vars::Verdict::False => Some(NoteKind::Denied),
        vars::Verdict::Unknown => {
            let prefer = vars::preferred(verdicts);
            Some(if Some(idx) == prefer {
                NoteKind::Default
            } else {
                NoteKind::Undecided
            })
        }
    }
}

fn branch_how(kind: BranchKind) -> &'static str {
    match kind {
        BranchKind::If => "if",
        BranchKind::Elif => "elif",
        BranchKind::Else => "else",
    }
}

/// True when the statements show, hide or change the background, including inside a branch.
fn presents(stmts: &[Stmt]) -> bool {
    stmts.iter().any(|stmt| match &stmt.kind {
        Kind::Present { cmd, .. } if *cmd == "show" || *cmd == "scene" || *cmd == "hide" => true,
        Kind::If { branches } => branches.iter().any(|b| presents(&b.body)),
        Kind::While { body, .. } => presents(body),
        Kind::Menu { choices, .. } => choices.iter().any(|c| presents(&c.body)),
        Kind::Label { body, .. } => presents(body),
        _ => false,
    })
}

fn switch_conds(node: &Node, out: &mut Vec<String>) {
    match node {
        Node::Switch(cases) => {
            for (when, child) in cases {
                if !when.is_empty() {
                    out.push(when.clone());
                }
                switch_conds(child, out);
            }
        }
        Node::Composite { layers, .. } => {
            for layer in layers {
                switch_conds(&layer.node, out);
            }
        }
        Node::Frames(list, _) => {
            for (child, _) in list {
                switch_conds(child, out);
            }
        }
        _ => {}
    }
}

fn layer_rank(layer: &str) -> u8 {
    if layer == "master" {
        0
    } else {
        1
    }
}

fn first_word(name: &str) -> Option<String> {
    name.split_whitespace().next().map(|w| w.to_string())
}

fn default_place() -> Place {
    align_place(0.5, 1.0)
}

fn align_place(x: f32, y: f32) -> Place {
    Place {
        xalign: Some(x),
        yalign: Some(y),
        xpos: Some(x),
        ypos: Some(y),
        xanchor: Some(x),
        yanchor: Some(y),
        zoom: Some(1.0),
        fit: None,
        flip: false,
    }
}

fn builtin_place(name: &str) -> Option<Place> {
    let mut place = match name {
        "left" => align_place(0.0, 1.0),
        "right" => align_place(1.0, 1.0),
        "center" => align_place(0.5, 1.0),
        "truecenter" => align_place(0.5, 0.5),
        "top" => align_place(0.5, 0.0),
        "topleft" => align_place(0.0, 0.0),
        "topright" => align_place(1.0, 0.0),
        "offscreenleft" => align_place(0.0, 1.0),
        "offscreenright" => align_place(1.0, 1.0),
        _ => return None,
    };
    if name == "offscreenleft" {
        place.xanchor = Some(1.0);
    } else if name == "offscreenright" {
        place.xanchor = Some(0.0);
    }
    Some(place)
}

const SHOW_WORDS: &[&str] = &[
    "at",
    "as",
    "behind",
    "with",
    "zorder",
    "onlayer",
    "expression",
];

fn parse_show(text: &str) -> ShowSpec {
    let tokens: Vec<String> = text
        .split_whitespace()
        .map(|w| w.trim_matches(|c: char| c == ',' || c == ':').to_string())
        .filter(|w| !w.is_empty())
        .collect();
    let mut spec = ShowSpec {
        name: None,
        tag: None,
        at: Vec::new(),
        behind: Vec::new(),
        layer: None,
        zorder: None,
        expression: false,
    };
    if tokens.first().is_some_and(|t| t == "expression") {
        spec.expression = true;
        return spec;
    }
    let mut name = Vec::new();
    let mut i = 0;
    while i < tokens.len() && !SHOW_WORDS.contains(&tokens[i].as_str()) {
        if tokens[i].contains(['"', '\'', '(', '[', '=']) {
            break;
        }
        name.push(tokens[i].clone());
        i += 1;
    }
    if !name.is_empty() && name[0] != "screen" {
        spec.name = Some(name.join(" "));
    }
    while i < tokens.len() {
        let word = tokens[i].as_str();
        i += 1;
        match word {
            "at" => {
                while i < tokens.len() && !SHOW_WORDS.contains(&tokens[i].as_str()) {
                    spec.at.push(tokens[i].clone());
                    i += 1;
                }
            }
            "as" => {
                if i < tokens.len() {
                    spec.tag = Some(tokens[i].clone());
                    i += 1;
                }
            }
            "behind" => {
                while i < tokens.len() && !SHOW_WORDS.contains(&tokens[i].as_str()) {
                    spec.behind.push(tokens[i].clone());
                    i += 1;
                }
            }
            "onlayer" => {
                if i < tokens.len() {
                    spec.layer = Some(tokens[i].clone());
                    i += 1;
                }
            }
            "zorder" => {
                if i < tokens.len() {
                    spec.zorder = tokens[i].parse().ok();
                    i += 1;
                }
            }
            "expression" => spec.expression = true,
            "with" => break,
            _ => {}
        }
    }
    spec
}

enum Choice<'a> {
    One(&'a Img),
    Ambiguous(Vec<String>),
    Missing,
}

/// Ren'Py's `apply_attributes` / `choose_image`: an exact name wins. Otherwise
/// every attribute of a candidate must be newly requested or already showing,
/// the candidate must contain every new attribute, and the longest match wins.
/// A tie is ambiguous. A leading `-` removes an attribute.
fn choose_image<'a>(index: &'a StageIndex, name: &str, optional_now: &[String]) -> Choice<'a> {
    let key = collapse_ws(&name.to_lowercase());
    if let Some(img) = index.images.get(&key) {
        return Choice::One(img);
    }
    let words: Vec<&str> = key.split_whitespace().collect();
    let Some(tag) = words.first().copied() else {
        return Choice::Missing;
    };
    let mut required: Vec<String> = Vec::new();
    let mut optional: Vec<String> = optional_now.to_vec();
    for word in words.iter().skip(1) {
        if let Some(stripped) = word.strip_prefix('-') {
            if stripped.is_empty() {
                continue;
            }
            optional.retain(|a| a != stripped);
            required.retain(|a| a != stripped);
        } else {
            required.push((*word).to_string());
        }
    }
    let mut max_len = -1i32;
    let mut matches: Vec<&Img> = Vec::new();
    let Some(keys) = index.by_tag.get(tag) else {
        return Choice::Missing;
    };
    for key in keys {
        let Some(img) = index.images.get(key) else {
            continue;
        };
        if image_tag(&img.name).as_deref() != Some(tag) {
            continue;
        }
        if !img
            .attrs
            .iter()
            .all(|a| required.iter().any(|r| r == a) || optional.iter().any(|o| o == a))
        {
            continue;
        }
        let found = img
            .attrs
            .iter()
            .filter(|a| required.iter().any(|r| r == *a))
            .count();
        if found != required.len() {
            continue;
        }
        let len_attrs = img.attrs.len() as i32;
        if len_attrs < max_len {
            continue;
        }
        if len_attrs > max_len {
            max_len = len_attrs;
            matches.clear();
        }
        matches.push(img);
    }
    match matches.len() {
        0 => Choice::Missing,
        1 => Choice::One(matches[0]),
        _ => {
            let mut names: Vec<String> = matches.iter().map(|img| img.name.clone()).collect();
            names.sort();
            Choice::Ambiguous(names)
        }
    }
}

fn attrs_of(name: &str) -> Vec<String> {
    collapse_ws(&name.to_lowercase())
        .split_whitespace()
        .skip(1)
        .map(|s| s.to_string())
        .collect()
}

fn image_tag(name: &str) -> Option<String> {
    collapse_ws(&name.to_lowercase())
        .split_whitespace()
        .next()
        .map(|s| s.to_string())
}

fn engine_img(raw: &crate::engine::EngineImage) -> Img {
    let mut img = Img::new(
        raw.name.clone(),
        Picture::Unknown,
        Size::natural(),
        None,
        None,
    );
    img.size.zoom = raw.zoom * raw.xzoom.abs() * raw.yzoom.abs();
    img.flip = raw.xzoom < 0.0;
    img.size.w = raw.w.filter(|w| *w > 0.0);
    img.size.h = raw.h.filter(|h| *h > 0.0);
    img.size.fit = raw.fit.clone().filter(|f| !f.is_empty());
    match raw.kind.as_str() {
        "image" => {
            if let Some(path) = raw.file.as_ref().filter(|p| !p.is_empty()) {
                img.picture = Picture::File {
                    path: normalize_game_path(path),
                };
            }
        }
        "solid" => {
            if let Some(hex) = raw.color.as_deref().and_then(hex_of) {
                img.picture = Picture::Color { hex };
            }
        }
        "ref" => img.reference = raw.reference.clone().filter(|r| !r.is_empty()),
        "composite" | "switch" | "pattern" | "frames" => img.tree = Some(node_of(raw)),
        other => img.undrawable = Some(kind_label(other).to_string()),
    }
    img
}

fn node_of(raw: &crate::engine::EngineImage) -> Node {
    match raw.kind.as_str() {
        "image" => raw
            .file
            .as_ref()
            .filter(|p| !p.is_empty())
            .map(|p| Node::File(normalize_game_path(p)))
            .unwrap_or(Node::Unknown),
        "solid" => raw
            .color
            .as_deref()
            .and_then(hex_of)
            .map(Node::Color)
            .unwrap_or(Node::Unknown),
        "null" => Node::Null,
        "ref" => raw
            .reference
            .clone()
            .filter(|r| !r.is_empty())
            .map(Node::Ref)
            .unwrap_or(Node::Unknown),
        "composite" => Node::Composite {
            w: raw.w.unwrap_or(0.0),
            h: raw.h.unwrap_or(0.0),
            layers: raw
                .layers
                .iter()
                .map(|layer| PlacedNode {
                    x: layer.x,
                    y: layer.y,
                    node: node_of(&layer.node),
                })
                .collect(),
        },
        "switch" => Node::Switch(
            raw.cases
                .iter()
                .map(|case| (case.when.clone(), Box::new(node_of(&case.node))))
                .collect(),
        ),
        "pattern" => Node::Pattern(raw.pattern.clone().unwrap_or_default()),
        "frames" => Node::Frames(
            raw.frames
                .iter()
                .map(|frame| (node_of(&frame.node), frame.seconds.max(0.0)))
                .collect(),
            raw.repeat,
        ),
        _ => Node::Unknown,
    }
}

const FRAME_CAP: usize = 64;

fn image_ext(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    [".png", ".jpg", ".jpeg", ".webp", ".gif", ".avif"]
        .iter()
        .any(|e| lower.ends_with(e))
}

/// Frames of an ATL `image` block: quoted pictures, `pause N` and `repeat`.
/// Everything else (transforms, `with`, blocks) is skipped.
fn atl_frames(body: &[String]) -> Option<Node> {
    let mut frames: Vec<(Node, f32)> = Vec::new();
    let mut repeat = false;
    for raw in body {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let first = line.split_whitespace().next().unwrap_or("");
        if let Some(rest) = line.strip_prefix('"').or_else(|| line.strip_prefix('\'')) {
            let quote = line.chars().next().unwrap_or('"');
            let Some(end) = rest.find(quote) else {
                continue;
            };
            let name = &rest[..end];
            let after = rest[end + 1..].trim();
            if name.is_empty()
                || !(after.is_empty() || after.starts_with("with ") || after == "with")
            {
                continue;
            }
            if frames.len() >= FRAME_CAP {
                continue;
            }
            let node = if image_ext(name) {
                Node::File(normalize_game_path(name))
            } else {
                Node::Ref(name.to_string())
            };
            frames.push((node, 0.0));
        } else if first == "repeat" {
            repeat = true;
        } else if first == "pause" || first == "linear" || first.starts_with("ease") {
            let secs = line
                .split_whitespace()
                .nth(1)
                .and_then(|n| n.parse::<f32>().ok());
            if let (Some(secs), Some(last)) = (secs, frames.last_mut()) {
                if secs > 0.0 {
                    last.1 += secs;
                }
            }
        }
    }
    if frames.is_empty() {
        None
    } else {
        Some(Node::Frames(frames, repeat))
    }
}

struct Outcome {
    picture: Picture,
    unknowns: Vec<String>,
    /// A condition was not true or false, so the first possible case was used.
    guessed: bool,
}

fn resolve_node(
    node: &Node,
    images: &HashMap<String, Img>,
    vars: &VarState,
    depth: usize,
) -> Outcome {
    if depth > 8 {
        return Outcome {
            picture: Picture::Unknown,
            unknowns: Vec::new(),
            guessed: false,
        };
    }
    match node {
        Node::File(path) => Outcome {
            picture: Picture::File { path: path.clone() },
            unknowns: Vec::new(),
            guessed: false,
        },
        Node::Color(hex) => Outcome {
            picture: Picture::Color { hex: hex.clone() },
            unknowns: Vec::new(),
            guessed: false,
        },
        Node::Null | Node::Unknown => Outcome {
            picture: Picture::Unknown,
            unknowns: Vec::new(),
            guessed: false,
        },
        Node::Ref(name) => {
            let key = collapse_ws(&name.to_lowercase());
            let Some(img) = images.get(&key) else {
                return Outcome {
                    picture: Picture::Unknown,
                    unknowns: Vec::new(),
                    guessed: false,
                };
            };
            if let Some(tree) = &img.tree {
                return resolve_node(tree, images, vars, depth + 1);
            }
            Outcome {
                picture: img.picture.clone(),
                unknowns: Vec::new(),
                guessed: false,
            }
        }
        Node::Pattern(pattern) => match fill_pattern(pattern, vars) {
            Ok(path) => Outcome {
                picture: Picture::File { path },
                unknowns: Vec::new(),
                guessed: false,
            },
            Err(unknowns) => Outcome {
                picture: Picture::Unknown,
                unknowns,
                guessed: false,
            },
        },
        Node::Switch(cases) => resolve_switch(cases, images, vars, depth),
        Node::Composite { w, h, layers } => resolve_composite(*w, *h, layers, images, vars, depth),
        Node::Frames(list, repeat) => resolve_frames(list, *repeat, images, vars, depth),
    }
}

fn resolve_frames(
    list: &[(Node, f32)],
    repeat: bool,
    images: &HashMap<String, Img>,
    vars: &VarState,
    depth: usize,
) -> Outcome {
    let mut frames = Vec::new();
    let mut unknowns = Vec::new();
    let mut guessed = false;
    for (node, seconds) in list {
        let inner = resolve_node(node, images, vars, depth + 1);
        guessed |= inner.guessed;
        for name in inner.unknowns {
            push_unique(&mut unknowns, name);
        }
        let picture = match inner.picture {
            Picture::Unknown => continue,
            Picture::Frames { frames: nested, .. } => match nested.into_iter().next() {
                Some(first) => first.picture,
                None => continue,
            },
            other => other,
        };
        frames.push(AnimFrame {
            picture,
            seconds: *seconds,
        });
    }
    let picture = match frames.len() {
        0 => Picture::Unknown,
        1 => frames.remove(0).picture,
        _ => Picture::Frames { frames, repeat },
    };
    Outcome {
        picture,
        unknowns,
        guessed,
    }
}

fn resolve_switch(
    cases: &[(String, Box<Node>)],
    images: &HashMap<String, Img>,
    vars: &VarState,
    depth: usize,
) -> Outcome {
    let mut chosen = None;
    let mut guessed = false;
    let mut unknowns = Vec::new();
    for (when, node) in cases {
        match vars.eval(when) {
            Some(true) => {
                chosen = Some(node);
                break;
            }
            Some(false) => continue,
            None => {
                guessed = true;
                for name in vars.unknown_names(when) {
                    push_unique(&mut unknowns, name);
                }
                chosen = Some(node);
                break;
            }
        }
    }
    let Some(node) = chosen else {
        return Outcome {
            picture: Picture::Unknown,
            unknowns,
            guessed,
        };
    };
    let mut inner = resolve_node(node, images, vars, depth + 1);
    inner.guessed |= guessed;
    for name in unknowns {
        push_unique(&mut inner.unknowns, name);
    }
    inner
}

fn resolve_composite(
    w: f32,
    h: f32,
    layers: &[PlacedNode],
    images: &HashMap<String, Img>,
    vars: &VarState,
    depth: usize,
) -> Outcome {
    let mut drawn = Vec::new();
    let mut unknowns = Vec::new();
    let mut guessed = false;
    for layer in layers {
        let inner = resolve_node(&layer.node, images, vars, depth + 1);
        guessed |= inner.guessed;
        for name in inner.unknowns {
            push_unique(&mut unknowns, name);
        }
        if matches!(layer.node, Node::Null) {
            continue;
        }
        let x = absolute_offset(layer.x, w);
        let y = absolute_offset(layer.y, h);
        match inner.picture {
            Picture::File { path } => drawn.push(PictureLayer { path, x, y }),
            Picture::Layers { layers, .. } => {
                for child in layers {
                    drawn.push(PictureLayer {
                        path: child.path,
                        x: x + child.x,
                        y: y + child.y,
                    });
                }
            }
            Picture::Frames { frames, .. } => match frames.into_iter().next().map(|f| f.picture) {
                Some(Picture::File { path }) => drawn.push(PictureLayer { path, x, y }),
                Some(Picture::Layers { layers, .. }) => {
                    for child in layers {
                        drawn.push(PictureLayer {
                            path: child.path,
                            x: x + child.x,
                            y: y + child.y,
                        });
                    }
                }
                _ => {}
            },
            Picture::Color { .. } | Picture::Unknown => {}
        }
    }
    let picture = if drawn.is_empty() {
        Picture::Unknown
    } else {
        Picture::Layers {
            w,
            h,
            layers: drawn,
        }
    };
    Outcome {
        picture,
        unknowns,
        guessed,
    }
}

fn absolute_offset(value: f32, span: f32) -> f32 {
    if value > 0.0 && value < 1.0 && span > 1.0 {
        value * span
    } else {
        value
    }
}

fn fill_pattern(pattern: &str, vars: &VarState) -> Result<String, Vec<String>> {
    let mut out = String::new();
    let mut rest = pattern;
    let mut unknowns = Vec::new();
    while let Some(start) = rest.find('[') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find(']') else {
            return Err(unknowns);
        };
        let inner = &after[..end];
        let name: String = inner
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        if name.is_empty() {
            push_unique(&mut unknowns, inner.to_string());
        } else {
            match vars.get(&name).and_then(crate::vars::Value::interpolate) {
                Some(text) => out.push_str(&text),
                None => push_unique(&mut unknowns, name),
            }
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    if !unknowns.is_empty() {
        return Err(unknowns);
    }
    Ok(pattern_path(&out))
}

/// Visits every file path a picture draws, including those inside frames.
fn for_each_path(picture: &mut Picture, visit: &mut dyn FnMut(&mut String)) {
    match picture {
        Picture::File { path } => visit(path),
        Picture::Layers { layers, .. } => {
            for layer in layers {
                visit(&mut layer.path);
            }
        }
        Picture::Frames { frames, .. } => {
            for frame in frames {
                for_each_path(&mut frame.picture, visit);
            }
        }
        Picture::Color { .. } | Picture::Unknown => {}
    }
}

fn pattern_path(filled: &str) -> String {
    let filled = normalize_game_path(filled.trim());
    if filled.is_empty() || filled.contains('/') {
        filled
    } else {
        format!("images/{filled}")
    }
}

fn layer_note(tag: &str, outcome: &Outcome) -> Option<String> {
    let placeholder = matches!(outcome.picture, Picture::Unknown);
    if outcome.unknowns.is_empty() {
        if outcome.guessed {
            return Some(format!(
                "{tag}: a condition could not be read; the preview picked the first possible layer."
            ));
        }
        return None;
    }
    let listed = outcome
        .unknowns
        .iter()
        .map(|name| format!("`{name}`"))
        .collect::<Vec<_>>()
        .join(", ");
    let verb = if outcome.unknowns.len() == 1 {
        "is"
    } else {
        "are"
    };
    if placeholder {
        Some(format!(
            "{tag}: {listed} {verb} not known at this line, so the preview left a placeholder."
        ))
    } else {
        Some(format!(
            "{tag}: {listed} {verb} not known at this line; the preview picked the first possible layer."
        ))
    }
}

fn push_unique(names: &mut Vec<String>, name: String) {
    if !name.is_empty() && !names.iter().any(|have| have == &name) {
        names.push(name);
    }
}

fn kind_label(kind: &str) -> &str {
    match kind {
        "layered" => "layered image",
        "atl" => "ATL transform",
        "dynamic" => "dynamic image",
        "composite" => "composite",
        "condition" => "condition switch",
        "unknown" => "displayable",
        other => other,
    }
}

fn normalize_game_path(path: &str) -> String {
    let path = path.replace('\\', "/");
    path.strip_prefix("./")
        .unwrap_or(&path)
        .strip_prefix("game/")
        .unwrap_or(&path)
        .to_string()
}

/// `black` with no definition is Ren'Py's built-in solid.
fn builtin_picture(name: &str) -> Option<Picture> {
    if collapse_ws(&name.to_lowercase()) == "black" {
        Some(Picture::Color {
            hex: "#000000".into(),
        })
    } else {
        None
    }
}

fn literal_transform(lines: &[String]) -> Option<Place> {
    let mut place = default_place();
    let mut any = false;
    for raw in lines {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line == "pass" {
            continue;
        }
        let (prop, rest) = line.split_once(char::is_whitespace)?;
        let rest = rest.trim().trim_matches(|c: char| c == '(' || c == ')');
        match prop {
            "xalign" => {
                let v = parse_num(rest)?;
                place.xalign = Some(v);
                place.xpos = Some(v);
                place.xanchor = Some(v);
            }
            "yalign" => {
                let v = parse_num(rest)?;
                place.yalign = Some(v);
                place.ypos = Some(v);
                place.yanchor = Some(v);
            }
            "align" => {
                let mut parts = rest
                    .split(|c: char| c == ',' || c.is_whitespace())
                    .filter(|s| !s.is_empty());
                let x = parse_num(parts.next()?)?;
                let y = parse_num(parts.next()?)?;
                place.xalign = Some(x);
                place.yalign = Some(y);
                place.xpos = Some(x);
                place.ypos = Some(y);
                place.xanchor = Some(x);
                place.yanchor = Some(y);
            }
            "xpos" => place.xpos = Some(parse_num(rest)?),
            "ypos" => place.ypos = Some(parse_num(rest)?),
            "xanchor" => place.xanchor = Some(parse_num(rest)?),
            "yanchor" => place.yanchor = Some(parse_num(rest)?),
            "zoom" => place.zoom = Some(parse_num(rest)?),
            "xzoom" => {
                let v = parse_num(rest)?;
                place.flip = v < 0.0;
                if place.zoom == Some(1.0) {
                    place.zoom = Some(v.abs());
                }
            }
            "yzoom" => place.zoom = Some(parse_num(rest)?.abs()),
            "fit" => {
                let fit = rest.trim_matches(|c: char| c == '"' || c == '\'');
                if !FITS.contains(&fit) {
                    return None;
                }
                place.fit = Some(fit.to_string());
            }
            _ => return None,
        }
        any = true;
    }
    if any {
        Some(place)
    } else {
        None
    }
}

const FITS: &[&str] = &["fill", "contain", "cover", "scale-down", "scale-up"];

fn parse_num(text: &str) -> Option<f32> {
    let text = text
        .trim()
        .trim_matches(|c: char| c == ',' || c == '(' || c == ')');
    if text.is_empty() || text.contains(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-')) {
        return None;
    }
    text.parse().ok()
}

fn solid_color(text: &str) -> Option<String> {
    if let Some(hex) = between(text, "Solid(", ")") {
        return hex_of(hex);
    }
    if let Some(hex) = between(text, "= \"", "\"") {
        return hex_of(hex);
    }
    if let Some(hex) = between(text, "= '", "'") {
        return hex_of(hex);
    }
    None
}

fn between<'a>(text: &'a str, open: &str, close: &str) -> Option<&'a str> {
    let start = text.find(open)? + open.len();
    let rest = text.get(start..)?;
    let end = rest.find(close)?;
    Some(
        rest[..end]
            .trim()
            .trim_matches(|c: char| c == '"' || c == '\''),
    )
}

fn hex_of(text: &str) -> Option<String> {
    let raw = text.trim().trim_start_matches('#');
    if raw.is_empty() || raw.len() > 8 || !raw.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    if raw.len() == 3 || raw.len() == 4 {
        let expanded: String = raw.chars().take(3).flat_map(|c| [c, c]).collect();
        return Some(format!("#{expanded}"));
    }
    if raw.len() == 6 || raw.len() == 8 {
        return Some(format!("#{}", &raw[..6]));
    }
    None
}

/// Size set by a `Transform(...)` or `im.Scale(...)` wrapped around the file.
fn image_size(span: &str, screen: (f32, f32)) -> Size {
    let mut size = Size::natural();
    if let Some(args) = call_args(span, "im.Scale(") {
        let parts = split_top(args);
        if parts.len() >= 3 {
            size.w = Some(dim(&parts[1], screen.0, &mut size.guessed));
            size.h = Some(dim(&parts[2], screen.1, &mut size.guessed));
            size.fit = Some("fill".into());
        }
        return size;
    }
    let Some(args) = call_args(span, "Transform(") else {
        return size;
    };
    for part in split_top(args) {
        let Some((key, value)) = part.split_once('=') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "zoom" => {
                if let Some(z) = parse_num(value) {
                    size.zoom = z;
                }
            }
            "xsize" => size.w = Some(dim(value, screen.0, &mut size.guessed)),
            "ysize" => size.h = Some(dim(value, screen.1, &mut size.guessed)),
            "size" | "xysize" => {
                let inner = value.trim_start_matches('(').trim_end_matches(')');
                let pair = split_top(inner);
                if pair.len() == 2 {
                    size.w = Some(dim(&pair[0], screen.0, &mut size.guessed));
                    size.h = Some(dim(&pair[1], screen.1, &mut size.guessed));
                }
            }
            "fit" => {
                size.fit = Some(
                    value
                        .trim_matches(|c: char| c == '"' || c == '\'')
                        .to_string(),
                )
            }
            _ => {}
        }
    }
    size
}

/// A float up to 1.0 is a fraction of the screen, as in Ren'Py. Anything that
/// is not a literal is taken as the full screen.
fn dim(value: &str, full: f32, guessed: &mut bool) -> f32 {
    match parse_num(value) {
        Some(v) if value.contains('.') && v <= 1.0 => v * full,
        Some(v) => v,
        None => {
            *guessed = true;
            full
        }
    }
}

/// Text between `open` (which ends with `(`) and its matching `)`.
fn call_args<'a>(text: &'a str, open: &str) -> Option<&'a str> {
    let start = text.find(open)? + open.len();
    let bytes = text.as_bytes();
    let mut depth = 1i32;
    let mut quote: Option<u8> = None;
    let mut i = start;
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
        } else if c == b'"' || c == b'\'' {
            quote = Some(c);
        } else if c == b'(' {
            depth += 1;
        } else if c == b')' {
            depth -= 1;
            if depth == 0 {
                return Some(&text[start..i]);
            }
        }
        i += 1;
    }
    None
}

/// Split on commas that are outside brackets and strings.
fn split_top(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut depth = 0i32;
    let mut quote: Option<char> = None;
    for c in text.chars() {
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
            cur.push(c);
            continue;
        }
        match c {
            '"' | '\'' => {
                quote = Some(c);
                cur.push(c);
            }
            '(' | '[' | '{' => {
                depth += 1;
                cur.push(c);
            }
            ')' | ']' | '}' => {
                depth -= 1;
                cur.push(c);
            }
            ',' if depth == 0 => {
                out.push(cur.trim().to_string());
                cur.clear();
            }
            _ => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

/// Display name and colour of `Character("Name", color="#hex")`.
fn character_bits(value: &str) -> (Option<String>, Option<String>) {
    let Some(args) = call_args(value, "Character(") else {
        return (None, None);
    };
    let parts = split_top(args);
    let mut name = parts.first().and_then(|p| quoted(p));
    let mut color = None;
    for part in &parts {
        let Some((key, val)) = part.split_once('=') else {
            continue;
        };
        match key.trim() {
            "name" => name = quoted(val),
            "color" | "who_color" => color = quoted(val),
            _ => {}
        }
    }
    (name, color)
}

fn quoted(text: &str) -> Option<String> {
    let t = text.trim();
    let q = t.chars().next().filter(|c| *c == '"' || *c == '\'')?;
    let rest = &t[1..];
    let end = rest.find(q)?;
    Some(rest[..end].to_string())
}

/// Text with `{tags}` removed, `{{` kept as `{`, and `\n` as a line break.
fn plain_text(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
                out.push('{');
            }
            '{' => {
                for t in chars.by_ref() {
                    if t == '}' {
                        break;
                    }
                }
            }
            '\\' if chars.peek() == Some(&'n') => {
                chars.next();
                out.push('\n');
            }
            '\\' if chars
                .peek()
                .is_some_and(|n| *n == '"' || *n == '\'' || *n == '\\') =>
            {
                if let Some(n) = chars.next() {
                    out.push(n);
                }
            }
            _ => out.push(c),
        }
    }
    out
}

fn strip_comment(line: &str) -> &str {
    let mut quote: Option<char> = None;
    for (i, c) in line.char_indices() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == '"' || c == '\'' => quote = Some(c),
            None if c == '#' => return &line[..i],
            None => {}
        }
    }
    line
}

struct GuiDefs {
    values: HashMap<String, String>,
}

impl GuiDefs {
    fn raw(&self, name: &str) -> Option<&str> {
        let mut value = self.values.get(name)?.as_str();
        for _ in 0..4 {
            match value.strip_prefix("gui.") {
                Some(other) => value = self.values.get(other)?.as_str(),
                None => break,
            }
        }
        Some(value)
    }

    /// Pixels, or a fraction of `full` for floats up to 1.0.
    fn px(&self, name: &str, default: f32, full: f32) -> f32 {
        let Some(raw) = self.raw(name) else {
            return default;
        };
        match parse_num(raw) {
            Some(v) if raw.contains('.') && v.abs() <= 1.0 => v * full,
            Some(v) => v,
            None => default,
        }
    }

    fn frac(&self, name: &str, default: f32) -> f32 {
        self.raw(name).and_then(parse_num).unwrap_or(default)
    }

    fn color(&self, name: &str, default: &str) -> String {
        self.raw(name)
            .and_then(quoted)
            .and_then(|c| hex_of(&c))
            .unwrap_or_else(|| default.to_string())
    }
}

/// Values from `define gui.* = ...`. Missing ones use the Ren'Py 8 template
/// scaled from 1920 wide.
fn read_gui(
    project: &Project,
    lines: &HashMap<String, Vec<String>>,
    width: f32,
    height: f32,
) -> StageGui {
    let mut values = HashMap::new();
    let mut choice_ypos = None;
    for file_lines in lines.values() {
        let mut in_choice_vbox = false;
        for raw in file_lines {
            let line = strip_comment(raw);
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("define gui.") {
                if let Some((name, value)) = rest.split_once('=') {
                    values.insert(name.trim().to_string(), value.trim().to_string());
                }
            }
            if trimmed.starts_with("style choice_vbox") && trimmed.ends_with(':') {
                in_choice_vbox = true;
                continue;
            }
            if in_choice_vbox {
                if !trimmed.is_empty() && !raw.starts_with(|c: char| c.is_whitespace()) {
                    in_choice_vbox = false;
                } else if let Some(v) = trimmed.strip_prefix("ypos ") {
                    if choice_ypos.is_none() {
                        choice_ypos = parse_num(v).map(|n| {
                            if v.contains('.') && n <= 1.0 {
                                n * height
                            } else {
                                n
                            }
                        });
                    }
                }
            }
        }
    }
    let gui = GuiDefs { values };
    let k = width / 1920.0;
    let text_size = gui.px("text_size", 33.0 * k, height);
    let idle = gui.color("idle_color", "#888888");
    StageGui {
        textbox: asset(project, "gui/textbox.png"),
        namebox: asset(project, "gui/namebox.png"),
        choice: asset(project, "gui/button/choice_idle_background.png"),
        textbox_height: gui.px("textbox_height", 278.0 * k, height),
        textbox_yalign: gui.frac("textbox_yalign", 1.0),
        name_xpos: gui.px("name_xpos", 360.0 * k, width),
        name_ypos: gui.px("name_ypos", 0.0, height),
        name_xalign: gui.frac("name_xalign", 0.0),
        dialogue_xpos: gui.px("dialogue_xpos", 402.0 * k, width),
        dialogue_ypos: gui.px("dialogue_ypos", 75.0 * k, height),
        dialogue_width: gui.px("dialogue_width", 1116.0 * k, width),
        text_size,
        name_text_size: gui.px("name_text_size", 45.0 * k, height),
        text_color: gui.color("text_color", "#ffffff"),
        accent_color: gui.color("accent_color", "#0099cc"),
        choice_width: gui.px("choice_button_width", 1185.0 * k, width),
        choice_text_size: gui.px("choice_button_text_size", text_size, height),
        choice_spacing: gui.px("choice_spacing", 33.0 * k, height),
        choice_text_color: gui.color("choice_button_text_idle_color", &idle),
        choice_ypos: choice_ypos.unwrap_or(height * 0.375),
    }
}

/// `rel` when the game has it as a loose file or inside an archive.
fn asset(project: &Project, rel: &str) -> Option<String> {
    if project.game_dir.join(rel).is_file() {
        return Some(rel.to_string());
    }
    let found = project
        .archives
        .iter()
        .filter_map(|a| a.archive.as_ref())
        .any(|a| {
            a.entries
                .keys()
                .any(|name| name.replace('\\', "/").eq_ignore_ascii_case(rel))
        });
    found.then(|| rel.to_string())
}

fn screen_of(
    project: &Project,
    lines: &HashMap<String, Vec<String>>,
) -> (u32, u32, Option<String>) {
    let (width, height, note) = screen_size(lines);
    if note.is_none() {
        return (width, height, None);
    }
    if let Some(run) = &project.stage_images {
        if let (Some(w), Some(h)) = (run.dump.screen_width, run.dump.screen_height) {
            if w > 0 && h > 0 {
                return (w, h, None);
            }
        }
    }
    (width, height, note)
}

fn screen_size(lines: &HashMap<String, Vec<String>>) -> (u32, u32, Option<String>) {
    let mut width = None;
    let mut height = None;
    for file_lines in lines.values() {
        for line in file_lines {
            let trimmed = line.trim();
            if trimmed.starts_with('#') {
                continue;
            }
            if let Some((w, h)) = gui_init(trimmed) {
                return (w, h, None);
            }
            if width.is_none() {
                width = assigned_int(trimmed, "config.screen_width");
            }
            if height.is_none() {
                height = assigned_int(trimmed, "config.screen_height");
            }
        }
    }
    if let (Some(w), Some(h)) = (width, height) {
        return (w, h, None);
    }
    (
        DEFAULT_W,
        DEFAULT_H,
        Some("Screen size is 1920×1080 because no gui.init was found.".into()),
    )
}

fn gui_init(line: &str) -> Option<(u32, u32)> {
    let at = line.find("gui.init")?;
    let rest = line[at + "gui.init".len()..].trim_start();
    let rest = rest.strip_prefix('(')?.trim_start();
    let (w, used) = take_int(rest)?;
    let rest = rest[used..].trim_start().strip_prefix(',')?.trim_start();
    let (h, _) = take_int(rest)?;
    Some((w, h))
}

fn assigned_int(line: &str, name: &str) -> Option<u32> {
    let at = line.find(name)?;
    let rest = line[at + name.len()..].trim_start();
    let rest = rest.strip_prefix('=')?.trim_start();
    take_int(rest).map(|(n, _)| n)
}

fn take_int(text: &str) -> Option<(u32, usize)> {
    let end = text
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(text.len());
    if end == 0 {
        return None;
    }
    Some((text[..end].parse().ok()?, end))
}

fn file_lines(file: &SourceFile) -> Vec<String> {
    if let Some(text) = &file.source {
        return text.lines().map(|l| l.to_string()).collect();
    }
    fs::read_to_string(&file.abs)
        .map(|text| text.lines().map(|l| l.to_string()).collect())
        .unwrap_or_default()
}

fn find_label<'a>(stmts: &'a [Stmt], line: u32) -> Option<&'a Stmt> {
    let mut best: Option<&Stmt> = None;
    fn walk<'a>(stmts: &'a [Stmt], line: u32, best: &mut Option<&'a Stmt>) {
        for stmt in stmts {
            if let Kind::Label { body, .. } = &stmt.kind {
                if stmt.line <= line && line <= stmt.end_line {
                    let tighter =
                        best.is_none_or(|cur| stmt.end_line - stmt.line <= cur.end_line - cur.line);
                    if tighter {
                        *best = Some(stmt);
                    }
                    walk(body, line, best);
                }
            }
        }
    }
    walk(stmts, line, &mut best);
    best
}

/// Replay one label in source order. Branches that do not contain `line`,
/// and menus that finished before it, are not applied.
fn replay_label(stage: &mut Replay<'_>, file: &str, stmts: &[Stmt], line: u32) {
    for stmt in stmts {
        if stmt.line > line {
            break;
        }
        match &stmt.kind {
            Kind::If { branches } => {
                if branches.iter().any(|b| presents(&b.body)) {
                    for branch in branches {
                        stage.record_names(file, branch.line, &branch.cond);
                    }
                }
                let pairs: Vec<(&str, &str)> = branches
                    .iter()
                    .map(|b| (branch_how(b.kind), b.cond.as_str()))
                    .collect();
                let verdicts = vars::judge(&pairs, &stage.basis);
                if let Some(idx) = branches
                    .iter()
                    .position(|b| b.line <= line && line <= b.end_line)
                {
                    if let Some(kind) = note_kind(&verdicts, idx) {
                        // The caret is in this branch, so it was not a default pick.
                        let kind = if kind == NoteKind::Default {
                            NoteKind::Undecided
                        } else {
                            kind
                        };
                        stage.note_branch(file, branches, idx, kind);
                    }
                    replay_label(stage, file, &branches[idx].body, line);
                } else if stmt.end_line < line {
                    if let Some(idx) = vars::preferred(&verdicts) {
                        if verdicts[idx] == vars::Verdict::Unknown {
                            stage.note_branch(file, branches, idx, NoteKind::Default);
                        }
                        replay_label(stage, file, &branches[idx].body, u32::MAX);
                    } else {
                        stage.note(&format!(
                            "{file}:{} is a condition before this line, so its branch was skipped.",
                            stmt.line
                        ));
                    }
                }
            }
            Kind::Menu { choices, .. } => {
                if let Some(choice) = choices
                    .iter()
                    .find(|c| c.line <= line && line <= c.end_line)
                {
                    replay_label(stage, file, &choice.body, line);
                } else if stmt.end_line < line {
                    stage.note(&format!(
                        "{file}:{} is a menu before this line, so no choice was followed.",
                        stmt.line
                    ));
                }
            }
            Kind::While { body, .. } => {
                if body.iter().any(|s| s.line <= line && line <= s.end_line)
                    || (stmt.line <= line && line <= stmt.end_line && line > stmt.line)
                {
                    if stmt.line <= line {
                        stage.apply_line(file, stmt.line);
                    }
                    replay_label(stage, file, body, line);
                }
            }
            Kind::Label { body, .. } => {
                if stmt.line <= line {
                    stage.apply_line(file, stmt.line);
                }
                if stmt.line <= line && line <= stmt.end_line {
                    replay_label(stage, file, body, line);
                }
            }
            _ => {
                if stmt.line <= line {
                    stage.apply_line(file, stmt.line);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Instant;

    fn estimate(project: &Project, file: &str, line: u32) -> Result<StageEstimate, String> {
        super::estimate(project, file, line, &[])
    }

    fn scratch(files: &[(&str, &str)]) -> (PathBuf, Project) {
        static N: AtomicU64 = AtomicU64::new(1);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("vnide-stage-{}-{n}", std::process::id()));
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

    fn tags(est: &StageEstimate) -> Vec<&str> {
        est.sprites.iter().map(|s| s.tag.as_str()).collect()
    }

    #[test]
    fn scene_clears_the_layer_and_show_replaces_a_tag() {
        let (root, project) = scratch(&[(
            "script.rpy",
            "\
image bg room = \"bg/room.png\"
image bg other = \"bg/other.png\"
image eileen happy = \"eileen.png\"
label start:
    show eileen happy at left
    scene bg room
    scene bg other
    \"done\"
",
        )]);
        let est = estimate(&project, "script.rpy", 8).unwrap();
        assert_eq!(est.via, "path");
        assert_eq!(tags(&est), vec!["bg"]);
        assert_eq!(
            est.sprites[0].picture,
            Picture::File {
                path: "bg/other.png".into()
            }
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn attributes_hide_as_behind_zorder_and_sides() {
        let (root, project) = scratch(&[(
            "script.rpy",
            "\
image eileen happy blush = \"blush.png\"
image eileen happy = \"happy.png\"
image lucy = \"lucy.png\"
label start:
    show eileen happy
    show eileen blush
    show lucy behind eileen
    hide lucy
    hide eileen
    show eileen as sister at right
    show lucy zorder 4
    \"done\"
",
        )]);
        let mid = estimate(&project, "script.rpy", 6).unwrap();
        assert_eq!(mid.sprites[0].name, "eileen happy blush");
        assert_eq!(
            mid.sprites[0].picture,
            Picture::File {
                path: "blush.png".into()
            }
        );
        let behind = estimate(&project, "script.rpy", 7).unwrap();
        assert_eq!(tags(&behind), vec!["lucy", "eileen"]);
        let hidden = estimate(&project, "script.rpy", 9).unwrap();
        assert!(tags(&hidden).is_empty(), "{:?}", tags(&hidden));
        let renamed = estimate(&project, "script.rpy", 10).unwrap();
        assert_eq!(renamed.sprites[0].tag, "sister");
        assert_eq!(renamed.sprites[0].xpos, Some(1.0));
        assert_eq!(renamed.sprites[0].ypos, Some(1.0));
        let raised = estimate(&project, "script.rpy", 12).unwrap();
        assert_eq!(raised.sprites.last().map(|s| s.tag.as_str()), Some("lucy"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_missing_attribute_replaces_the_sprite_with_a_placeholder() {
        let (root, project) = scratch(&[(
            "script.rpy",
            "\
image eileen happy = \"happy.png\"
label start:
    show eileen happy
    show eileen sad
    \"done\"
",
        )]);
        let est = estimate(&project, "script.rpy", 5).unwrap();
        assert_eq!(est.sprites[0].name, "eileen sad");
        assert_eq!(est.sprites[0].picture, Picture::Unknown);
        assert!(est
            .notes
            .iter()
            .any(|n| n.contains("no image for eileen sad")));
        assert!(est.notes.iter().all(|n| !n.contains("still showing")));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_literal_transform_is_read_and_atl_is_a_note() {
        let (root, project) = scratch(&[(
            "script.rpy",
            "\
image eileen = \"eileen.png\"
transform near:
    xalign 0.2
    yalign 1.0
transform slide:
    linear 0.2 xalign 1.0
label start:
    show eileen at near
    \"posed\"
    show eileen at slide
    \"moved\"
",
        )]);
        let posed = estimate(&project, "script.rpy", 9).unwrap();
        assert_eq!(posed.sprites[0].xpos, Some(0.2));
        let moved = estimate(&project, "script.rpy", 11).unwrap();
        assert!(moved.notes.iter().any(|n| n.contains("slide")));
        assert_eq!(moved.sprites[0].xpos, Some(0.5));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn an_atl_frame_animation_becomes_frames() {
        let (root, project) = scratch(&[(
            "script.rpy",
            "\
image fa = \"fa.png\"
image fb = \"fb.png\"
image anim:
    \"fa\" with dissolve
    pause 0.25
    \"fb\"
    pause 0.2
    repeat
label start:
    scene anim
    \"look\"
",
        )]);
        let est = estimate(&project, "script.rpy", 11).unwrap();
        assert_eq!(
            est.sprites[0].picture,
            Picture::Frames {
                frames: vec![
                    AnimFrame {
                        picture: Picture::File {
                            path: "fa.png".into()
                        },
                        seconds: 0.25
                    },
                    AnimFrame {
                        picture: Picture::File {
                            path: "fb.png".into()
                        },
                        seconds: 0.2
                    },
                ],
                repeat: true,
            }
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn atl_frames_can_name_files_and_one_frame_is_plain() {
        let (root, project) = scratch(&[(
            "script.rpy",
            "\
image pair:
    \"images/a.webp\" with fps
    pause 0.3
    \"images/b.webp\"
    pause 0.1
image single:
    \"images/a.webp\"
    pause 1.0
label start:
    scene pair
    \"one\"
    scene single
    \"two\"
",
        )]);
        let pair = estimate(&project, "script.rpy", 11).unwrap();
        match &pair.sprites[0].picture {
            Picture::Frames { frames, repeat } => {
                assert_eq!(frames.len(), 2);
                assert_eq!(
                    frames[1].picture,
                    Picture::File {
                        path: "images/b.webp".into()
                    }
                );
                assert!(!repeat);
            }
            other => panic!("expected frames, got {other:?}"),
        }
        let single = estimate(&project, "script.rpy", 13).unwrap();
        assert_eq!(
            single.sprites[0].picture,
            Picture::File {
                path: "images/a.webp".into()
            }
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn an_atl_block_without_pictures_stays_unknown() {
        let (root, project) = scratch(&[(
            "script.rpy",
            "\
image spin:
    rotate 0
    linear 1.0 rotate 360
    repeat
label start:
    scene spin
    \"x\"
",
        )]);
        let est = estimate(&project, "script.rpy", 7).unwrap();
        assert_eq!(est.sprites[0].picture, Picture::Unknown);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_jump_and_a_menu_choose_the_branch_that_reaches_the_line() {
        let (root, project) = scratch(&[(
            "script.rpy",
            "\
image bg room = \"room.png\"
image bg wrong = \"wrong.png\"
label start:
    menu:
        \"left\":
            jump finale
        \"right\":
            scene bg wrong
            \"no\"
label finale:
    scene bg room
    \"done\"
",
        )]);
        let est = estimate(&project, "script.rpy", 11).unwrap();
        assert_eq!(est.via, "path");
        assert_eq!(est.decisions, 1);
        assert_eq!(
            est.sprites[0].picture,
            Picture::File {
                path: "room.png".into()
            }
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn bare_atl_frame_names_resolve_under_images() {
        let (root, project) = scratch(&[
            (
                "script.rpy",
                "\
image anim:
    \"fa.webp\"
    pause 0.25
    \"fb.webp\"
    pause 0.2
    repeat
label start:
    scene anim
    \"look\"
",
            ),
            ("images/fa.webp", "x"),
            ("images/fb.webp", "x"),
        ]);
        let est = estimate(&project, "script.rpy", 9).unwrap();
        match &est.sprites[0].picture {
            Picture::Frames { frames, .. } => {
                assert_eq!(
                    frames[0].picture,
                    Picture::File {
                        path: "images/fa.webp".into()
                    }
                );
                assert_eq!(
                    frames[1].picture,
                    Picture::File {
                        path: "images/fb.webp".into()
                    }
                );
            }
            other => panic!("expected frames, got {other:?}"),
        }
        assert!(!est.notes.iter().any(|n| n.contains("was not found")));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn find_asset_prefers_the_exact_path_then_images_then_archives() {
        let (root, _opened) = scratch(&[
            ("script.rpy", "label start:\n    \"a\"\n"),
            ("both.png", "x"),
            ("images/both.png", "x"),
            ("images/only.png", "x"),
        ]);
        let game = root.join("game");
        let mut writer = crate::rpa::ArchiveWriter::create(&game.join("art.rpa")).unwrap();
        writer.add_bytes("images/Packed.PNG", b"png").unwrap();
        writer.finish().unwrap();
        let project = Project::open(&root).unwrap();
        assert_eq!(project.find_asset("both.png").as_deref(), Some("both.png"));
        assert_eq!(
            project.find_asset("only.png").as_deref(),
            Some("images/only.png")
        );
        assert_eq!(
            project.find_asset("images/only.png").as_deref(),
            Some("images/only.png")
        );
        assert_eq!(
            project.find_asset("packed.png").as_deref(),
            Some("images/Packed.PNG")
        );
        assert_eq!(project.find_asset("nope.png"), None);
        assert_eq!(project.find_asset("../escape.png"), None);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_picture_that_is_nowhere_keeps_its_name_and_is_reported_once() {
        let (root, project) = scratch(&[(
            "script.rpy",
            "\
image bg room = \"gone.png\"
label start:
    scene bg room
    \"a\"
",
        )]);
        let est = estimate(&project, "script.rpy", 4).unwrap();
        assert_eq!(
            est.sprites[0].picture,
            Picture::File {
                path: "gone.png".into()
            }
        );
        let notes: Vec<_> = est
            .notes
            .iter()
            .filter(|n| n.contains("`gone.png` was not found"))
            .collect();
        assert_eq!(notes.len(), 1);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn an_unreachable_label_is_estimated_on_its_own() {
        let (root, project) = scratch(&[(
            "script.rpy",
            "\
image bg room = \"room.png\"
label start:
    \"nope\"
    return
label secret:
    scene bg room
    \"here\"
",
        )]);
        let est = estimate(&project, "script.rpy", 6).unwrap();
        assert_eq!(est.via, "label");
        assert_eq!(
            est.sprites[0].picture,
            Picture::File {
                path: "room.png".into()
            }
        );
        assert!(est.notes.iter().any(|n| n.contains("Earlier labels")));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn images_resolve_from_files_solids_and_archives() {
        let (root, _opened) = scratch(&[
            ("script.rpy", "image bg night = Solid(\"#010203\")\nlabel start:\n    scene bg room\n    \"a\"\n    scene bg night\n    \"b\"\n    scene black\n    \"c\"\n"),
            ("images/bg room.png", "not-really-a-png"),
        ]);
        let game = root.join("game");
        let mut writer = crate::rpa::ArchiveWriter::create(&game.join("images.rpa")).unwrap();
        writer.add_bytes("images/bg hall.png", b"png").unwrap();
        writer.finish().unwrap();
        // Re-open so the archive is part of the project. The loose image stays.
        let project = Project::open(&root).unwrap();
        let room = estimate(&project, "script.rpy", 4).unwrap();
        assert_eq!(
            room.sprites[0].picture,
            Picture::File {
                path: "images/bg room.png".into()
            }
        );
        let night = estimate(&project, "script.rpy", 6).unwrap();
        assert_eq!(
            night.sprites[0].picture,
            Picture::Color {
                hex: "#010203".into()
            }
        );
        let black = estimate(&project, "script.rpy", 8).unwrap();
        assert_eq!(
            black.sprites[0].picture,
            Picture::Color {
                hex: "#000000".into()
            }
        );
        let (root2, _project2) =
            scratch(&[("script.rpy", "label start:\n    scene bg hall\n    \"x\"\n")]);
        let mut writer =
            crate::rpa::ArchiveWriter::create(&root2.join("game").join("images.rpa")).unwrap();
        writer.add_bytes("images/bg hall.png", b"png").unwrap();
        writer.finish().unwrap();
        let project2 = Project::open(&root2).unwrap();
        let hall = estimate(&project2, "script.rpy", 3).unwrap();
        assert_eq!(
            hall.sprites[0].picture,
            Picture::File {
                path: "images/bg hall.png".into()
            }
        );
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&root2);
    }

    #[test]
    fn transform_and_scale_wrappers_size_the_image() {
        let (root, project) = scratch(&[
            ("gui.rpy", "init python:\n    gui.init(2560, 1440)\n"),
            (
                "script.rpy",
                "\
image bg hub = Transform(\"bg/hub.png\", xsize=bg_width, ysize=bg_height, fit=\"fill\")
image bg loft = Transform(\"bg/loft.jpg\", zoom=0.5)
image bg old = im.Scale(\"bg/old.png\", 1280, 720)
image mia = Transform(\"mia.png\", fit=\"contain\")
transform char_left:
    xalign 0.15
    yalign 1.0
    xzoom -0.95
    fit \"contain\"
label start:
    scene bg hub
    \"a\"
    scene bg loft
    show mia at char_left
    \"b\"
    scene bg old
    \"c\"
",
            ),
        ]);
        let hub = estimate(&project, "script.rpy", 12).unwrap();
        let bg = &hub.sprites[0];
        assert_eq!(
            (bg.size_w, bg.size_h, bg.fit.as_deref()),
            (Some(2560.0), Some(1440.0), Some("fill"))
        );
        assert!(hub
            .notes
            .iter()
            .any(|n| n.contains("bg hub") && n.contains("variable")));
        let loft = estimate(&project, "script.rpy", 15).unwrap();
        assert_eq!(loft.sprites[0].zoom, Some(0.5));
        let mia = &loft.sprites[1];
        assert_eq!(mia.fit.as_deref(), Some("contain"));
        assert_eq!(mia.size_w, None);
        assert_eq!(mia.place_fit.as_deref(), Some("contain"));
        assert!(mia.flip);
        assert_eq!(mia.zoom, Some(0.95));
        assert_eq!(mia.xpos, Some(0.15));
        assert!(loft.notes.iter().all(|n| !n.contains("char_left")));
        let old = estimate(&project, "script.rpy", 17).unwrap();
        assert_eq!(
            (old.sprites[0].size_w, old.sprites[0].size_h),
            (Some(1280.0), Some(720.0))
        );
        assert!(old.notes.iter().all(|n| !n.contains("bg old")));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn say_lines_and_menus_fill_the_window_and_choices() {
        let (root, project) = scratch(&[
            (
                "gui.rpy",
                "\
init python:
    gui.init(2560, 1440)
define gui.text_size = 44
define gui.choice_button_text_size = gui.text_size
define gui.text_color = '#ffffff' # white
define gui.textbox_height = 370
style choice_vbox:
    xalign 0.5
    ypos 540
",
            ),
            (
                "script.rpy",
                "\
define narrator = Character(\"Narrator\", color=\"#ffffff\")
define lux = Character(\"Lux\", color=\"#ff69b4\")
label start:
    lux \"Hi {i}there{/i}. This line is long enough that the parser would shorten it for the outline, but the say window needs every word of it, right to the very end.\"
    \"Plain narration.\"
    menu:
        \"What now?\"
        \"Go\":
            pass
        \"Stay\" if flag:
            pass
",
            ),
            ("gui/textbox.png", "png"),
        ]);
        let said = estimate(&project, "script.rpy", 4).unwrap();
        let say = said.say.clone().unwrap();
        assert_eq!(say.who.as_deref(), Some("Lux"));
        assert_eq!(say.who_color.as_deref(), Some("#ff69b4"));
        assert!(say.what.starts_with("Hi there. This line"));
        assert!(say.what.ends_with("right to the very end."));
        assert!(said.choices.is_empty());
        assert_eq!(said.gui.textbox.as_deref(), Some("gui/textbox.png"));
        assert_eq!(said.gui.namebox, None);
        assert_eq!(said.gui.text_size, 44.0);
        assert_eq!(said.gui.choice_text_size, 44.0);
        assert_eq!(said.gui.textbox_height, 370.0);
        assert_eq!(said.gui.text_color, "#ffffff");
        assert_eq!(said.gui.choice_ypos, 540.0);
        let narration = estimate(&project, "script.rpy", 5).unwrap();
        assert_eq!(narration.say.unwrap().who.as_deref(), Some("Narrator"));
        for line in [6, 7, 8] {
            let menu = estimate(&project, "script.rpy", line).unwrap();
            assert_eq!(
                menu.say.as_ref().map(|s| s.what.as_str()),
                Some("What now?")
            );
            let texts: Vec<_> = menu
                .choices
                .iter()
                .map(|c| (c.text.as_str(), c.conditional, c.line))
                .collect();
            assert_eq!(texts, vec![("Go", false, 8), ("Stay", true, 10)]);
        }
        let scene_line = estimate(&project, "script.rpy", 3).unwrap();
        assert!(scene_line.say.is_none());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn notes_about_a_sprite_leave_with_it() {
        let (root, project) = scratch(&[(
            "script.rpy",
            "label start:\n    show rae default at char_center\n    \"a\"\n    scene black\n    \"b\"\n",
        )]);
        let shown = estimate(&project, "script.rpy", 3).unwrap();
        assert!(shown.notes.iter().any(|n| n.contains("rae default")));
        assert!(shown.notes.iter().any(|n| n.contains("char_center")));
        let cleared = estimate(&project, "script.rpy", 5).unwrap();
        assert!(cleared
            .notes
            .iter()
            .all(|n| !n.contains("rae") && !n.contains("char_center")));
        let after = estimate(&project, "script.rpy", 6).unwrap();
        assert_eq!(after.say.as_ref().map(|s| s.what.as_str()), Some("b"));
        assert_eq!(tags(&after), vec!["black"]);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn gui_init_sets_the_screen_size() {
        let (root, project) = scratch(&[(
            "script.rpy",
            "init python:\n    gui.init(1280, 720)\nlabel start:\n    \"hi\"\n",
        )]);
        let est = estimate(&project, "script.rpy", 4).unwrap();
        assert_eq!((est.width, est.height), (1280, 720));
        assert!(est.notes.iter().all(|n| !n.contains("1920")));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn python_show_and_expression_are_notes() {
        let (root, project) = scratch(&[(
            "script.rpy",
            "label start:\n    $ renpy.show(\"eileen\")\n    show expression \"eileen\"\n    \"done\"\n",
        )]);
        let est = estimate(&project, "script.rpy", 4).unwrap();
        assert!(est.sprites.is_empty());
        assert!(est.notes.iter().any(|n| n.contains("renpy.show")));
        assert!(est.notes.iter().any(|n| n.contains("expression")));
        let _ = fs::remove_dir_all(root);
    }

    fn engine_image(
        name: &str,
        kind: &str,
        file: Option<&str>,
        reference: Option<&str>,
    ) -> crate::engine::EngineImage {
        crate::engine::EngineImage {
            name: name.into(),
            kind: kind.into(),
            file: file.map(|p| p.to_string()),
            reference: reference.map(|p| p.to_string()),
            ..crate::engine::EngineImage::default()
        }
    }

    #[test]
    fn engine_aliases_resolve_and_a_fresh_dump_replaces_the_script() {
        let (root, mut project) = scratch(&[(
            "script.rpy",
            "image rae school default = \"old.png\"\ndefine e = Character(\"Script\", color=\"#111111\")\nlabel start:\n    show rae default\n    e \"Hi\"\n",
        )]);
        project.set_stage_images(crate::engine::ImageRun {
            key: project.init_key(),
            dump: crate::engine::ImageDump {
                images: vec![
                    engine_image("rae school default", "image", Some("images/new.png"), None),
                    engine_image("rae default", "ref", None, Some("rae school default")),
                ],
                characters: vec![crate::engine::EngineCharacter {
                    var: "e".into(),
                    name: Some("Engine".into()),
                    color: Some("#ff00aa".into()),
                }],
                screen_width: Some(1280),
                screen_height: Some(720),
                callbacks: true,
            },
            ..crate::engine::ImageRun::default()
        });
        let est = estimate(&project, "script.rpy", 5).unwrap();
        assert_eq!(
            est.sprites[0].picture,
            Picture::File {
                path: "images/new.png".into()
            }
        );
        assert_eq!(
            est.say.as_ref().map(|s| s.who.as_deref()),
            Some(Some("Script"))
        );
        assert!(est.notes.iter().any(|n| n.contains("attributes in Python")));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_stale_dump_keeps_a_newer_image_statement() {
        let (root, mut project) = scratch(&[(
            "script.rpy",
            "image rae school default = \"old.png\"\nlabel start:\n    show rae default\n    \"x\"\n",
        )]);
        project.set_stage_images(crate::engine::ImageRun {
            key: "not-the-current-scripts".into(),
            dump: crate::engine::ImageDump {
                images: vec![
                    engine_image("rae school default", "image", Some("images/new.png"), None),
                    engine_image("rae default", "ref", None, Some("rae school default")),
                    engine_image("rae outfit", "layered", None, None),
                ],
                ..crate::engine::ImageDump::default()
            },
            ..crate::engine::ImageRun::default()
        });
        let est = estimate(&project, "script.rpy", 3).unwrap();
        assert_eq!(
            est.sprites[0].picture,
            Picture::File {
                path: "old.png".into()
            }
        );
        let (root2, mut project2) = scratch(&[(
            "script.rpy",
            "label start:\n    show rae outfit\n    \"x\"\n",
        )]);
        project2.set_stage_images(project.stage_images.clone().unwrap());
        let layered = estimate(&project2, "script.rpy", 2).unwrap();
        assert_eq!(layered.sprites[0].picture, Picture::Unknown);
        assert!(layered.notes.iter().any(|n| n.contains("layered image")));
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(root2);
    }

    #[test]
    fn attributes_stick_drop_and_a_tie_is_ambiguous() {
        let (root, project) = scratch(&[(
            "script.rpy",
            "\
image rae school default = \"default.png\"
image rae school angry = \"angry.png\"
label start:
    show rae school default
    show rae angry
    \"done\"
",
        )]);
        let sticky = estimate(&project, "script.rpy", 5).unwrap();
        assert_eq!(
            sticky.sprites[0].picture,
            Picture::File {
                path: "angry.png".into()
            }
        );
        let _ = fs::remove_dir_all(root);

        let (root, project) = scratch(&[(
            "script.rpy",
            "\
image rae school angry = \"angry.png\"
image rae angry = \"just-angry.png\"
label start:
    show rae school angry
    show rae -school
    \"done\"
",
        )]);
        let dropped = estimate(&project, "script.rpy", 5).unwrap();
        assert_eq!(
            dropped.sprites[0].picture,
            Picture::File {
                path: "just-angry.png".into()
            }
        );
        let _ = fs::remove_dir_all(root);

        let (root, project) = scratch(&[(
            "script.rpy",
            "\
image eileen a b = \"ab.png\"
image eileen a c = \"ac.png\"
image eileen b c = \"bc.png\"
label start:
    show eileen b c
    show eileen a
    \"done\"
",
        )]);
        let ambiguous = estimate(&project, "script.rpy", 6).unwrap();
        assert_eq!(ambiguous.sprites[0].picture, Picture::Unknown);
        let note = ambiguous
            .notes
            .iter()
            .find(|n| n.contains("ambiguous"))
            .cloned()
            .unwrap_or_default();
        assert!(
            note.contains("eileen a b") && note.contains("eileen a c"),
            "{note}"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn the_engine_screen_size_fills_a_missing_gui_init() {
        let (root, mut project) = scratch(&[("script.rpy", "label start:\n    \"hi\"\n")]);
        project.set_stage_images(crate::engine::ImageRun {
            key: project.init_key(),
            dump: crate::engine::ImageDump {
                screen_width: Some(1280),
                screen_height: Some(720),
                ..crate::engine::ImageDump::default()
            },
            ..crate::engine::ImageRun::default()
        });
        let est = estimate(&project, "script.rpy", 2).unwrap();
        assert_eq!((est.width, est.height), (1280, 720));
        assert!(est.notes.iter().all(|n| !n.contains("1920")));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_long_script_stays_quick() {
        let mut body = String::from("label start:\n");
        for i in 0..2000 {
            body.push_str(&format!("    \"line {i}\"\n"));
        }
        body.push_str("    scene black\n    \"end\"\n");
        let (root, project) = scratch(&[("script.rpy", &body)]);
        let started = Instant::now();
        let est = estimate(&project, "script.rpy", 2003).unwrap();
        let ms = started.elapsed().as_millis();
        eprintln!("stage estimate of 2000 lines took {ms} ms");
        assert!(est.sprites.iter().any(|s| s.name == "black"));
        assert!(ms < 500, "stage estimate took {ms} ms");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_later_line_reuses_the_path_search() {
        let mut body = String::from("label start:\n");
        for i in 0..20000 {
            body.push_str(&format!("    \"line {i}\"\n"));
        }
        let (root, project) = scratch(&[("script.rpy", &body)]);
        let last = 20001u32;
        let far = estimate(&project, "script.rpy", last).unwrap();
        let walked = replay::stored_walks(&project.stage_index().prepared);
        assert!(walked > 1, "the far line did not fill the search");
        let near = estimate(&project, "script.rpy", last - 30).unwrap();
        assert_eq!(
            replay::stored_walks(&project.stage_index().prepared),
            walked,
            "the nearer line was already settled, so the search ran again"
        );
        assert_eq!(
            far.say.as_ref().map(|s| s.what.as_str()),
            Some("line 19999")
        );
        assert_eq!(
            near.say.as_ref().map(|s| s.what.as_str()),
            Some("line 19969")
        );
        let _ = fs::remove_dir_all(root);
    }

    fn file_node(path: &str) -> crate::engine::EngineImage {
        crate::engine::EngineImage {
            kind: "image".into(),
            file: Some(path.into()),
            ..crate::engine::EngineImage::default()
        }
    }

    fn switch_node(cases: Vec<(&str, &str)>) -> crate::engine::EngineImage {
        crate::engine::EngineImage {
            kind: "switch".into(),
            cases: cases
                .into_iter()
                .map(|(when, path)| crate::engine::EngineCase {
                    when: when.into(),
                    node: file_node(path),
                })
                .collect(),
            ..crate::engine::EngineImage::default()
        }
    }

    fn install(project: &mut Project, image: crate::engine::EngineImage) {
        project.set_stage_images(crate::engine::ImageRun {
            key: project.init_key(),
            dump: crate::engine::ImageDump {
                images: vec![image],
                ..crate::engine::ImageDump::default()
            },
            ..crate::engine::ImageRun::default()
        });
    }

    #[test]
    fn a_composite_switch_uses_the_assignment_after_the_show() {
        let (root, mut project) = scratch(&[(
            "script.rpy",
            "\
default fian = \"n\"
label start:
    show ian
    $ fian = \"smile\"
    \"here\"
",
        )]);
        install(
            &mut project,
            crate::engine::EngineImage {
                name: "ian".into(),
                kind: "composite".into(),
                w: Some(640.0),
                h: Some(1080.0),
                layers: vec![crate::engine::EngineLayer {
                    x: 0.0,
                    y: 0.0,
                    node: switch_node(vec![
                        ("fian == 'smile'", "images/iansmile.webp"),
                        ("True", "images/ian.webp"),
                    ]),
                }],
                ..crate::engine::EngineImage::default()
            },
        );
        let before = estimate(&project, "script.rpy", 3).unwrap();
        assert_eq!(
            before.sprites[0].picture,
            Picture::Layers {
                w: 640.0,
                h: 1080.0,
                layers: vec![PictureLayer {
                    path: "images/ian.webp".into(),
                    x: 0.0,
                    y: 0.0
                }],
            }
        );
        assert!(before.notes.iter().all(|n| !n.contains("not known")));
        let after = estimate(&project, "script.rpy", 5).unwrap();
        assert_eq!(
            after.sprites[0].picture,
            Picture::Layers {
                w: 640.0,
                h: 1080.0,
                layers: vec![PictureLayer {
                    path: "images/iansmile.webp".into(),
                    x: 0.0,
                    y: 0.0
                }],
            }
        );
        assert_eq!(
            (after.sprites[0].size_w, after.sprites[0].size_h),
            (Some(640.0), Some(1080.0))
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn an_unknown_variable_picks_the_first_possible_layer() {
        let (root, mut project) = scratch(&[(
            "script.rpy",
            "\
label start:
    show ian
    \"here\"
",
        )]);
        install(
            &mut project,
            crate::engine::EngineImage {
                name: "ian".into(),
                kind: "composite".into(),
                w: Some(640.0),
                h: Some(1080.0),
                layers: vec![crate::engine::EngineLayer {
                    x: 0.0,
                    y: 0.0,
                    node: switch_node(vec![
                        ("ian_fit == 1", "images/fit.webp"),
                        ("True", "images/ian.webp"),
                    ]),
                }],
                ..crate::engine::EngineImage::default()
            },
        );
        let est = estimate(&project, "script.rpy", 3).unwrap();
        assert_eq!(
            est.sprites[0].picture,
            Picture::Layers {
                w: 640.0,
                h: 1080.0,
                layers: vec![PictureLayer {
                    path: "images/fit.webp".into(),
                    x: 0.0,
                    y: 0.0
                }],
            }
        );
        assert!(
            est.notes.iter().any(|n| n == "ian: `ian_fit` is not known at this line; the preview picked the first possible layer."),
            "{:?}",
            est.notes
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_pattern_image_is_filled_in() {
        let (root, mut project) = scratch(&[(
            "script.rpy",
            "\
default fian = \"smile\"
label start:
    show face
    \"here\"
",
        )]);
        install(
            &mut project,
            crate::engine::EngineImage {
                name: "face".into(),
                kind: "pattern".into(),
                pattern: Some("ian_[fian].webp".into()),
                ..crate::engine::EngineImage::default()
            },
        );
        let est = estimate(&project, "script.rpy", 4).unwrap();
        assert_eq!(
            est.sprites[0].picture,
            Picture::File {
                path: "images/ian_smile.webp".into()
            }
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_cached_stage_is_quick_the_second_time() {
        let mut owned: Vec<(String, String)> = Vec::new();
        for i in 0..40 {
            let mut body = if i == 0 {
                String::from("label start:\n")
            } else {
                format!("label extra_{i}:\n")
            };
            for n in 0..5000 {
                if n % 10 == 0 {
                    body.push_str("    show eileen happy\n");
                } else {
                    body.push_str("    \"line\"\n");
                }
            }
            owned.push((format!("s{i}.rpy"), body));
        }
        let files: Vec<(&str, &str)> = owned
            .iter()
            .map(|(n, t)| (n.as_str(), t.as_str()))
            .collect();
        let (root, mut project) = scratch(&files);
        let mut images = Vec::with_capacity(3000);
        for i in 0..3000 {
            images.push(crate::engine::EngineImage {
                name: format!("eileen mood{i}"),
                kind: "image".into(),
                file: Some(format!("images/mood{i}.png")),
                ..crate::engine::EngineImage::default()
            });
        }
        images.push(crate::engine::EngineImage {
            name: "eileen happy".into(),
            kind: "image".into(),
            file: Some("images/happy.png".into()),
            ..crate::engine::EngineImage::default()
        });
        project.set_stage_images(crate::engine::ImageRun {
            dump: crate::engine::ImageDump {
                images,
                ..crate::engine::ImageDump::default()
            },
            ..crate::engine::ImageRun::default()
        });
        let cold_at = Instant::now();
        let first = estimate(&project, "s0.rpy", 2).unwrap();
        let cold = cold_at.elapsed();
        let warm_at = Instant::now();
        let second = estimate(&project, "s0.rpy", 12).unwrap();
        let warm = warm_at.elapsed();
        eprintln!(
            "cached stage cold {}ms warm {}ms",
            cold.as_millis(),
            warm.as_millis()
        );
        assert_eq!(
            first.sprites[0].picture,
            Picture::File {
                path: "images/happy.png".into()
            }
        );
        assert_eq!(
            second.sprites[0].picture,
            Picture::File {
                path: "images/happy.png".into()
            }
        );
        assert!(
            warm < cold,
            "warm {}ms was not under cold {}ms",
            warm.as_millis(),
            cold.as_millis()
        );
        assert!(
            warm.as_millis() < 50,
            "warm stage estimate took {}ms",
            warm.as_millis()
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_same_size_edit_and_a_new_image_list_rebuild_the_stage() {
        let (root, mut project) = scratch(&[(
            "script.rpy",
            "label start:\n    show eileen happy\n    \"hi\"\n",
        )]);
        let before = estimate(&project, "script.rpy", 2).unwrap();
        assert_eq!(before.sprites[0].name, "eileen happy");
        let path = root.join("game").join("script.rpy");
        fs::write(&path, "label start:\n    show eileen smile\n    \"hi\"\n").unwrap();
        assert!(project.refresh_paths(&[path]));
        let edited = estimate(&project, "script.rpy", 2).unwrap();
        assert_eq!(edited.sprites[0].name, "eileen smile");
        project.set_stage_images(crate::engine::ImageRun {
            key: "not-the-init-key".into(),
            dump: crate::engine::ImageDump {
                images: vec![crate::engine::EngineImage {
                    name: "eileen smile".into(),
                    kind: "image".into(),
                    file: Some("images/smile.png".into()),
                    ..crate::engine::EngineImage::default()
                }],
                ..crate::engine::ImageDump::default()
            },
            ..crate::engine::ImageRun::default()
        });
        let listed = estimate(&project, "script.rpy", 2).unwrap();
        assert_eq!(
            listed.sprites[0].picture,
            Picture::File {
                path: "images/smile.png".into()
            }
        );
        let _ = fs::remove_dir_all(root);
    }

    fn background(est: &StageEstimate) -> Vec<&str> {
        est.sprites.iter().map(|s| s.tag.as_str()).collect()
    }

    #[test]
    fn time_of_day_picks_a_background_and_lists_the_variable() {
        let script = "\
image liv_day = \"day.png\"
image liv_night = \"night.png\"
label start:
    if time >= 6 and time < 20:
        scene liv_day
    else:
        scene liv_night
    \"hi\"
";
        let (root, project) = scratch(&[("script.rpy", script)]);
        let open = estimate(&project, "script.rpy", 8).unwrap();
        assert_eq!(background(&open), vec!["liv_day"]);
        assert_eq!(open.via, "path");
        assert!(open.assumptions.iter().any(|a| a.contains("could not be decided")));
        assert!(open.assumptions.iter().any(|a| a.contains("first branch was used")));
        assert!(open.assumptions.iter().any(|a| a.contains("needs `time`")));
        let inside = estimate(&project, "script.rpy", 5).unwrap();
        assert_eq!(background(&inside), vec!["liv_day"]);
        assert!(inside.assumptions.iter().all(|a| !a.contains("first branch was used")), "{:?}", inside.assumptions);
        assert_eq!(open.vars.len(), 1);
        assert_eq!(open.vars[0].name, "time");
        assert_eq!(open.vars[0].origin, "unset");
        let _ = fs::remove_dir_all(root);

        let (root, project) = scratch(&[(
            "script.rpy",
            "\
default time = 22
image liv_day = \"day.png\"
image liv_night = \"night.png\"
label start:
    if time >= 6 and time < 20:
        scene liv_day
    else:
        scene liv_night
    \"hi\"
",
        )]);
        let night = estimate(&project, "script.rpy", 9).unwrap();
        assert_eq!(background(&night), vec!["liv_night"]);
        assert!(night.assumptions.iter().all(|a| !a.contains("could not be decided")));
        assert_eq!(night.vars[0].origin, "script");
        assert_eq!(night.vars[0].value, "22");
        let _ = fs::remove_dir_all(root);

        let (root, project) = scratch(&[(
            "script.rpy",
            "\
default time = 22
image liv_day = \"day.png\"
image liv_night = \"night.png\"
label start:
    $ time = 22
    if time >= 6 and time < 20:
        scene liv_day
    else:
        scene liv_night
    \"hi\"
",
        )]);
        let pinned = super::estimate(
            &project,
            "script.rpy",
            10,
            &[("time".into(), "12".into())],
        )
        .unwrap();
        assert_eq!(background(&pinned), vec!["liv_day"]);
        assert_eq!(pinned.vars[0].origin, "pinned");
        assert_eq!(pinned.vars[0].value, "12");
        let ignored = estimate(&project, "script.rpy", 10).unwrap();
        assert_eq!(background(&ignored), vec!["liv_night"]);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn an_unreachable_label_still_applies_the_chosen_scene() {
        let (root, project) = scratch(&[(
            "script.rpy",
            "\
image liv_day = \"day.png\"
image liv_night = \"night.png\"
label start:
    return

label room:
    if time >= 6 and time < 20:
        scene liv_day
    else:
        scene liv_night
    \"hi\"
",
        )]);
        let est = estimate(&project, "script.rpy", 11).unwrap();
        assert_eq!(est.via, "label");
        assert_eq!(background(&est), vec!["liv_day"]);
        assert!(est.notes.iter().any(|n| n.contains("could not be decided")));
        assert_eq!(est.vars[0].name, "time");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn the_caret_inside_a_ruled_out_branch_keeps_that_scene() {
        let (root, project) = scratch(&[(
            "script.rpy",
            "\
default time = 8
image liv_day = \"day.png\"
image liv_night = \"night.png\"
label start:
    if time >= 6 and time < 20:
        scene liv_day
    else:
        scene liv_night
        \"night\"
",
        )]);
        let est = estimate(&project, "script.rpy", 9).unwrap();
        assert_eq!(background(&est), vec!["liv_night"]);
        let text = est.assumptions.join("\n") + &est.notes.join("\n");
        assert!(text.contains("needs"), "{text}");
        assert!(text.contains("time"), "{text}");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_while_before_the_caret_is_skipped() {
        let (root, project) = scratch(&[(
            "script.rpy",
            "\
image liv_day = \"day.png\"
label start:
    while time < 10:
        scene liv_day
    \"after\"
",
        )]);
        let est = estimate(&project, "script.rpy", 5).unwrap();
        assert!(background(&est).is_empty(), "{:?}", background(&est));
        assert!(
            est.assumptions.iter().any(|a| a.contains("skipped")),
            "{:?}",
            est.assumptions
        );
        assert!(est.assumptions.iter().all(|a| !a.contains("first branch")));
        let _ = fs::remove_dir_all(root);

        let (root, project) = scratch(&[(
            "script.rpy",
            "\
image liv_day = \"day.png\"
label start:
    return

label room:
    while time < 10:
        scene liv_day
    \"after\"
",
        )]);
        let est = estimate(&project, "script.rpy", 8).unwrap();
        assert_eq!(est.via, "label");
        assert!(background(&est).is_empty(), "{:?}", background(&est));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_pinned_value_chooses_an_elif_when_the_label_is_unreachable() {
        let (root, project) = scratch(&[(
            "script.rpy",
            "\
image liv_day = \"day.png\"
image liv_night = \"night.png\"
image liv_dusk = \"dusk.png\"
label start:
    return

label room:
    if time >= 20:
        scene liv_night
    elif time >= 6:
        scene liv_day
    else:
        scene liv_dusk
    \"hi\"
",
        )]);
        let day = super::estimate(&project, "script.rpy", 14, &[("time".into(), "12".into())]).unwrap();
        assert_eq!(day.via, "label");
        assert_eq!(background(&day), vec!["liv_day"]);
        let night = super::estimate(&project, "script.rpy", 14, &[("time".into(), "22".into())]).unwrap();
        assert_eq!(background(&night), vec!["liv_night"]);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn the_conditions_list_keeps_the_nearest_names() {
        let mut body = String::from("label start:\n");
        for i in 0..30 {
            body.push_str(&format!("    if flag{i}:\n        scene bg{i}\n"));
        }
        body.push_str("    \"end\"\n");
        let (root, project) = scratch(&[("script.rpy", &body)]);
        let est = estimate(&project, "script.rpy", 62).unwrap();
        assert_eq!(est.vars.len(), 24);
        assert_eq!(est.vars_more, 6);
        assert_eq!(est.vars[0].name, "flag29");
        let _ = fs::remove_dir_all(root);

        let mut body = String::from("label start:\n");
        for _ in 0..6 {
            body.push_str("    if time:\n        scene bg\n");
        }
        body.push_str("    \"end\"\n");
        let (root, project) = scratch(&[("script.rpy", &body)]);
        let est = estimate(&project, "script.rpy", 14).unwrap();
        assert_eq!(est.vars.len(), 1);
        assert_eq!(est.vars[0].name, "time");
        assert_eq!(est.vars[0].uses.len(), 5);
        assert_eq!(est.vars[0].more, 1);
        assert_eq!(est.vars[0].uses[0].line, 12);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_dotted_switch_condition_is_not_a_variable() {
        let (root, mut project) = scratch(&[(
            "script.rpy",
            "\
label start:
    show ian
    \"here\"
",
        )]);
        install(
            &mut project,
            crate::engine::EngineImage {
                name: "ian".into(),
                kind: "composite".into(),
                w: Some(640.0),
                h: Some(1080.0),
                layers: vec![crate::engine::EngineLayer {
                    x: 0.0,
                    y: 0.0,
                    node: switch_node(vec![
                        ("persistent.x", "images/fit.webp"),
                        ("True", "images/ian.webp"),
                    ]),
                }],
                ..crate::engine::EngineImage::default()
            },
        );
        let est = estimate(&project, "script.rpy", 3).unwrap();
        assert!(
            est.notes.iter().any(|n| n.contains("condition could not be read")),
            "{:?}",
            est.notes
        );
        assert!(est.vars.iter().all(|v| v.name != "x" && v.name != "persistent"));
        let _ = fs::remove_dir_all(root);
    }
}
