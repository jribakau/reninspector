//! Search, file operations, symbols, assets, translations, logs and project lifecycle.

use crate::error::AppError;
use std::path::{Path, PathBuf};

pub(crate) fn script_rel(rel: &str) -> Result<String, AppError> {
    let rel = rel.trim().replace('\\', "/");
    if rel.is_empty()
        || rel.starts_with('/')
        || rel.contains(':')
        || rel
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == "..")
    {
        return Err("That path is not inside game/.".into());
    }
    if !(rel.ends_with(".rpy") || rel.ends_with(".rpym")) {
        return Err("Script files must end in .rpy.".into());
    }
    Ok(rel)
}

pub(crate) fn abs_under(game_dir: &Path, rel: &str) -> PathBuf {
    game_dir.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR))
}

mod assets;
mod diag;
mod project;
mod scene;
mod scripts;
mod search;

pub use assets::*;
pub use diag::*;
pub use project::*;
pub use scene::*;
pub use scripts::*;
pub use search::*;

#[cfg(test)]
mod tests {
    use std::fs;

    use super::assets::{
        append_archived_assets, asset_rel, merge_listing, preview_kind, with_game_prefix,
    };
    use super::diag::{hide_home, tail_file};
    use super::project::first_script;
    use super::scene::{AUTORELOAD, AUTORELOAD_FILE};
    use super::search::same_text;
    use super::*;

    #[test]
    fn autoreload_is_out_of_game_during_a_build_and_back_after() {
        let game = std::env::temp_dir().join(format!("vnide-autoreload-{}", std::process::id()));
        let _ = fs::remove_dir_all(&game);
        fs::create_dir_all(&game).unwrap();
        let rpy = game.join(AUTORELOAD_FILE);
        fs::write(&rpy, AUTORELOAD).unwrap();
        fs::write(rpy.with_extension("rpyc"), b"compiled").unwrap();
        {
            let _aside = AutoreloadAside::take(&game).unwrap();
            assert!(!rpy.exists());
            assert!(!rpy.with_extension("rpyc").exists());
        }
        assert_eq!(fs::read_to_string(&rpy).unwrap(), AUTORELOAD);

        fs::remove_file(&rpy).unwrap();
        fs::write(rpy.with_extension("rpyc"), b"orphan").unwrap();
        drop(AutoreloadAside::take(&game).unwrap());
        assert!(!rpy.exists());
        assert!(!rpy.with_extension("rpyc").exists());
        let _ = fs::remove_dir_all(&game);
    }

    #[test]
    fn diagnostics_hide_the_home_folder() {
        let text = "C:\\Users\\ann\\game\\x.rpy and C:/Users/ann/game/y.rpy";
        assert_eq!(
            hide_home(text, Some("C:\\Users\\ann")),
            "~\\game\\x.rpy and ~/game/y.rpy"
        );
        assert_eq!(hide_home(text, None), text);
        assert_eq!(hide_home(text, Some("/")), text);
    }

    #[test]
    fn tail_keeps_only_the_end_of_a_file() {
        let path = std::env::temp_dir().join(format!("vnide-tail-{}.txt", std::process::id()));
        fs::write(&path, "one\ntwo\nthree\nfour\n").unwrap();
        assert_eq!(tail_file(&path, 1000).unwrap(), "one\ntwo\nthree\nfour\n");
        // Starts mid-line, so the partial first line is dropped.
        assert_eq!(tail_file(&path, 10).unwrap(), "four\n");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn replace_rejects_a_file_that_changed() {
        let before = "hello world\n";
        assert!(same_text(before.as_bytes(), before));
        assert!(!same_text(b"hello there\n", before));
        assert!(same_text(b"\xEF\xBB\xBFhello world\n", before));
    }

    #[test]
    fn first_scene_is_reached_from_start() {
        assert_eq!(
            first_script("start"),
            "label start:\n    \"…\"\n    return\n"
        );
        let named = first_script("intro");
        assert!(
            named.starts_with("label start:\n    jump intro\n\nlabel intro:\n"),
            "{named}"
        );
        let parsed = renpy_core::parser::check_syntax(&named);
        assert!(parsed.is_empty(), "{parsed:?}");
    }

    #[test]
    fn sdk_root_is_found_from_each_launcher() {
        let sdk = std::env::temp_dir().join(format!("vnide-sdk-root-{}", std::process::id()));
        let _ = fs::remove_dir_all(&sdk);
        fs::create_dir_all(sdk.join("launcher")).unwrap();
        fs::create_dir_all(sdk.join("gui").join("game")).unwrap();
        assert_eq!(
            crate::sdk::sdk_root(&sdk.join("renpy.exe")),
            Some(sdk.clone())
        );
        assert_eq!(
            crate::sdk::sdk_root(&sdk.join("renpy.sh")),
            Some(sdk.clone())
        );
        let mac = sdk
            .join("renpy.app")
            .join("Contents")
            .join("MacOS")
            .join("renpy");
        assert_eq!(crate::sdk::sdk_root(&mac), Some(sdk.clone()));
        assert_eq!(
            crate::sdk::sdk_root(
                &sdk.join("launcher")
                    .join("x")
                    .join("y")
                    .join("z")
                    .join("w")
                    .join("renpy")
            ),
            None
        );
        let _ = fs::remove_dir_all(&sdk);
    }

    /// Needs an installed SDK:
    ///   VNIDE_SDK=C:\renpy-8.5.2-sdk\renpy.exe cargo test --lib -- --ignored sdk_generates
    #[test]
    #[ignore]
    fn sdk_generates_the_standard_gui() {
        let exe = PathBuf::from(std::env::var("VNIDE_SDK").expect("VNIDE_SDK"));
        let root = std::env::temp_dir()
            .join(format!("vnide-sdk-gen-{}", std::process::id()))
            .join("My Game");
        let _ = fs::remove_dir_all(root.parent().unwrap());
        fs::create_dir_all(root.join("game")).unwrap();
        crate::sdk::generate_from_sdk(None, None, None, &exe, &root).unwrap();
        let game = root.join("game");
        for file in [
            "gui.rpy",
            "screens.rpy",
            "options.rpy",
            "gui/textbox.png",
            "gui/button/idle_background.png",
        ] {
            assert!(game.join(file).is_file(), "missing {file}");
        }
        let options = fs::read_to_string(game.join("options.rpy")).unwrap();
        assert!(
            options.contains("define config.name = _(\"My Game\")"),
            "{options}"
        );
        let _ = fs::remove_dir_all(root.parent().unwrap());
    }

    #[test]
    fn archived_images_join_the_asset_list() {
        let mut files = vec![AssetFile {
            path: "images/loose.png".into(),
            kind: "image".into(),
            bytes: 1,
            used: false,
        }];
        let mut total = 1u32;
        append_archived_assets(
            &mut files,
            &mut total,
            &[
                ("images/loose.png".into(), 9),
                ("images\\bg/sayori.png".into(), 20),
                ("audio/music.ogg".into(), 30),
                ("scripts/script.rpy".into(), 4),
                ("images/bg/sayori.png".into(), 99),
            ],
        );
        assert_eq!(total, 3);
        assert_eq!(files.len(), 3);
        assert_eq!(
            files
                .iter()
                .filter(|f| f.path == "images/loose.png")
                .count(),
            1
        );
        assert!(files
            .iter()
            .any(|f| f.path == "images/bg/sayori.png" && f.bytes == 20));
        assert!(files
            .iter()
            .any(|f| f.path == "audio/music.ogg" && f.kind == "audio"));
    }

    #[test]
    fn archive_path_lists_as_directory_then_file() {
        let names = vec!["scripts/story/a.rpy".into()];
        let parent = merge_listing("scripts", vec![], &names);
        assert_eq!(
            parent,
            vec![DirEntry {
                name: "story".into(),
                path: "scripts/story".into(),
                dir: true,
            }]
        );
        let child = merge_listing("scripts/story", vec![], &names);
        assert_eq!(
            child,
            vec![DirEntry {
                name: "a.rpy".into(),
                path: "scripts/story/a.rpy".into(),
                dir: false,
            }]
        );
    }

    #[test]
    fn disk_entry_hides_the_same_archive_child() {
        let disk = vec![DirEntry {
            name: "Scripts".into(),
            path: "Scripts".into(),
            dir: true,
        }];
        let merged = merge_listing("", disk, &["scripts/story/a.rpy".into()]);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].name, "Scripts");
        assert!(merged[0].dir);
    }

    #[test]
    fn listing_skips_cache_saves_and_shims() {
        let names = vec![
            "cache/x.png".into(),
            "saves/1.save".into(),
            "vnide_live.rpy".into(),
            "script.rpy".into(),
        ];
        let root = merge_listing("", vec![], &names);
        assert_eq!(root.len(), 1);
        assert_eq!(root[0].name, "script.rpy");
        assert!(!root[0].dir);
    }

    #[test]
    fn archive_entries_appear_under_the_game_folder() {
        let names = vec![with_game_prefix("game", "scripts/story/a.rpy")];
        let root = merge_listing(
            "",
            vec![DirEntry {
                name: "game".into(),
                path: "game".into(),
                dir: true,
            }],
            &names,
        );
        assert_eq!(root.len(), 1);
        assert_eq!(root[0].path, "game");
        let inside = merge_listing("game", vec![], &names);
        assert_eq!(inside[0].name, "scripts");
        assert_eq!(inside[0].path, "game/scripts");
        assert!(inside[0].dir);
    }

    #[test]
    fn asset_paths_stay_inside_game() {
        assert_eq!(
            asset_rel("images/bg.png").as_deref().ok(),
            Some("images/bg.png")
        );
        assert!(asset_rel(r"C:/Users/x.png").is_err());
        assert!(asset_rel(r"C:\Users\x.png").is_err());
        assert!(asset_rel("images/../../secret.png").is_err());
        assert!(asset_rel("images/../../../windows/notepad.exe").is_err());
        assert!(asset_rel("notes.txt").is_err());
    }

    #[test]
    fn preview_kinds_cover_text_image_sound_and_video() {
        assert_eq!(preview_kind("notes.md"), Some("text"));
        assert_eq!(preview_kind("shot.SVG"), Some("image"));
        assert_eq!(preview_kind("voice.flac"), Some("audio"));
        assert_eq!(preview_kind("clip.webm"), Some("video"));
        assert_eq!(preview_kind("archive.rpa"), None);
    }
}
