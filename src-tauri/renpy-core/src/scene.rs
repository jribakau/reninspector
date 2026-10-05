//! Rewrite one script's text for the scene writer.
//!
//! Edits stay on the physical lines the parser numbered. Comments, attributes
//! on a say (`e happy "hi"`), and choice conditions are left in place. A line
//! the rewriter does not understand is refused so the code view can take it.

use serde::{Deserialize, Serialize};

use crate::parser::{parse_say, parse_string_at};

/// A one-line statement the flow editor can write.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum StmtSpec {
    Say {
        #[serde(default)]
        speaker: String,
        #[serde(default)]
        text: String,
    },
    Scene {
        image: String,
        #[serde(default)]
        at: String,
        #[serde(default)]
        with: String,
    },
    Show {
        image: String,
        #[serde(default)]
        at: String,
        #[serde(default)]
        with: String,
    },
    Hide {
        image: String,
        #[serde(default)]
        with: String,
    },
    With {
        transition: String,
    },
    Play {
        channel: String,
        file: String,
        #[serde(default)]
        fadein: String,
        #[serde(default)]
        looped: bool,
    },
    Stop {
        channel: String,
        #[serde(default)]
        fadeout: String,
    },
    Pause {
        #[serde(default)]
        secs: String,
    },
    Jump {
        target: String,
    },
    Call {
        target: String,
    },
    Return,
}

/// Where a new statement goes relative to the line it is anchored to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Place {
    /// On the line before the anchor, at its indent.
    Before,
    /// After the anchor and everything nested under it.
    After,
    /// As the first line inside the anchor's block.
    Into,
}

/// How a scene is tied to the end of another.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkKind {
    Jump,
    Call,
    Choice,
    Return,
}

/// One change, applied to source that uses `\n` and has no BOM.
#[derive(Debug, Clone)]
pub enum SceneOp {
    /// Insert a statement built from `spec`.
    AddStmt {
        anchor: u32,
        place: Place,
        spec: StmtSpec,
    },
    /// Rewrite a one-line staging, jump, call, or return statement in place.
    SetStmt {
        line: u32,
        spec: StmtSpec,
    },
    /// Remove a statement and everything nested under it.
    DeleteStmt {
        line: u32,
    },
    /// Swap a statement with its neighbour. `dir` is -1 (up) or 1 (down).
    MoveStmt {
        line: u32,
        dir: i32,
    },
    DuplicateStmt {
        line: u32,
    },
    /// Set or clear the `if` condition on a menu choice.
    SetChoiceCond {
        line: u32,
        cond: String,
    },
    /// Tie the end of the label at `label_line` to `target`.
    LinkScene {
        label_line: u32,
        how: LinkKind,
        target: String,
        caption: String,
    },
    SetSay {
        line: u32,
        speaker: String,
        text: String,
    },
    /// Replace a choice caption. When `target_line` is set, also retarget its jump.
    SetChoice {
        line: u32,
        text: String,
        target: String,
        target_line: u32,
    },
    SetJump {
        line: u32,
        target: String,
    },
    AddSay {
        after: u32,
        speaker: String,
        text: String,
    },
    /// Insert a choice after a menu, a choice, or any other line (which grows a new menu).
    AddChoice {
        anchor: u32,
        text: String,
        target: String,
    },
    AddLabel {
        name: String,
    },
    AddCharacter {
        var: String,
        who: String,
        color: String,
    },
    AddImage {
        name: String,
        file: String,
    },
}

pub fn apply_all(src: &str, ops: &[SceneOp]) -> Result<String, String> {
    apply_all_focus(src, ops).map(|(text, _)| text)
}

/// Like `apply_all`, with the 1-based line the edit touched (0 when it has none).
/// The first op that names a line decides it; later ops (a new scene) keep it.
pub fn apply_all_focus(src: &str, ops: &[SceneOp]) -> Result<(String, u32), String> {
    let mut cur = src.to_string();
    let mut focus = 0;
    for op in ops {
        let (next, at) = apply_focus(&cur, op)?;
        cur = next;
        if focus == 0 {
            focus = at;
        }
    }
    Ok((cur, focus))
}

pub fn apply(src: &str, op: &SceneOp) -> Result<String, String> {
    apply_focus(src, op).map(|(text, _)| text)
}

/// Apply one op and report the 1-based line it inserted, moved, or edited.
pub fn apply_focus(src: &str, op: &SceneOp) -> Result<(String, u32), String> {
    match op {
        SceneOp::AddStmt {
            anchor,
            place,
            spec,
        } => add_stmt(src, *anchor, *place, spec),
        SceneOp::SetStmt { line, spec } => {
            map_line(src, *line, |raw| rewrite_stmt(raw, spec)).map(|t| (t, *line))
        }
        SceneOp::DeleteStmt { line } => delete_stmt(src, *line),
        SceneOp::MoveStmt { line, dir } => move_stmt(src, *line, *dir),
        SceneOp::DuplicateStmt { line } => duplicate_stmt(src, *line),
        SceneOp::SetChoiceCond { line, cond } => {
            map_line(src, *line, |raw| rewrite_choice_cond(raw, cond)).map(|t| (t, *line))
        }
        SceneOp::LinkScene {
            label_line,
            how,
            target,
            caption,
        } => link_scene(src, *label_line, *how, target, caption),
        SceneOp::AddSay {
            after,
            speaker,
            text,
        } => {
            let (lines, _) = split_lines(src);
            let host = line_mut(&lines, *after)?;
            let place = if code_part(host).ends_with(':') {
                Place::Into
            } else {
                Place::After
            };
            let spec = StmtSpec::Say {
                speaker: speaker.clone(),
                text: text.clone(),
            };
            add_stmt(src, *after, place, &spec)
        }
        SceneOp::AddChoice {
            anchor,
            text,
            target,
        } => add_choice(src, *anchor, text, target),
        other => apply_plain(src, other).map(|t| {
            let line = match other {
                SceneOp::SetSay { line, .. }
                | SceneOp::SetChoice { line, .. }
                | SceneOp::SetJump { line, .. } => *line,
                _ => 0,
            };
            (t, line)
        }),
    }
}

fn apply_plain(src: &str, op: &SceneOp) -> Result<String, String> {
    match op {
        SceneOp::SetSay {
            line,
            speaker,
            text,
        } => map_line(src, *line, |raw| rewrite_say(raw, speaker, text)),
        SceneOp::SetChoice {
            line,
            text,
            target,
            target_line,
        } => {
            let next = map_line(src, *line, |raw| rewrite_choice(raw, text))?;
            if target.is_empty() || *target_line == 0 {
                return Ok(next);
            }
            check_label(target)?;
            map_line(&next, *target_line, |raw| rewrite_jump(raw, target))
        }
        SceneOp::SetJump { line, target } => {
            check_label(target)?;
            map_line(src, *line, |raw| rewrite_jump(raw, target))
        }
        SceneOp::AddLabel { name } => add_label(src, name),
        SceneOp::AddCharacter { var, who, color } => add_character(src, var, who, color),
        SceneOp::AddImage { name, file } => add_image(src, name, file),
        _ => Err("That scene edit is not supported.".into()),
    }
}

/// A double-quoted Ren'Py string.
pub fn renpy_string(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

pub fn character_statement(var: &str, who: &str, color: &str) -> Result<String, String> {
    check_ident(var, "character")?;
    check_color(color)?;
    if who.contains('\n') {
        return Err("A character's name stays on one line.".into());
    }
    if color.is_empty() {
        Ok(format!("define {var} = Character({})", renpy_string(who)))
    } else {
        Ok(format!(
            "define {var} = Character({}, color={})",
            renpy_string(who),
            renpy_string(color)
        ))
    }
}

pub fn image_statement(name: &str, file: &str) -> Result<String, String> {
    check_image_name(name)?;
    if file.is_empty() || file.contains(['\n', '"', '\\']) {
        return Err("That image path cannot be written into the script.".into());
    }
    Ok(format!("image {name} = {}", renpy_string(file)))
}

fn add_choice(src: &str, anchor: u32, text: &str, target: &str) -> Result<(String, u32), String> {
    if text.is_empty() {
        return Err("Write the choice first.".into());
    }
    check_label(target)?;
    let (mut lines, ended) = split_lines(src);
    let idx = (anchor as usize)
        .checked_sub(1)
        .filter(|i| *i < lines.len())
        .ok_or_else(|| "That line is not in the file.".to_string())?;
    let host = lines[idx].clone();
    let code = code_part(&host);
    #[allow(clippy::needless_late_init)]
    let focus;
    if is_menu_header(code) || is_choice_header(code) {
        let choice_indent = if is_menu_header(code) {
            deeper(&indent_of(&host))
        } else {
            indent_of(&host)
        };
        let at = stmt_end(&lines, idx);
        let block = choice_block(&choice_indent, text, target);
        for (i, line) in block.into_iter().enumerate() {
            lines.insert(at + i, line);
        }
        focus = at as u32 + 1;
    } else {
        if is_label_header(code) {
            let at = child_insert_at(&lines, idx);
            let menu_indent = child_indent(&lines, idx);
            let choice_indent = deeper(&menu_indent);
            let mut block = vec![format!("{menu_indent}menu:")];
            block.extend(choice_block(&choice_indent, text, target));
            for (i, line) in block.into_iter().enumerate() {
                lines.insert(at + i, line);
            }
            return Ok((join_lines(&lines, ended), at as u32 + 2));
        }
        let menu_indent = if code.ends_with(':') {
            deeper(&indent_of(&host))
        } else {
            indent_of(&host)
        };
        let choice_indent = deeper(&menu_indent);
        let mut block = vec![format!("{menu_indent}menu:")];
        block.extend(choice_block(&choice_indent, text, target));
        insert_after(&mut lines, anchor, &block)?;
        focus = anchor + 2;
    }
    Ok((join_lines(&lines, ended), focus))
}

fn choice_block(choice_indent: &str, text: &str, target: &str) -> Vec<String> {
    let jump_indent = deeper(choice_indent);
    vec![
        format!("{choice_indent}{}:", renpy_string(text)),
        format!("{jump_indent}jump {target}"),
    ]
}

fn add_label(src: &str, name: &str) -> Result<String, String> {
    check_label(name)?;
    if src.lines().any(|line| {
        let t = code_part(line);
        t == format!("label {name}:") || t.starts_with(&format!("label {name} "))
    }) {
        return Err(format!("`{name}` is already a scene in this file."));
    }
    let mut out = src.to_string();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    if !out.is_empty() {
        out.push('\n');
    }
    out.push_str(&format!("label {name}:\n    \"…\"\n    return\n"));
    Ok(out)
}

fn add_character(src: &str, var: &str, who: &str, color: &str) -> Result<String, String> {
    let stmt = character_statement(var, who, color)?;
    let mut out = src.to_string();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    if !out.is_empty() {
        out.push('\n');
    }
    out.push_str(&stmt);
    out.push('\n');
    Ok(out)
}

fn add_image(src: &str, name: &str, file: &str) -> Result<String, String> {
    let stmt = image_statement(name, file)?;
    let mut out = src.to_string();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&stmt);
    out.push('\n');
    Ok(out)
}

fn map_line(
    src: &str,
    line: u32,
    f: impl FnOnce(&str) -> Result<String, String>,
) -> Result<String, String> {
    let (mut lines, ended) = split_lines(src);
    let raw = line_mut(&lines, line)?.to_string();
    let next = f(&raw)?;
    lines[(line as usize) - 1] = next;
    Ok(join_lines(&lines, ended))
}

fn rewrite_say(raw: &str, speaker: &str, text: &str) -> Result<String, String> {
    if !speaker.is_empty() {
        check_ident(speaker, "character")?;
    }
    let indent = indent_of(raw);
    let body = raw.trim();
    if parse_say(body).is_none() {
        return Err("That line is not dialogue. Edit it in code.".into());
    }
    let with_text = replace_first_string(body, text)
        .ok_or_else(|| "This dialogue spans more than one line. Edit it in code.".to_string())?;
    let (who, _) = parse_say(&with_text)
        .ok_or_else(|| "That line is not dialogue. Edit it in code.".to_string())?;
    let next = match who {
        None if speaker.is_empty() => with_text,
        None => format!("{speaker} {with_text}"),
        Some(old) if speaker.is_empty() => {
            if old.starts_with('"') || old.starts_with('\'') || !with_text.starts_with(&old) {
                return Err("This speaker is an expression. Edit it in code.".into());
            }
            with_text[old.len()..].trim_start().to_string()
        }
        Some(old) if old == speaker => with_text,
        Some(old) => {
            if old.starts_with('"') || old.starts_with('\'') || !with_text.starts_with(&old) {
                return Err("This speaker is an expression. Edit it in code.".into());
            }
            format!("{speaker}{}", &with_text[old.len()..])
        }
    };
    Ok(format!("{indent}{next}"))
}

fn rewrite_choice(raw: &str, text: &str) -> Result<String, String> {
    let indent = indent_of(raw);
    let body = raw.trim();
    if !is_choice_header(code_part(body)) && !body.starts_with('"') && !body.starts_with('\'') {
        return Err("That line is not a choice. Edit it in code.".into());
    }
    let next = replace_first_string(body, text)
        .ok_or_else(|| "This choice spans more than one line. Edit it in code.".to_string())?;
    Ok(format!("{indent}{next}"))
}

fn rewrite_jump(raw: &str, target: &str) -> Result<String, String> {
    let indent = indent_of(raw);
    let body = code_part(raw.trim());
    let (kw, rest) = if let Some(rest) = body.strip_prefix("jump") {
        ("jump", rest)
    } else if let Some(rest) = body.strip_prefix("call") {
        ("call", rest)
    } else {
        return Err("That line is not a jump. Edit it in code.".into());
    };
    if rest.is_empty() || !rest.starts_with(char::is_whitespace) {
        return Err("That line is not a jump. Edit it in code.".into());
    }
    if rest.split_whitespace().next() == Some("expression") {
        return Err("This jump uses an expression. Edit it in code.".into());
    }
    if rest.split_whitespace().any(|w| w == "from") {
        return Err("This jump has a from clause. Edit it in code.".into());
    }
    let suffix = raw
        .trim()
        .find(" #")
        .map(|i| raw.trim()[i..].to_string())
        .unwrap_or_default();
    Ok(format!("{indent}{kw} {target}{suffix}"))
}

fn replace_first_string(body: &str, text: &str) -> Option<String> {
    let (start, q) = body.char_indices().find(|(_, c)| *c == '"' || *c == '\'')?;
    let mut chars = body[start + q.len_utf8()..].char_indices();
    let mut end = None;
    while let Some((i, c)) = chars.next() {
        if c == '\\' {
            chars.next();
            continue;
        }
        if c == q {
            end = Some(start + q.len_utf8() + i);
            break;
        }
    }
    let end = end?;
    Some(format!(
        "{}{}{}",
        &body[..start],
        renpy_string(text),
        &body[end + q.len_utf8()..]
    ))
}

fn split_lines(src: &str) -> (Vec<String>, bool) {
    let ended = src.ends_with('\n');
    let mut lines: Vec<String> = src.split('\n').map(|s| s.to_string()).collect();
    if ended {
        lines.pop();
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    (lines, ended || src.is_empty())
}

fn join_lines(lines: &[String], ended: bool) -> String {
    let mut out = lines.join("\n");
    if ended {
        out.push('\n');
    }
    out
}

fn line_mut(lines: &[String], line: u32) -> Result<&String, String> {
    lines
        .get((line as usize).wrapping_sub(1))
        .ok_or_else(|| "That line is not in the file.".to_string())
}

fn insert_after(lines: &mut Vec<String>, after: u32, block: &[String]) -> Result<(), String> {
    if after == 0 || after as usize > lines.len() {
        return Err("That line is not in the file.".into());
    }
    let at = after as usize;
    for (i, line) in block.iter().enumerate() {
        lines.insert(at + i, line.clone());
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Statement builders

impl StmtSpec {
    /// The statement as one line of script, without indent.
    pub fn render(&self) -> Result<String, String> {
        Ok(match self {
            StmtSpec::Say { speaker, text } => {
                let speaker = speaker.trim();
                if !speaker.is_empty() {
                    check_ident(speaker, "character")?;
                }
                if speaker.is_empty() {
                    renpy_string(text)
                } else {
                    format!("{speaker} {}", renpy_string(text))
                }
            }
            StmtSpec::Scene { image, at, with } => {
                let mut out = format!("scene {}", clean_image(image)?);
                out.push_str(&clause_at(at)?);
                out.push_str(&clause_with(with)?);
                out
            }
            StmtSpec::Show { image, at, with } => {
                let mut out = format!("show {}", clean_image(image)?);
                out.push_str(&clause_at(at)?);
                out.push_str(&clause_with(with)?);
                out
            }
            StmtSpec::Hide { image, with } => {
                let mut out = format!("hide {}", clean_image(image)?);
                out.push_str(&clause_with(with)?);
                out
            }
            StmtSpec::With { transition } => {
                let t = transition.trim();
                check_transition(t)?;
                format!("with {t}")
            }
            StmtSpec::Play {
                channel,
                file,
                fadein,
                looped,
            } => {
                let channel = channel.trim();
                check_ident(channel, "channel")?;
                check_audio_path(file)?;
                let mut out = format!("play {channel} {}", renpy_string(file.trim()));
                let fadein = fadein.trim();
                if !fadein.is_empty() {
                    check_secs(fadein)?;
                    out.push_str(&format!(" fadein {fadein}"));
                }
                if *looped {
                    out.push_str(" loop");
                }
                out
            }
            StmtSpec::Stop { channel, fadeout } => {
                let channel = channel.trim();
                check_ident(channel, "channel")?;
                let mut out = format!("stop {channel}");
                let fadeout = fadeout.trim();
                if !fadeout.is_empty() {
                    check_secs(fadeout)?;
                    out.push_str(&format!(" fadeout {fadeout}"));
                }
                out
            }
            StmtSpec::Pause { secs } => {
                let secs = secs.trim();
                if secs.is_empty() {
                    "pause".to_string()
                } else {
                    check_secs(secs)?;
                    format!("pause {secs}")
                }
            }
            StmtSpec::Jump { target } => {
                let t = target.trim();
                check_label(t)?;
                format!("jump {t}")
            }
            StmtSpec::Call { target } => {
                let t = target.trim();
                check_label(t)?;
                format!("call {t}")
            }
            StmtSpec::Return => "return".to_string(),
        })
    }
}

const RESERVED_IMAGE_WORDS: &[&str] = &[
    "expression",
    "as",
    "behind",
    "onlayer",
    "zorder",
    "at",
    "with",
];

fn clean_image(image: &str) -> Result<String, String> {
    let words: Vec<&str> = image.split_whitespace().collect();
    let ok = !words.is_empty()
        && words.iter().all(|w| {
            !RESERVED_IMAGE_WORDS.contains(w)
                && w.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        });
    if ok {
        Ok(words.join(" "))
    } else {
        Err("Use an image name like eileen happy.".into())
    }
}

fn clause_at(at: &str) -> Result<String, String> {
    let names: Vec<&str> = at
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|w| !w.is_empty())
        .collect();
    if names.is_empty() {
        return Ok(String::new());
    }
    for n in &names {
        if !is_ident(n) || RESERVED_IMAGE_WORDS.contains(n) {
            return Err("Use a position like left, right, or center.".into());
        }
    }
    Ok(format!(" at {}", names.join(", ")))
}

fn clause_with(with: &str) -> Result<String, String> {
    let t = with.trim();
    if t.is_empty() {
        return Ok(String::new());
    }
    check_transition(t)?;
    Ok(format!(" with {t}"))
}

/// A transition expression such as `dissolve` or `Dissolve(0.5)`.
pub fn check_transition(t: &str) -> Result<(), String> {
    let t = t.trim();
    let mut depth = 0i32;
    let mut ok = t
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_');
    for c in t.chars() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            c if c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | ',' | ' ' | '-') => {}
            _ => ok = false,
        }
        if depth < 0 {
            ok = false;
        }
    }
    if ok && depth == 0 {
        Ok(())
    } else {
        Err("Use a transition like dissolve or Dissolve(0.5).".into())
    }
}

pub fn check_audio_path(file: &str) -> Result<(), String> {
    let f = file.trim();
    if f.is_empty() || f.chars().any(|c| c.is_control() || c == '"' || c == '\\') {
        Err("Pick an audio file from the project.".into())
    } else {
        Ok(())
    }
}

fn check_secs(s: &str) -> Result<(), String> {
    match s.trim().parse::<f64>() {
        Ok(n) if n.is_finite() && n >= 0.0 => Ok(()),
        _ => Err("Use a number of seconds like 0.5.".into()),
    }
}

/// Read a one-line staging statement back into a spec. Anything the editor
/// cannot write again exactly (expressions, `behind`, ATL blocks) is `None`.
pub fn parse_present(code: &str) -> Option<StmtSpec> {
    let code = code.trim();
    if code.is_empty() || code.ends_with(':') || code.contains('\n') {
        return None;
    }
    let (cmd, rest) = match code.split_once(char::is_whitespace) {
        Some((c, r)) => (c, r.trim()),
        None => (code, ""),
    };
    match cmd {
        "scene" | "show" | "hide" => {
            let tokens: Vec<&str> = rest.split_whitespace().collect();
            let (head, with) = match tokens.iter().position(|t| *t == "with") {
                Some(p) => {
                    let t = tokens[p + 1..].join(" ");
                    check_transition(&t).ok()?;
                    (&tokens[..p], t)
                }
                None => (&tokens[..], String::new()),
            };
            let (img, at) = match head.iter().position(|t| *t == "at") {
                Some(p) => (&head[..p], head[p + 1..].join(" ")),
                None => (head, String::new()),
            };
            let image = clean_image(&img.join(" ")).ok()?;
            let at = if at.is_empty() {
                at
            } else {
                clause_at(&at).ok()?.trim_start_matches(" at ").to_string()
            };
            match cmd {
                "scene" => Some(StmtSpec::Scene { image, at, with }),
                "show" => Some(StmtSpec::Show { image, at, with }),
                _ if !at.is_empty() => None,
                _ => Some(StmtSpec::Hide { image, with }),
            }
        }
        "with" => {
            check_transition(rest).ok()?;
            Some(StmtSpec::With {
                transition: rest.to_string(),
            })
        }
        "pause" => {
            if !rest.is_empty() {
                check_secs(rest).ok()?;
            }
            Some(StmtSpec::Pause {
                secs: rest.to_string(),
            })
        }
        "play" => {
            let (channel, after) = rest.split_once(char::is_whitespace)?;
            if !is_ident(channel) {
                return None;
            }
            let after = after.trim();
            let (file, end) = parse_string_at(after)?;
            check_audio_path(&file).ok()?;
            let mut fadein = String::new();
            let mut looped = false;
            let mut words = after[end..].split_whitespace();
            while let Some(w) = words.next() {
                match w {
                    "fadein" => {
                        let n = words.next()?;
                        check_secs(n).ok()?;
                        fadein = n.to_string();
                    }
                    "loop" => looped = true,
                    _ => return None,
                }
            }
            Some(StmtSpec::Play {
                channel: channel.to_string(),
                file,
                fadein,
                looped,
            })
        }
        "stop" => {
            let mut words = rest.split_whitespace();
            let channel = words.next()?;
            if !is_ident(channel) {
                return None;
            }
            let mut fadeout = String::new();
            while let Some(w) = words.next() {
                if w != "fadeout" {
                    return None;
                }
                let n = words.next()?;
                check_secs(n).ok()?;
                fadeout = n.to_string();
            }
            Some(StmtSpec::Stop {
                channel: channel.to_string(),
                fadeout,
            })
        }
        _ => None,
    }
}

/// `parse_present`, plus jump, call, return, and dialogue.
pub fn parse_stmt(code: &str) -> Option<StmtSpec> {
    let code = code.trim();
    let word = code.split_whitespace().next().unwrap_or("");
    match word {
        "return" if code == "return" => Some(StmtSpec::Return),
        "jump" | "call" => {
            let rest = code[word.len()..].trim();
            let words: Vec<&str> = rest.split_whitespace().collect();
            if words.len() != 1 || check_label(words[0]).is_err() {
                return None;
            }
            let target = words[0].to_string();
            Some(if word == "jump" {
                StmtSpec::Jump { target }
            } else {
                StmtSpec::Call { target }
            })
        }
        "scene" | "show" | "hide" | "with" | "pause" | "play" | "stop" => parse_present(code),
        _ => {
            if code.ends_with(':') {
                return None;
            }
            let (who, text) = parse_say(code)?;
            Some(StmtSpec::Say {
                speaker: who.unwrap_or_default(),
                text,
            })
        }
    }
}

// ---------------------------------------------------------------------------
// Structure helpers

fn is_content(line: &str) -> bool {
    let t = line.trim();
    !t.is_empty() && !t.starts_with('#')
}

/// Exclusive end of the statement at `idx` and everything nested under it,
/// not counting blank or comment lines that trail it.
fn stmt_end(lines: &[String], idx: usize) -> usize {
    let ind = indent_width(&lines[idx]);
    let mut last = idx + 1;
    let mut i = idx + 1;
    while i < lines.len() {
        if !is_content(&lines[i]) {
            i += 1;
            continue;
        }
        if indent_width(&lines[i]) <= ind {
            break;
        }
        i += 1;
        last = i;
    }
    last
}

/// `stmt_end`, widened over the `elif` and `else` branches that belong to an `if`.
fn group_end(lines: &[String], idx: usize) -> usize {
    let mut end = stmt_end(lines, idx);
    if is_branch_tail(code_part(&lines[idx])) {
        return end;
    }
    let ind = indent_width(&lines[idx]);
    loop {
        let mut j = end;
        while j < lines.len() && !is_content(&lines[j]) {
            j += 1;
        }
        if j < lines.len() && indent_width(&lines[j]) == ind && is_branch_tail(code_part(&lines[j]))
        {
            end = stmt_end(lines, j);
        } else {
            return end;
        }
    }
}

/// Start of the statement that sits just above `idx` in the same block.
/// A trailing `else` or `elif` resolves to the `if` that opens it.
fn prev_sibling(lines: &[String], idx: usize) -> Option<usize> {
    let ind = indent_width(&lines[idx]);
    let mut from = idx;
    loop {
        let mut found = None;
        let mut j = from;
        while j > 0 {
            j -= 1;
            if !is_content(&lines[j]) {
                continue;
            }
            let w = indent_width(&lines[j]);
            if w > ind {
                continue;
            }
            if w == ind {
                found = Some(j);
            }
            break;
        }
        let s = found?;
        if is_branch_tail(code_part(&lines[s])) {
            from = s;
        } else {
            return Some(s);
        }
    }
}

/// The nearest earlier line with less indent.
fn parent_of(lines: &[String], idx: usize) -> Option<usize> {
    let ind = indent_width(&lines[idx]);
    (0..idx)
        .rev()
        .find(|j| is_content(&lines[*j]) && indent_width(&lines[*j]) < ind)
}

/// Indent used inside the block that starts at `header`.
fn child_indent(lines: &[String], header: usize) -> String {
    let end = stmt_end(lines, header);
    (header + 1..end)
        .find(|i| is_content(&lines[*i]))
        .map(|i| indent_of(&lines[i]))
        .unwrap_or_else(|| deeper(&indent_of(&lines[header])))
}

fn child_insert_at(_lines: &[String], header: usize) -> usize {
    header + 1
}

fn is_label_header(code: &str) -> bool {
    code.starts_with("label ") && code.ends_with(':')
}

fn is_branch_tail(code: &str) -> bool {
    let w = code.split(|c: char| c == ':' || c.is_whitespace()).next();
    matches!(w, Some("else") | Some("elif"))
}

fn is_exit(code: &str) -> bool {
    matches!(
        code.split_whitespace().next(),
        Some("return") | Some("jump")
    )
}

/// True when the statement on this line carries on past it (an open string or bracket).
fn continues(line: &str) -> bool {
    let cs: Vec<char> = line.trim().chars().collect();
    let mut i = 0;
    let mut depth = 0i32;
    while i < cs.len() {
        let c = cs[i];
        if c == '#' {
            break;
        }
        if c == '"' || c == '\'' {
            let triple = i + 2 < cs.len() && cs[i + 1] == c && cs[i + 2] == c;
            let mut j = i + if triple { 3 } else { 1 };
            let mut closed = false;
            while j < cs.len() {
                if cs[j] == '\\' {
                    j += 2;
                    continue;
                }
                if cs[j] == c {
                    if !triple {
                        j += 1;
                        closed = true;
                        break;
                    }
                    if j + 2 < cs.len() && cs[j + 1] == c && cs[j + 2] == c {
                        j += 3;
                        closed = true;
                        break;
                    }
                }
                j += 1;
            }
            if !closed {
                return true;
            }
            i = j;
            continue;
        }
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            _ => {}
        }
        i += 1;
    }
    depth > 0 || line.trim_end().ends_with('\\')
}

fn line_idx(lines: &[String], line: u32) -> Result<usize, String> {
    (line as usize)
        .checked_sub(1)
        .filter(|i| *i < lines.len())
        .ok_or_else(|| "That line is not in the file.".to_string())
}

fn single_line(lines: &[String], idx: usize) -> Result<(), String> {
    if continues(&lines[idx]) {
        Err("This statement spans more than one line. Edit it in code.".into())
    } else {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Statement edits

fn add_stmt(
    src: &str,
    anchor: u32,
    place: Place,
    spec: &StmtSpec,
) -> Result<(String, u32), String> {
    let text = spec.render()?;
    let (mut lines, ended) = split_lines(src);
    let idx = line_idx(&lines, anchor)?;
    single_line(&lines, idx)?;
    let host = lines[idx].clone();
    let code = code_part(&host).to_string();
    if is_choice_header(&code) && place != Place::Into {
        return Err("A choice holds lines inside it. Add the line inside the choice.".into());
    }
    if is_label_header(&code) && place != Place::Into {
        return Err("Add the line inside the scene.".into());
    }
    if is_menu_header(&code) && place == Place::Into {
        return Err("A menu holds choices. Add a choice instead.".into());
    }
    if is_branch_tail(&code) && place != Place::Into {
        return Err("Add the line inside the branch, or next to the whole condition.".into());
    }
    let (pad, at) = match place {
        Place::Into => {
            if !code.ends_with(':') {
                return Err("That line has no body to add to.".into());
            }
            (child_indent(&lines, idx), child_insert_at(&lines, idx))
        }
        Place::Before => (indent_of(&host), idx),
        Place::After => (indent_of(&host), group_end(&lines, idx)),
    };
    lines.insert(at, format!("{pad}{text}"));
    Ok((join_lines(&lines, ended), at as u32 + 1))
}

fn rewrite_stmt(raw: &str, spec: &StmtSpec) -> Result<String, String> {
    if let StmtSpec::Say { speaker, text } = spec {
        return rewrite_say(raw, speaker, text);
    }
    if continues(raw) {
        return Err("This statement spans more than one line. Edit it in code.".into());
    }
    let indent = indent_of(raw);
    let body = raw.trim();
    let code = code_part(body);
    match parse_stmt(code) {
        None | Some(StmtSpec::Say { .. }) => {
            return Err("The flow editor cannot change that line. Edit it in code.".into())
        }
        Some(_) => {}
    }
    let tail = body.get(code.len()..).unwrap_or("");
    Ok(format!("{indent}{}{tail}", spec.render()?))
}

fn rewrite_choice_cond(raw: &str, cond: &str) -> Result<String, String> {
    let indent = indent_of(raw);
    let body = raw.trim();
    let (_, end) = parse_string_at(body)
        .filter(|_| body.starts_with('"') || body.starts_with('\''))
        .ok_or_else(|| "That line is not a choice. Edit it in code.".to_string())?;
    let literal = &body[..end];
    let tail = &body[end..];
    let code_region = match tail.find('#') {
        Some(i) => &tail[..i],
        None => tail,
    }
    .trim_end();
    let rest = code_region
        .strip_suffix(':')
        .ok_or_else(|| "That line is not a choice. Edit it in code.".to_string())?
        .trim();
    // Whatever follows the colon (spacing and a comment) stays as it was.
    let comment = tail[code_region.len()..].trim_end();
    let has_if = rest.starts_with("if") && rest[2..].starts_with(char::is_whitespace);
    if !rest.is_empty() && !has_if {
        return Err("This choice has extra settings. Edit it in code.".into());
    }
    let cond = cond.trim();
    if cond.contains('\n') || cond.ends_with(':') || cond.contains('#') {
        return Err("Write the condition on one line, like met_eileen.".into());
    }
    let cond_part = if cond.is_empty() {
        String::new()
    } else {
        format!(" if {cond}")
    };
    Ok(format!("{indent}{literal}{cond_part}:{comment}"))
}

fn delete_stmt(src: &str, line: u32) -> Result<(String, u32), String> {
    let (mut lines, ended) = split_lines(src);
    let idx = line_idx(&lines, line)?;
    if !is_content(&lines[idx]) {
        return Err("That line is empty.".into());
    }
    single_line(&lines, idx)?;
    let code = code_part(&lines[idx]).to_string();
    if is_label_header(&code) {
        return Err("Delete a scene in the code view.".into());
    }
    let mut start = idx;
    if is_choice_header(&code) {
        if let Some(p) = parent_of(&lines, idx) {
            if is_menu_header(code_part(&lines[p])) {
                let ci = indent_width(&lines[idx]);
                let choices = (p + 1..stmt_end(&lines, p))
                    .filter(|i| {
                        is_content(&lines[*i])
                            && indent_width(&lines[*i]) == ci
                            && is_choice_header(code_part(&lines[*i]))
                    })
                    .count();
                if choices <= 1 {
                    start = p;
                }
            }
        }
    }
    let end = group_end(&lines, start);
    let pad = indent_of(&lines[start]);
    let parent = parent_of(&lines, start);
    lines.drain(start..end);
    let mut padded = false;
    if let Some(p) = parent {
        if stmt_end(&lines, p) == p + 1 {
            lines.insert(start, format!("{pad}pass"));
            padded = true;
        }
    }
    let focus = if padded {
        start as u32 + 1
    } else {
        (start as u32).max(1)
    };
    Ok((join_lines(&lines, ended), focus))
}

fn move_stmt(src: &str, line: u32, dir: i32) -> Result<(String, u32), String> {
    if dir != -1 && dir != 1 {
        return Err("Move a line up or down.".into());
    }
    let (lines, ended) = split_lines(src);
    let idx = line_idx(&lines, line)?;
    if !is_content(&lines[idx]) {
        return Err("That line is empty.".into());
    }
    single_line(&lines, idx)?;
    let code = code_part(&lines[idx]).to_string();
    if is_label_header(&code) {
        return Err("Scenes stay where they are.".into());
    }
    if is_branch_tail(&code) {
        return Err("Move the whole condition, not one of its branches.".into());
    }
    let ind = indent_width(&lines[idx]);
    let end = group_end(&lines, idx);
    let (next, focus) = if dir < 0 {
        let s = prev_sibling(&lines, idx).ok_or_else(|| "It is already first here.".to_string())?;
        let mut out: Vec<String> = Vec::with_capacity(lines.len());
        out.extend_from_slice(&lines[..s]);
        out.extend_from_slice(&lines[idx..end]);
        out.extend_from_slice(&lines[s..idx]);
        out.extend_from_slice(&lines[end..]);
        (out, s as u32 + 1)
    } else {
        let mut j = end;
        while j < lines.len() && !is_content(&lines[j]) {
            j += 1;
        }
        if j >= lines.len() || indent_width(&lines[j]) != ind {
            return Err("It is already last here.".into());
        }
        let e2 = group_end(&lines, j);
        let mut out: Vec<String> = Vec::with_capacity(lines.len());
        out.extend_from_slice(&lines[..idx]);
        out.extend_from_slice(&lines[j..e2]);
        out.extend_from_slice(&lines[end..j]);
        out.extend_from_slice(&lines[idx..end]);
        out.extend_from_slice(&lines[e2..]);
        (out, (idx + (e2 - j) + (j - end)) as u32 + 1)
    };
    Ok((join_lines(&next, ended), focus))
}

fn duplicate_stmt(src: &str, line: u32) -> Result<(String, u32), String> {
    let (mut lines, ended) = split_lines(src);
    let idx = line_idx(&lines, line)?;
    if !is_content(&lines[idx]) {
        return Err("That line is empty.".into());
    }
    single_line(&lines, idx)?;
    let code = code_part(&lines[idx]);
    if is_label_header(code) {
        return Err("Copy a scene in the code view.".into());
    }
    if is_branch_tail(code) {
        return Err("Copy the whole condition, not one of its branches.".into());
    }
    let end = group_end(&lines, idx);
    let copy: Vec<String> = lines[idx..end].to_vec();
    for (i, l) in copy.into_iter().enumerate() {
        lines.insert(end + i, l);
    }
    Ok((join_lines(&lines, ended), end as u32 + 1))
}

fn link_scene(
    src: &str,
    label_line: u32,
    how: LinkKind,
    target: &str,
    caption: &str,
) -> Result<(String, u32), String> {
    let (mut lines, ended) = split_lines(src);
    let h = line_idx(&lines, label_line)?;
    if !is_label_header(code_part(&lines[h])) {
        return Err("Pick a scene to add to.".into());
    }
    if how != LinkKind::Return {
        check_label(target)?;
    }
    let end = stmt_end(&lines, h);
    let pad = child_indent(&lines, h);
    let width = indent_width(&pad);
    let mut last = None;
    let mut pos = h + 1;
    while pos < end {
        if is_content(&lines[pos]) && indent_width(&lines[pos]) == width {
            last = Some(pos);
            pos = stmt_end(&lines, pos);
        } else {
            pos += 1;
        }
    }
    let last_code = last.map(|i| code_part(&lines[i]).to_string());
    let exits = last_code.as_deref().is_some_and(is_exit);
    let focus;
    match how {
        LinkKind::Jump | LinkKind::Return => {
            let text = if how == LinkKind::Jump {
                format!("{pad}jump {target}")
            } else {
                format!("{pad}return")
            };
            match last {
                Some(i) if exits => {
                    if how == LinkKind::Return {
                        return Err("This scene already ends.".into());
                    }
                    lines[i] = text;
                    focus = i as u32 + 1;
                }
                _ => {
                    lines.insert(end, text);
                    focus = end as u32 + 1;
                }
            }
        }
        LinkKind::Call => {
            let text = format!("{pad}call {target}");
            let at = match last {
                Some(i) if exits => i,
                _ => end,
            };
            lines.insert(at, text);
            focus = at as u32 + 1;
        }
        LinkKind::Choice => {
            if caption.trim().is_empty() {
                return Err("Write the choice first.".into());
            }
            if let Some(i) = last.filter(|i| is_menu_header(code_part(&lines[*i]))) {
                let joined = join_lines(&lines, ended);
                return add_choice(&joined, i as u32 + 1, caption.trim(), target);
            }
            check_label(target)?;
            let at = match last {
                Some(i) if exits => i,
                _ => end,
            };
            let mut block = vec![format!("{pad}menu:")];
            block.extend(choice_block(&deeper(&pad), caption.trim(), target));
            for (i, l) in block.into_iter().enumerate() {
                lines.insert(at + i, l);
            }
            focus = at as u32 + 2;
        }
    }
    Ok((join_lines(&lines, ended), focus))
}

fn indent_of(line: &str) -> String {
    line.chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .collect()
}

fn indent_width(line: &str) -> usize {
    indent_of(line)
        .chars()
        .map(|c| if c == '\t' { 4 } else { 1 })
        .sum()
}

fn deeper(indent: &str) -> String {
    if indent.contains('\t') {
        format!("{indent}\t")
    } else {
        format!("{indent}    ")
    }
}

fn code_part(line: &str) -> &str {
    let t = line.trim();
    match t.find(" #") {
        Some(i) => t[..i].trim(),
        None => t,
    }
}

fn is_menu_header(code: &str) -> bool {
    code == "menu:" || (code.starts_with("menu ") && code.ends_with(':'))
}

fn is_choice_header(code: &str) -> bool {
    (code.starts_with('"') || code.starts_with('\'')) && code.ends_with(':')
}

pub fn check_label(name: &str) -> Result<(), String> {
    if name.is_empty() || name.starts_with('.') || name.ends_with('.') || name.contains("..") {
        return Err("Name the scene with letters, numbers, and dots.".into());
    }
    if name.split('.').all(is_ident) {
        Ok(())
    } else {
        Err("Name the scene with letters, numbers, and dots.".into())
    }
}

pub fn check_ident(name: &str, what: &str) -> Result<(), String> {
    if is_ident(name) {
        Ok(())
    } else {
        Err(format!("Use a {what} name like eileen."))
    }
}

fn is_ident(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {
            chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        }
        _ => false,
    }
}

fn check_color(color: &str) -> Result<(), String> {
    if color.is_empty() {
        return Ok(());
    }
    let ok = color.len() <= 9 && color.chars().all(|c| c.is_ascii_hexdigit() || c == '#');
    if ok {
        Ok(())
    } else {
        Err("Use a color like #c8ffc8.".into())
    }
}

pub fn check_image_name(name: &str) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Name the image.".into());
    }
    if name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ' ')
    {
        Ok(())
    } else {
        Err("Use an image name like eileen happy.".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_say_keeps_attributes_and_indent() {
        let src = "label start:\n    e happy \"hi\"\n    return\n";
        let next = apply(
            src,
            &SceneOp::SetSay {
                line: 2,
                speaker: "f".into(),
                text: "hello \"there\"".into(),
            },
        )
        .unwrap();
        assert_eq!(
            next,
            "label start:\n    f happy \"hello \\\"there\\\"\"\n    return\n"
        );
    }

    #[test]
    fn narrator_and_back() {
        let src = "label start:\n    \"hi\"\n";
        let next = apply(
            src,
            &SceneOp::SetSay {
                line: 2,
                speaker: "e".into(),
                text: "hi".into(),
            },
        )
        .unwrap();
        assert_eq!(next, "label start:\n    e \"hi\"\n");
        let back = apply(
            &next,
            &SceneOp::SetSay {
                line: 2,
                speaker: "".into(),
                text: "yo".into(),
            },
        )
        .unwrap();
        assert_eq!(back, "label start:\n    \"yo\"\n");
    }

    #[test]
    fn choice_keeps_condition_and_retargets_jump() {
        let src = "label start:\n    menu:\n        \"Stay\" if late:\n            jump here\n";
        let next = apply(
            src,
            &SceneOp::SetChoice {
                line: 3,
                text: "Go".into(),
                target: "there".into(),
                target_line: 4,
            },
        )
        .unwrap();
        assert_eq!(
            next,
            "label start:\n    menu:\n        \"Go\" if late:\n            jump there\n"
        );
    }

    #[test]
    fn add_say_and_choice_and_label() {
        let src = "label start:\n    e \"hi\"\n";
        let src = apply(
            src,
            &SceneOp::AddSay {
                after: 2,
                speaker: "e".into(),
                text: "again".into(),
            },
        )
        .unwrap();
        assert_eq!(src, "label start:\n    e \"hi\"\n    e \"again\"\n");
        let src = apply(
            &src,
            &SceneOp::AddChoice {
                anchor: 3,
                text: "Leave".into(),
                target: "end".into(),
            },
        )
        .unwrap();
        let src = apply(&src, &SceneOp::AddLabel { name: "end".into() }).unwrap();
        assert!(src.contains("menu:\n        \"Leave\":\n            jump end\n"));
        assert!(src.contains("label end:\n    \"…\"\n    return\n"));
    }

    #[test]
    fn add_choice_extends_a_menu() {
        let src = "label start:\n    menu:\n        \"A\":\n            jump a\n    return\n";
        let next = apply(
            src,
            &SceneOp::AddChoice {
                anchor: 3,
                text: "B".into(),
                target: "b".into(),
            },
        )
        .unwrap();
        assert_eq!(
            next,
            "label start:\n    menu:\n        \"A\":\n            jump a\n        \"B\":\n            jump b\n    return\n"
        );
    }

    #[test]
    fn character_and_image_statements() {
        let src = apply(
            "",
            &SceneOp::AddCharacter {
                var: "e".into(),
                who: "Eileen".into(),
                color: "#c8ffc8".into(),
            },
        )
        .unwrap();
        assert_eq!(src, "define e = Character(\"Eileen\", color=\"#c8ffc8\")\n");
        let src = apply(
            &src,
            &SceneOp::AddImage {
                name: "side e".into(),
                file: "images/e.png".into(),
            },
        )
        .unwrap();
        assert!(src.contains("image side e = \"images/e.png\"\n"));
    }

    #[test]
    fn refuses_a_bad_scene_name() {
        let err = apply(
            src_start(),
            &SceneOp::AddLabel {
                name: "has space".into(),
            },
        )
        .unwrap_err();
        assert!(err.contains("scene"));
    }

    fn src_start() -> &'static str {
        "label start:\n    \"hi\"\n"
    }

    fn say(speaker: &str, text: &str) -> StmtSpec {
        StmtSpec::Say {
            speaker: speaker.into(),
            text: text.into(),
        }
    }

    fn run(src: &str, op: SceneOp) -> (String, u32) {
        apply_focus(src, &op).unwrap()
    }

    #[test]
    fn spec_renders_and_parses_back() {
        let specs = vec![
            StmtSpec::Scene {
                image: "bg room".into(),
                at: "".into(),
                with: "fade".into(),
            },
            StmtSpec::Show {
                image: "eileen happy".into(),
                at: "left, flip".into(),
                with: "Dissolve(0.5)".into(),
            },
            StmtSpec::Hide {
                image: "eileen".into(),
                with: "".into(),
            },
            StmtSpec::With {
                transition: "dissolve".into(),
            },
            StmtSpec::Play {
                channel: "music".into(),
                file: "audio/theme.ogg".into(),
                fadein: "1.5".into(),
                looped: true,
            },
            StmtSpec::Stop {
                channel: "music".into(),
                fadeout: "2".into(),
            },
            StmtSpec::Pause { secs: "0.5".into() },
            StmtSpec::Pause { secs: "".into() },
            StmtSpec::Jump {
                target: "end".into(),
            },
            StmtSpec::Call {
                target: "chat.a".into(),
            },
            StmtSpec::Return,
            say("e", "hi \"there\""),
        ];
        for spec in specs {
            let line = spec.render().unwrap();
            let back = parse_stmt(&line).unwrap_or_else(|| panic!("no parse for {line}"));
            assert_eq!(back, spec, "{line}");
        }
    }

    #[test]
    fn spec_refuses_what_it_cannot_write() {
        assert!(parse_present("show eileen as e2").is_none());
        assert!(parse_present("show eileen behind bob").is_none());
        assert!(parse_present("show expression \"x\"").is_none());
        assert!(parse_present("show eileen:").is_none());
        assert!(parse_present("play music [\"a.ogg\", \"b.ogg\"]").is_none());
        assert!(parse_present("hide eileen at left").is_none());
        assert!(StmtSpec::Scene {
            image: "bg\" room".into(),
            at: "".into(),
            with: "".into()
        }
        .render()
        .is_err());
        assert!(StmtSpec::Show {
            image: "e".into(),
            at: "left;".into(),
            with: "".into()
        }
        .render()
        .is_err());
        assert!(StmtSpec::With {
            transition: "x\ny".into()
        }
        .render()
        .is_err());
        assert!(StmtSpec::Play {
            channel: "music".into(),
            file: "a\".ogg".into(),
            fadein: "".into(),
            looped: false
        }
        .render()
        .is_err());
    }

    #[test]
    fn add_stmt_before_after_into() {
        let src = "label start:\n    e \"one\"\n    menu:\n        \"A\":\n            jump a\n    return\n";
        let (next, focus) = run(
            src,
            SceneOp::AddStmt {
                anchor: 3,
                place: Place::After,
                spec: say("e", "after menu"),
            },
        );
        assert_eq!(
            next,
            "label start:\n    e \"one\"\n    menu:\n        \"A\":\n            jump a\n    e \"after menu\"\n    return\n"
        );
        assert_eq!(focus, 6);
        let (next, focus) = run(
            src,
            SceneOp::AddStmt {
                anchor: 3,
                place: Place::Before,
                spec: StmtSpec::Pause { secs: "1".into() },
            },
        );
        assert!(next.contains("    e \"one\"\n    pause 1\n    menu:"));
        assert_eq!(focus, 3);
        let (next, focus) = run(
            src,
            SceneOp::AddStmt {
                anchor: 1,
                place: Place::Into,
                spec: StmtSpec::Scene {
                    image: "bg room".into(),
                    at: "".into(),
                    with: "".into(),
                },
            },
        );
        assert!(next.starts_with("label start:\n    scene bg room\n    e \"one\""));
        assert_eq!(focus, 2);
        let (next, _) = run(
            src,
            SceneOp::AddStmt {
                anchor: 4,
                place: Place::Into,
                spec: say("", "inside"),
            },
        );
        assert!(next.contains("\"A\":\n            \"inside\"\n            jump a"));
    }

    #[test]
    fn add_stmt_refuses_bad_places() {
        let src = "label start:\n    menu:\n        \"A\":\n            jump a\n";
        for (anchor, place) in [
            (1, Place::After),
            (1, Place::Before),
            (2, Place::Into),
            (3, Place::After),
        ] {
            let err = apply_focus(
                src,
                &SceneOp::AddStmt {
                    anchor,
                    place,
                    spec: StmtSpec::Return,
                },
            );
            assert!(err.is_err(), "{anchor} {place:?}");
        }
        let err = apply_focus(
            "label a:\n    \"x\"\n",
            &SceneOp::AddStmt {
                anchor: 2,
                place: Place::Into,
                spec: StmtSpec::Return,
            },
        );
        assert!(err.is_err());
    }

    #[test]
    fn add_stmt_after_an_if_clears_its_branches() {
        let src = "label a:\n    if x:\n        \"1\"\n    else:\n        \"2\"\n    return\n";
        let (next, focus) = run(
            src,
            SceneOp::AddStmt {
                anchor: 2,
                place: Place::After,
                spec: say("", "after"),
            },
        );
        assert_eq!(
            next,
            "label a:\n    if x:\n        \"1\"\n    else:\n        \"2\"\n    \"after\"\n    return\n"
        );
        assert_eq!(focus, 6);
        assert!(apply_focus(
            src,
            &SceneOp::AddStmt {
                anchor: 4,
                place: Place::After,
                spec: StmtSpec::Return,
            },
        )
        .is_err());
    }

    #[test]
    fn add_stmt_skips_a_multiline_statement() {
        let src = "label a:\n    e \"\"\"one\ntwo\"\"\"\n    return\n";
        assert!(apply_focus(
            src,
            &SceneOp::AddStmt {
                anchor: 2,
                place: Place::After,
                spec: StmtSpec::Return,
            },
        )
        .is_err());
    }

    #[test]
    fn set_stmt_keeps_comment_and_indent() {
        let src = "label a:\n    show eileen at left  # hello\n    jump b\n";
        let (next, focus) = run(
            src,
            SceneOp::SetStmt {
                line: 2,
                spec: StmtSpec::Show {
                    image: "eileen happy".into(),
                    at: "right".into(),
                    with: "dissolve".into(),
                },
            },
        );
        assert_eq!(
            next,
            "label a:\n    show eileen happy at right with dissolve  # hello\n    jump b\n"
        );
        assert_eq!(focus, 2);
        let (next, _) = run(
            src,
            SceneOp::SetStmt {
                line: 3,
                spec: StmtSpec::Jump { target: "c".into() },
            },
        );
        assert!(next.ends_with("    jump c\n"));
        let err = apply_focus(
            "label a:\n    show eileen:\n        linear 1.0 xalign 1.0\n",
            &SceneOp::SetStmt {
                line: 2,
                spec: StmtSpec::Hide {
                    image: "eileen".into(),
                    with: "".into(),
                },
            },
        );
        assert!(err.is_err());
    }

    #[test]
    fn delete_stmt_and_pass() {
        let src = "label a:\n    e \"one\"\n    menu:\n        \"X\":\n            e \"x\"\n        \"Y\":\n            jump y\n    return\n";
        let (next, focus) = run(src, SceneOp::DeleteStmt { line: 2 });
        assert_eq!(
            next,
            "label a:\n    menu:\n        \"X\":\n            e \"x\"\n        \"Y\":\n            jump y\n    return\n"
        );
        assert_eq!(focus, 1);
        let (next, _) = run(src, SceneOp::DeleteStmt { line: 4 });
        assert_eq!(
            next,
            "label a:\n    e \"one\"\n    menu:\n        \"Y\":\n            jump y\n    return\n"
        );
        let (next, _) = run("label a:\n    \"only\"\n", SceneOp::DeleteStmt { line: 2 });
        assert_eq!(next, "label a:\n    pass\n");
        // Deleting the one choice of a menu removes the menu.
        let single = "label a:\n    menu:\n        \"X\":\n            jump x\n    return\n";
        let (next, _) = run(single, SceneOp::DeleteStmt { line: 3 });
        assert_eq!(next, "label a:\n    return\n");
        assert!(apply_focus(src, &SceneOp::DeleteStmt { line: 1 }).is_err());
    }

    #[test]
    fn move_stmt_swaps_siblings_only() {
        let src =
            "label a:\n    e \"one\"\n    menu:\n        \"X\":\n            jump x\n    return\n";
        let (next, focus) = run(src, SceneOp::MoveStmt { line: 2, dir: 1 });
        assert_eq!(
            next,
            "label a:\n    menu:\n        \"X\":\n            jump x\n    e \"one\"\n    return\n"
        );
        assert_eq!(focus, 5);
        let (back, focus) = run(&next, SceneOp::MoveStmt { line: 5, dir: -1 });
        assert_eq!(back, src);
        assert_eq!(focus, 2);
        assert!(apply_focus(src, &SceneOp::MoveStmt { line: 2, dir: -1 }).is_err());
        assert!(apply_focus(src, &SceneOp::MoveStmt { line: 6, dir: 1 }).is_err());
        // Inside the choice: no sibling to cross into.
        assert!(apply_focus(src, &SceneOp::MoveStmt { line: 5, dir: -1 }).is_err());
        assert!(apply_focus(src, &SceneOp::MoveStmt { line: 1, dir: 1 }).is_err());
    }

    #[test]
    fn move_stmt_keeps_if_branches_together() {
        let src = "label a:\n    if x:\n        \"1\"\n    else:\n        \"2\"\n    \"after\"\n";
        assert!(apply_focus(src, &SceneOp::MoveStmt { line: 4, dir: 1 }).is_err());
        let (next, _) = run(src, SceneOp::MoveStmt { line: 6, dir: -1 });
        assert_eq!(
            next,
            "label a:\n    \"after\"\n    if x:\n        \"1\"\n    else:\n        \"2\"\n"
        );
    }

    #[test]
    fn duplicate_stmt_copies_the_block() {
        let src = "label a:\n    menu:\n        \"X\":\n            jump x\n    return\n";
        let (next, focus) = run(src, SceneOp::DuplicateStmt { line: 3 });
        assert_eq!(
            next,
            "label a:\n    menu:\n        \"X\":\n            jump x\n        \"X\":\n            jump x\n    return\n"
        );
        assert_eq!(focus, 5);
    }

    #[test]
    fn choice_condition_round_trip() {
        let src = "label a:\n    menu:\n        \"Stay\":  # note\n            jump x\n";
        let (with, _) = run(
            src,
            SceneOp::SetChoiceCond {
                line: 3,
                cond: "late and rich".into(),
            },
        );
        assert!(with.contains("        \"Stay\" if late and rich:  # note\n"));
        let (replaced, _) = run(
            &with,
            SceneOp::SetChoiceCond {
                line: 3,
                cond: "early".into(),
            },
        );
        assert!(replaced.contains("\"Stay\" if early:  # note"));
        let (cleared, _) = run(
            &replaced,
            SceneOp::SetChoiceCond {
                line: 3,
                cond: "".into(),
            },
        );
        assert_eq!(cleared, src);
        assert!(apply_focus(
            src,
            &SceneOp::SetChoiceCond {
                line: 3,
                cond: "a:".into()
            }
        )
        .is_err());
        assert!(apply_focus(
            src,
            &SceneOp::SetChoiceCond {
                line: 4,
                cond: "a".into()
            }
        )
        .is_err());
    }

    fn link(src: &str, how: LinkKind, target: &str, caption: &str) -> (String, u32) {
        run(
            src,
            SceneOp::LinkScene {
                label_line: 1,
                how,
                target: target.into(),
                caption: caption.into(),
            },
        )
    }

    #[test]
    fn link_scene_jump_replaces_the_ending() {
        let (next, focus) = link(
            "label a:\n    \"x\"\n    return\n\nlabel b:\n    return\n",
            LinkKind::Jump,
            "b",
            "",
        );
        assert_eq!(
            next,
            "label a:\n    \"x\"\n    jump b\n\nlabel b:\n    return\n"
        );
        assert_eq!(focus, 3);
        let (next, focus) = link(
            "label a:\n    \"x\"\n\nlabel b:\n    return\n",
            LinkKind::Jump,
            "b",
            "",
        );
        assert_eq!(
            next,
            "label a:\n    \"x\"\n    jump b\n\nlabel b:\n    return\n"
        );
        assert_eq!(focus, 3);
    }

    #[test]
    fn link_scene_call_goes_before_the_exit() {
        let (next, focus) = link("label a:\n    \"x\"\n    return\n", LinkKind::Call, "b", "");
        assert_eq!(next, "label a:\n    \"x\"\n    call b\n    return\n");
        assert_eq!(focus, 3);
        let (next, _) = link("label a:\n    \"x\"\n", LinkKind::Call, "b", "");
        assert_eq!(next, "label a:\n    \"x\"\n    call b\n");
    }

    #[test]
    fn link_scene_choice_extends_or_creates_a_menu() {
        let (next, _) = link(
            "label a:\n    menu:\n        \"A\":\n            jump a\n",
            LinkKind::Choice,
            "b",
            "B",
        );
        assert_eq!(
            next,
            "label a:\n    menu:\n        \"A\":\n            jump a\n        \"B\":\n            jump b\n"
        );
        let (next, focus) = link(
            "label a:\n    \"x\"\n    return\n",
            LinkKind::Choice,
            "b",
            "Go",
        );
        assert_eq!(
            next,
            "label a:\n    \"x\"\n    menu:\n        \"Go\":\n            jump b\n    return\n"
        );
        assert_eq!(focus, 4);
        assert!(apply_focus(
            "label a:\n    \"x\"\n",
            &SceneOp::LinkScene {
                label_line: 1,
                how: LinkKind::Choice,
                target: "b".into(),
                caption: "".into()
            }
        )
        .is_err());
    }

    #[test]
    fn link_scene_return_only_when_open() {
        let (next, _) = link("label a:\n    \"x\"\n", LinkKind::Return, "", "");
        assert_eq!(next, "label a:\n    \"x\"\n    return\n");
        assert!(apply_focus(
            "label a:\n    return\n",
            &SceneOp::LinkScene {
                label_line: 1,
                how: LinkKind::Return,
                target: "".into(),
                caption: "".into()
            }
        )
        .is_err());
    }

    #[test]
    fn add_choice_reports_the_new_choice_line() {
        let (_, focus) = run(
            "label a:\n    \"x\"\n",
            SceneOp::AddChoice {
                anchor: 2,
                text: "Go".into(),
                target: "b".into(),
            },
        );
        assert_eq!(focus, 4);
    }
}
