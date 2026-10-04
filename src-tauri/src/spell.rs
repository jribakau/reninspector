//! Dialogue spell check against a bundled English dictionary.
//!
//! Extra words live in `<project>/.vnide/words.txt`, outside `game/`, so a
//! build does not ship them and the list can still be committed.

use std::collections::HashSet;
use std::fs;
use std::sync::OnceLock;

use serde::Serialize;
use spellbook::Dictionary;
use tauri::State;

use crate::commands::AppState;

const AFF: &str = include_str!("../dictionaries/en_US.aff");
const DIC: &str = include_str!("../dictionaries/en_US.dic");
const MAX_HITS: usize = 2000;
const MAX_TEXT: usize = 500_000;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpellHit {
    pub line: u32,
    pub from: u32,
    pub to: u32,
    pub word: String,
}

fn dictionary() -> Result<&'static Dictionary, String> {
    static DICT: OnceLock<Dictionary> = OnceLock::new();
    if let Some(dict) = DICT.get() {
        return Ok(dict);
    }
    let parsed = Dictionary::new(AFF, DIC)
        .map_err(|e| format!("Could not read the spell check dictionary: {e}"))?;
    Ok(DICT.get_or_init(|| parsed))
}

fn no_project() -> String {
    "No project is open.".into()
}

fn project_root(state: &State<'_, AppState>) -> Result<std::path::PathBuf, String> {
    let guard = state.project.lock().map_err(|e| e.to_string())?;
    Ok(guard.as_ref().ok_or_else(no_project)?.root.clone())
}

fn word_list(root: &std::path::Path) -> HashSet<String> {
    let text = fs::read_to_string(root.join(".vnide").join("words.txt")).unwrap_or_default();
    text.lines()
        .map(|line| line.trim().to_lowercase())
        .filter(|line| !line.is_empty())
        .collect()
}

fn symbol_names(state: &State<'_, AppState>) -> Result<HashSet<String>, String> {
    let guard = state.project.lock().map_err(|e| e.to_string())?;
    let project = guard.as_ref().ok_or_else(no_project)?;
    let mut names = HashSet::new();
    for symbol in &project.analysis.catalog.symbols {
        names.insert(symbol.name.to_lowercase());
    }
    for variable in &project.analysis.catalog.variables {
        names.insert(variable.name.to_lowercase());
    }
    Ok(names)
}

fn all_caps(word: &str) -> bool {
    let mut letters = 0;
    for ch in word.chars() {
        if !ch.is_alphabetic() {
            continue;
        }
        letters += 1;
        if !ch.is_uppercase() {
            return false;
        }
    }
    letters > 1
}

fn known_name(word: &str, names: &HashSet<String>, extra: &HashSet<String>) -> bool {
    let lower = word.to_lowercase();
    if names.contains(&lower) || extra.contains(&lower) {
        return true;
    }
    lower
        .split('\'')
        .next()
        .is_some_and(|stem| !stem.is_empty() && (names.contains(stem) || extra.contains(stem)))
}

fn clean_word(word: &str) -> Result<String, String> {
    let word = word.trim();
    if word.is_empty() || word.chars().count() > 64 {
        return Err("That word cannot be added.".into());
    }
    let ok = word
        .chars()
        .all(|c| c.is_alphabetic() || c == '\'' || c == '-' || c == '’')
        && word.chars().any(|c| c.is_alphabetic());
    if !ok {
        return Err("That word cannot be added.".into());
    }
    Ok(word.to_string())
}

#[tauri::command(async)]
pub fn spell_check(state: State<'_, AppState>, text: String) -> Result<Vec<SpellHit>, String> {
    if text.len() > MAX_TEXT {
        return Ok(Vec::new());
    }
    let dict = dictionary()?;
    let names = symbol_names(&state)?;
    let root = project_root(&state)?;
    let extra = word_list(&root);
    let mut hits = Vec::new();
    for word in renpy_core::prose::prose_words(&text) {
        if hits.len() >= MAX_HITS {
            break;
        }
        if word.word.chars().any(|c| c.is_numeric())
            || all_caps(&word.word)
            || known_name(&word.word, &names, &extra)
        {
            continue;
        }
        if dict.check(&word.word) {
            continue;
        }
        hits.push(SpellHit {
            line: word.line,
            from: word.from,
            to: word.to,
            word: word.word,
        });
    }
    Ok(hits)
}

#[tauri::command(async)]
pub fn spell_suggest(word: String) -> Result<Vec<String>, String> {
    let word = word.trim();
    if word.is_empty() || word.len() > 64 {
        return Ok(Vec::new());
    }
    let dict = dictionary()?;
    let mut out = Vec::new();
    dict.suggest(word, &mut out);
    out.truncate(8);
    Ok(out)
}

#[tauri::command(async)]
pub fn spell_add_word(state: State<'_, AppState>, word: String) -> Result<(), String> {
    let word = clean_word(&word)?;
    let root = project_root(&state)?;
    let dir = root.join(".vnide");
    fs::create_dir_all(&dir).map_err(|e| format!("Could not create .vnide: {e}"))?;
    let path = dir.join("words.txt");
    let mut existing = fs::read_to_string(&path).unwrap_or_default();
    if existing
        .lines()
        .any(|line| line.trim().eq_ignore_ascii_case(&word))
    {
        return Ok(());
    }
    if !existing.is_empty() && !existing.ends_with('\n') {
        existing.push('\n');
    }
    existing.push_str(&word);
    existing.push('\n');
    fs::write(&path, existing).map_err(|e| format!("Could not save the word list: {e}"))
}

/// Words the author added to this project, in the order they were added.
#[tauri::command(async)]
pub fn spell_words(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    let root = project_root(&state)?;
    let text = fs::read_to_string(root.join(".vnide").join("words.txt")).unwrap_or_default();
    Ok(text
        .lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect())
}

#[tauri::command(async)]
pub fn spell_remove_word(state: State<'_, AppState>, word: String) -> Result<(), String> {
    let root = project_root(&state)?;
    let path = root.join(".vnide").join("words.txt");
    let Ok(existing) = fs::read_to_string(&path) else {
        return Ok(());
    };
    let word = word.trim();
    let kept: Vec<&str> = existing
        .lines()
        .filter(|line| !line.trim().eq_ignore_ascii_case(word))
        .collect();
    let mut out = kept.join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    fs::write(&path, out).map_err(|e| format!("Could not save the word list: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dictionary_knows_ordinary_words() {
        let dict = dictionary().unwrap();
        assert!(dict.check("hello"));
        assert!(!dict.check("notawordxyz"));
    }
}
