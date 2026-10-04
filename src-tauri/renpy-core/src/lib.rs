//! Ren'Py project reader: discovery, lexer, parser, control-flow graphs and diagnostics.
//! Pure Rust with no Tauri dependency so it can be tested quickly.

pub mod analysis;
pub mod ast;
pub mod catalog;
pub mod diagnostics;
pub mod engine;
pub mod flow;
pub mod lexer;
pub mod parser;
pub mod project;
pub mod prose;
pub mod replay;
pub mod rpa;
pub mod rpyc;
pub mod scene;
pub mod screens;
pub mod stage;
pub mod vars;

pub use analysis::{Analysis, EngineDiff, External, LabelDef, MapEdge, MapNode, ProjectMap, Stats};
pub use catalog::{
    Catalog, DialogueStats, LanguageStat, SearchHit, SpeakerStat, Symbol, Translation, Variable,
};
pub use diagnostics::{DiagReport, Diagnostic};
pub use engine::{EngineDump, EngineRun, EngineSummary, LintItem};
pub use flow::LabelGraph;
pub use parser::SyntaxIssue;
pub use project::{ArchiveInfo, EngineTarget, Launcher, Origin, Project, ProjectInfo};
