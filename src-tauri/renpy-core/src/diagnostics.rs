//! Project diagnostics: duplicates, missing/unreachable labels, undefined
//! images and speakers, syntax problems and opaque-statement summaries.

use std::collections::{BTreeMap, HashSet};

use serde::Serialize;

use crate::analysis::{walk_all, EngineDiff, External, LabelDef};
use crate::ast::*;
use crate::project::{ImageIndex, SourceFile};

pub struct MissingRef {
    pub file: usize,
    pub line: u32,
    pub target: String,
    pub from: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    /// `error`, `warning` or `info`.
    pub severity: &'static str,
    pub code: &'static str,
    pub message: String,
    /// Path relative to `game/`; empty when not tied to a file.
    pub path: String,
    pub line: u32,
    pub label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DiagReport {
    pub items: Vec<Diagnostic>,
    pub errors: u32,
    pub warnings: u32,
    pub infos: u32,
    pub truncated: u32,
}

const MAX_ITEMS: usize = 5000;

const BUILTIN_SPEAKERS: &[&str] = &[
    "narrator",
    "extend",
    "centered",
    "vcentered",
    "adv",
    "nvl",
    "name_only",
    "_narrator",
];
const BUILTIN_IMAGES: &[&str] = &["black", "text", "vtext"];

fn rank(sev: &str) -> u8 {
    match sev {
        "error" => 0,
        "warning" => 1,
        _ => 2,
    }
}

fn is_ident(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if c.is_alphabetic() || c == '_')
        && chars.all(|c| c.is_alphanumeric() || c == '_')
}

pub fn build(
    files: &[SourceFile],
    defs: &[LabelDef],
    missing: &[MissingRef],
    unreachable: &[usize],
    _known: &HashSet<String>,
    auto_images: &ImageIndex,
    ext: &External,
    engine_diff: Option<&EngineDiff>,
) -> DiagReport {
    let mut items: Vec<Diagnostic> = Vec::new();
    let path_of = |fi: usize| files[fi].rel.clone();

    // Syntax problems.
    for f in files {
        for issue in f.issues.iter().take(200) {
            items.push(Diagnostic {
                severity: "error",
                code: "syntax",
                message: issue.message.clone(),
                path: f.rel.clone(),
                line: issue.line,
                label: None,
            });
        }
    }

    // Duplicate labels.
    let mut first: BTreeMap<&str, &LabelDef> = BTreeMap::new();
    for d in defs {
        if d.primary {
            first.insert(d.name.as_str(), d);
        }
    }
    for d in defs.iter().filter(|d| !d.primary) {
        let other = first.get(d.name.as_str());
        let message = match other {
            Some(o) => format!(
                "`{}` is already defined at {}:{}.",
                d.name, files[o.file].rel, o.line
            ),
            None => format!("`{}` is defined more than once.", d.name),
        };
        // Copies in underscore-prefixed folders (`_extracted`, `_old`, ...) are the
        // usual leftovers of decompiling or backing up a game; flag them softly.
        let rel = &files[d.file].rel;
        let parts: Vec<&str> = rel.split('/').collect();
        let in_scratch_dir =
            parts.len() > 1 && parts[..parts.len() - 1].iter().any(|p| p.starts_with('_'));
        let message = if in_scratch_dir {
            format!("{message} (copy inside a `_` folder, probably extracted or backed-up scripts)")
        } else {
            message
        };
        items.push(Diagnostic {
            severity: if in_scratch_dir { "warning" } else { "error" },
            code: "duplicate-label",
            message,
            path: path_of(d.file),
            line: d.line,
            label: Some(d.name.clone()),
        });
    }

    // Missing jump targets.
    let source_less = ext.source_less();
    for m in missing {
        let who = match m.from.strip_prefix("screen:") {
            Some(screen) => format!("screen {screen}"),
            None => m.from.clone(),
        };
        items.push(Diagnostic {
            severity: if source_less { "warning" } else { "error" },
            code: "missing-label",
            message: if source_less {
                format!(
                    "`{}` jumps or calls to `{}`, which is not defined in the readable scripts (it may live in a compiled script or archive).",
                    who, m.target
                )
            } else {
                format!("`{}` jumps or calls to `{}`, which is not defined.", who, m.target)
            },
            path: path_of(m.file),
            line: m.line,
            label: Some(m.from.clone()),
        });
    }

    // Scripts we cannot read.
    if !ext.compiled_only.is_empty() {
        items.push(Diagnostic {
            severity: "warning",
            code: "compiled-only",
            message: format!(
                "{} script{} exist only as compiled .rpyc ({}); their labels are not mapped.",
                ext.compiled_only.len(),
                plural(ext.compiled_only.len() as u32),
                preview_list(ext.compiled_only)
            ),
            path: String::new(),
            line: 0,
            label: None,
        });
    }
    if !ext.archives.is_empty() {
        items.push(Diagnostic {
            severity: "warning",
            code: "archives",
            message: format!(
                "{} archive{} could not be fully read, so some of their scripts are not mapped: {}.",
                ext.archives.len(),
                plural(ext.archives.len() as u32),
                preview_list(ext.archives)
            ),
            path: String::new(),
            line: 0,
            label: None,
        });
    }
    if !ext.shadowed.is_empty() {
        items.push(Diagnostic {
            severity: "info",
            code: "shadowed-script",
            message: format!(
                "{} script{} hidden by a loose file or a higher-priority archive: {}.",
                ext.shadowed.len(),
                plural(ext.shadowed.len() as u32),
                preview_list(ext.shadowed)
            ),
            path: String::new(),
            line: 0,
            label: None,
        });
    }
    if !ext.stale.is_empty() {
        items.push(Diagnostic {
            severity: "warning",
            code: "stale-patch",
            message: format!(
                "The patch archive was built against a different version of the game: {}.",
                preview_list(ext.stale)
            ),
            path: String::new(),
            line: 0,
            label: None,
        });
    }

    // Parser versus engine.
    if let Some(diff) = engine_diff {
        const MAX_PER_KIND: usize = 40;
        for (name, file, line) in diff.parser_only.iter().take(MAX_PER_KIND) {
            items.push(Diagnostic {
                severity: "info",
                code: "engine-mismatch",
                message: format!("`{name}` is a label here but the engine does not list it."),
                path: file.clone(),
                line: *line,
                label: Some(name.clone()),
            });
        }
        for (name, file, line) in diff.engine_only.iter().take(MAX_PER_KIND) {
            items.push(Diagnostic {
                severity: "info",
                code: "engine-mismatch",
                message: format!("The engine lists `{name}` here but the reader did not find it."),
                path: file.clone(),
                line: *line,
                label: Some(name.clone()),
            });
        }
        for (name, (file, line), (efile, eline)) in diff.moved.iter().take(MAX_PER_KIND) {
            items.push(Diagnostic {
                severity: "info",
                code: "engine-mismatch",
                message: format!(
                    "`{name}` is at line {line} here but the engine says {efile}:{eline}."
                ),
                path: file.clone(),
                line: *line,
                label: Some(name.clone()),
            });
        }
    }

    // Unreachable labels.
    for &di in unreachable {
        let d = &defs[di];
        items.push(Diagnostic {
            severity: "warning",
            code: "unreachable-label",
            message: format!(
                "`{}` is not reachable from `start` or any known entry point.",
                d.name
            ),
            path: path_of(d.file),
            line: d.line,
            label: Some(d.name.clone()),
        });
    }

    // Undefined images / speakers, grouped per file.
    let mut speakers: HashSet<String> = BUILTIN_SPEAKERS.iter().map(|s| s.to_string()).collect();
    let mut extra_names: HashSet<String> = BUILTIN_IMAGES.iter().map(|s| s.to_string()).collect();
    let mut extra_tags: HashSet<String> = extra_names.clone();
    for f in files {
        speakers.extend(f.meta.defs.iter().cloned());
        for img in &f.meta.images {
            let lower = img.to_lowercase();
            if let Some(tag) = lower.split(' ').next() {
                extra_tags.insert(tag.to_string());
            }
            extra_names.insert(lower);
        }
    }
    for f in files {
        let mut undefined_images: BTreeMap<String, (u32, u32)> = BTreeMap::new();
        let mut undefined_speakers: BTreeMap<String, (u32, u32)> = BTreeMap::new();
        walk_all(&f.stmts, &mut |s| match &s.kind {
            Kind::Present {
                cmd, name: Some(n), ..
            } if *cmd == "show" || *cmd == "scene" => {
                let lower = n.to_lowercase();
                let tag = lower.split(' ').next().unwrap_or("");
                if !auto_images.names.contains(&lower)
                    && !extra_names.contains(&lower)
                    && !auto_images.tags.contains(tag)
                    && !extra_tags.contains(tag)
                {
                    let e = undefined_images.entry(n.clone()).or_insert((s.line, 0));
                    e.1 += 1;
                }
            }
            Kind::Say { who: Some(w), .. } if is_ident(w) && !speakers.contains(w) => {
                let e = undefined_speakers.entry(w.clone()).or_insert((s.line, 0));
                e.1 += 1;
            }
            _ => {}
        });
        for (name, (line, count)) in undefined_images {
            items.push(Diagnostic {
                severity: "warning",
                code: "undefined-image",
                message: format!(
                    "Image `{name}` is not defined ({count} use{}).",
                    plural(count)
                ),
                path: f.rel.clone(),
                line,
                label: None,
            });
        }
        for (name, (line, count)) in undefined_speakers {
            items.push(Diagnostic {
                severity: "warning",
                code: "undefined-speaker",
                message: format!(
                    "Speaker `{name}` is not defined ({count} line{}).",
                    plural(count)
                ),
                path: f.rel.clone(),
                line,
                label: None,
            });
        }
        if f.opaque > 0 {
            items.push(Diagnostic {
                severity: "info",
                code: "opaque",
                message: format!(
                    "{} statement{} shown as opaque blocks (screens, transforms, styles, ...).",
                    f.opaque,
                    plural(f.opaque)
                ),
                path: f.rel.clone(),
                line: 1,
                label: None,
            });
        }
    }

    finish(items)
}

impl DiagReport {
    /// Add items and re-sort / re-count.
    pub fn extend_sorted(&mut self, extra: Vec<Diagnostic>) {
        let mut items = std::mem::take(&mut self.items);
        items.extend(extra);
        let previously_cut = self.truncated;
        *self = finish(items);
        self.truncated += previously_cut;
    }
}

fn finish(mut items: Vec<Diagnostic>) -> DiagReport {
    let mut report = DiagReport::default();
    for d in &items {
        match d.severity {
            "error" => report.errors += 1,
            "warning" => report.warnings += 1,
            _ => report.infos += 1,
        }
    }
    items.sort_by(|a, b| {
        rank(a.severity)
            .cmp(&rank(b.severity))
            .then_with(|| a.code.cmp(b.code))
            .then_with(|| a.path.cmp(&b.path))
            .then_with(|| a.line.cmp(&b.line))
    });
    if items.len() > MAX_ITEMS {
        report.truncated = (items.len() - MAX_ITEMS) as u32;
        items.truncate(MAX_ITEMS);
    }
    report.items = items;
    report
}

fn preview_list(items: &[String]) -> String {
    let shown: Vec<&str> = items.iter().take(4).map(|s| s.as_str()).collect();
    let more = items.len().saturating_sub(shown.len());
    if more > 0 {
        format!("{}, and {more} more", shown.join(", "))
    } else {
        shown.join(", ")
    }
}

fn plural(n: u32) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}
