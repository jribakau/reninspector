//! Statement tree. Only constructs that matter for story flow are modelled;
//! everything else is `Kind::Opaque` with an exact source span.

#[derive(Debug, Clone)]
pub struct Stmt {
    pub line: u32,
    /// Last physical line of this statement including any child block.
    pub end_line: u32,
    pub kind: Kind,
}

#[derive(Debug, Clone)]
pub enum Kind {
    Label {
        name: String,
        body: Vec<Stmt>,
    },
    Menu {
        name: Option<String>,
        caption: Option<String>,
        choices: Vec<Choice>,
    },
    Jump {
        target: String,
        dynamic: bool,
    },
    Call {
        target: String,
        dynamic: bool,
        from: Option<String>,
    },
    Return,
    If {
        branches: Vec<Branch>,
    },
    While {
        cond: String,
        body: Vec<Stmt>,
    },
    Say {
        who: Option<String>,
        text: String,
    },
    /// show / scene / hide / with / pause / play / ... (cosmetic statements).
    Present {
        cmd: &'static str,
        name: Option<String>,
        text: String,
    },
    /// `$ ...` (block == false) or a `python:` block (block == true).
    Python {
        block: bool,
        text: String,
        refs: Vec<PyRef>,
    },
    Define {
        keyword: &'static str,
        name: String,
        value: String,
    },
    Image {
        name: String,
    },
    /// `transform name:` — the ATL body stays unparsed.
    Transform {
        name: String,
    },
    Pass,
    /// screen name(...): with the labels and other screens its body can lead to.
    Screen {
        name: String,
        labels: Vec<NameAt>,
        uses: Vec<NameAt>,
    },
    /// call screen x / show screen x.
    ScreenRef {
        how: &'static str,
        name: String,
    },
    Opaque {
        kind: String,
    },
}

#[derive(Debug, Clone)]
pub struct Choice {
    pub line: u32,
    pub end_line: u32,
    pub text: String,
    pub cond: Option<String>,
    pub body: Vec<Stmt>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchKind {
    If,
    Elif,
    Else,
}

#[derive(Debug, Clone)]
pub struct Branch {
    pub line: u32,
    pub end_line: u32,
    pub kind: BranchKind,
    pub cond: String,
    pub body: Vec<Stmt>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefKind {
    /// `renpy.jump("x")`
    RenpyJump,
    /// `renpy.call("x")`
    RenpyCall,
    /// Screen actions: `Jump("x")`, `Call("x")`, `Start("x")`.
    Action,
    /// `renpy.call_screen("x")` / `renpy.show_screen("x")`.
    Screen,
    /// Screen actions that open another screen: `Show("x")`, `ShowMenu("x")`, `ToggleScreen("x")`.
    ScreenAction,
}

/// A name mentioned at one source line (`Jump("shop")` inside a screen, `use phone`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameAt {
    pub name: String,
    pub line: u32,
}

#[derive(Debug, Clone)]
pub struct PyRef {
    pub kind: RefKind,
    /// `None` when the argument is not a string literal.
    pub name: Option<String>,
    /// Physical line of this call. `0` when the caller did not know it.
    pub line: u32,
}

impl Stmt {
    pub fn is_label_like(&self) -> bool {
        matches!(
            &self.kind,
            Kind::Label { .. } | Kind::Menu { name: Some(_), .. }
        )
    }
}

/// True when executing the statements can transfer control somewhere else
/// (jump, call, return, menu, label) so they must stay visible in the graph.
pub fn contains_flow(stmts: &[Stmt]) -> bool {
    stmts.iter().any(|s| match &s.kind {
        Kind::Label { .. }
        | Kind::Menu { .. }
        | Kind::Jump { .. }
        | Kind::Call { .. }
        | Kind::Return => true,
        Kind::If { branches } => branches.iter().any(|b| contains_flow(&b.body)),
        Kind::While { body, .. } => contains_flow(body),
        Kind::Python { refs, .. } => refs
            .iter()
            .any(|r| matches!(r.kind, RefKind::RenpyJump | RefKind::RenpyCall)),
        _ => false,
    })
}

/// True when the statements only change presentation (show/hide/scene/with/...).
pub fn is_cosmetic(stmts: &[Stmt]) -> bool {
    stmts.iter().all(|s| match &s.kind {
        Kind::Present { .. } | Kind::Pass => true,
        Kind::If { branches } => branches.iter().all(|b| is_cosmetic(&b.body)),
        _ => false,
    })
}
