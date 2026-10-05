use super::*;

#[test]
fn engine_spec_round_trips_a_decompiled_line() {
    let root = std::env::temp_dir().join(format!("vnide-engine-spec-{}", std::process::id()));
    let game = root.join("game");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&game).unwrap();
    fs::write(game.join("script.rpy"), "label start:\n    \"hi\"\n").unwrap();
    let mut project = Project::open(&root).unwrap();
    assert_eq!(
        project.engine_spec("script.rpy", 2),
        ("script.rpy".into(), 2)
    );
    assert!(project.ide_line("script.rpy", 2).is_none());
    project.files[0].engine = Some(("script.rpy".into(), vec![10, 10, 12, 12]));
    assert_eq!(
        project.engine_spec("script.rpy", 1),
        ("script.rpy".into(), 10)
    );
    assert_eq!(
        project.engine_spec("script.rpy", 3),
        ("script.rpy".into(), 12)
    );
    assert_eq!(
        project
            .ide_line("game/script.rpy", 12)
            .as_ref()
            .map(|(f, l)| (f.as_str(), *l)),
        Some(("script.rpy", 3))
    );
    assert_eq!(
        project
            .ide_line(r"C:\proj\game\script.rpy", 10)
            .as_ref()
            .map(|(f, l)| (f.as_str(), *l)),
        Some(("script.rpy", 1))
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn init_key_ignores_dialogue_and_tracks_init_and_images() {
    let root = std::env::temp_dir().join(format!("vnide-init-key-{}", std::process::id()));
    let game = root.join("game");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&game).unwrap();
    let script = game.join("script.rpy");
    let write = |text: &str| fs::write(&script, text).unwrap();
    write("label start:\n    \"hello\"\nimage eileen = \"e.png\"\ninit python:\n    x = 1\n");
    let key = Project::open(&root).unwrap().init_key();
    write("label start:\n    \"world\"\nimage eileen = \"e.png\"\ninit python:\n    x = 1\n");
    assert_eq!(Project::open(&root).unwrap().init_key(), key, "dialogue");
    write("# a comment\nlabel begin:\n    \"world\"\nimage eileen = \"e.png\"\ninit python:\n    x = 1\n");
    assert_eq!(
        Project::open(&root).unwrap().init_key(),
        key,
        "comment and same-size label"
    );
    write("# a comment\nlabel begin:\n    \"world\"\nimage eileen = \"e.png\"\ninit python:\n    x = 2\n");
    let edited = Project::open(&root).unwrap().init_key();
    assert_ne!(edited, key, "init python");
    write("# a comment\nlabel begin:\n    \"world\"\nimage eileen = \"f.png\"\ninit python:\n    x = 2\n");
    let image = Project::open(&root).unwrap().init_key();
    assert_ne!(image, edited, "image statement");
    fs::create_dir_all(game.join("images")).unwrap();
    fs::write(game.join("images").join("e.png"), b"png").unwrap();
    assert_ne!(
        Project::open(&root).unwrap().init_key(),
        image,
        "image file"
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn source_key_changes_when_an_edit_keeps_the_same_length() {
    let root = std::env::temp_dir().join(format!("vnide-source-key-{}", std::process::id()));
    let game = root.join("game");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&game).unwrap();
    let script = game.join("script.rpy");
    fs::write(&script, "label start:\n    \"hello\"\n").unwrap();
    let before = Project::open(&root).unwrap().source_key();
    fs::write(&script, "label start:\n    \"world\"\n").unwrap();
    let after = Project::open(&root).unwrap().source_key();
    assert_ne!(before, after);
    assert_eq!(
        fs::metadata(&script).unwrap().len(),
        "label start:\n    \"hello\"\n".len() as u64
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn scripts_sort_shallower_paths_first() {
    let root = std::env::temp_dir().join(format!("vnide-sort-{}", std::process::id()));
    let game = root.join("game");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(game.join("chapter")).unwrap();
    fs::write(game.join("z.rpy"), "label z:\n    return\n").unwrap();
    fs::write(game.join("a.rpy"), "label a:\n    return\n").unwrap();
    fs::write(game.join("chapter").join("b.rpy"), "label b:\n    return\n").unwrap();
    let project = Project::open(&root).unwrap();
    let rels: Vec<_> = project.files.iter().map(|f| f.rel.as_str()).collect();
    assert_eq!(rels, ["a.rpy", "z.rpy", "chapter/b.rpy"]);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_stale_analysis_does_not_replace_a_newer_one() {
    let root = std::env::temp_dir().join(format!("vnide-epoch-{}", std::process::id()));
    let game = root.join("game");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&game).unwrap();
    fs::write(game.join("script.rpy"), "label start:\n    return\n").unwrap();
    let mut project = Project::open(&root).unwrap();
    let first = project.fork_analysis();
    let second = project.fork_analysis();
    let old = first.run();
    let new = second.run();
    assert!(!project.publish_analysis(first.epoch, old));
    assert!(project.publish_analysis(second.epoch, new));
    assert!(project.analysis.by_name.contains_key("start"));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn game_info_reads_config_layout_and_warns_on_85() {
    let root = std::env::temp_dir().join(format!("vnide-game-info-{}", std::process::id()));
    let game = root.join("game");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("renpy")).unwrap();
    fs::create_dir_all(&game).unwrap();
    fs::write(
        root.join("renpy").join("vc_version.py"),
        "version = '8.5.2'\n",
    )
    .unwrap();
    fs::write(game.join("script_version.txt"), "(8, 4, 1)\n").unwrap();
    fs::write(
            game.join("options.rpy"),
            "define config.name = _(\"Demo\")\ndefine config.version = \"1.2\"\ndefine config.save_directory = \"Demo-1\"\ndefine build.name = \"demo-pkg\"\n",
        )
        .unwrap();
    fs::write(game.join("script.rpy"), "label start:\n    return\n").unwrap();
    fs::write(game.join("scripts.rpa"), b"not-an-archive").unwrap();
    let info = Project::open(&root).unwrap().game_info();
    assert_eq!(info.engine_version.as_deref(), Some("8.5.2"));
    assert_eq!(info.script_version.as_deref(), Some("8.4.1"));
    assert_eq!(info.name.as_deref(), Some("Demo"));
    assert_eq!(info.version.as_deref(), Some("1.2"));
    assert_eq!(info.build_name.as_deref(), Some("demo-pkg"));
    assert_eq!(info.save_directory.as_deref(), Some("Demo-1"));
    assert_eq!(info.layout.loose_scripts, 2);
    assert_eq!(info.layout.archives, 1);
    assert_eq!(info.archives.len(), 1);
    assert!(info.archives[0].sha256.is_none());
    assert!(info.files_newest.is_some());
    assert!(info.notes.iter().any(|n| n.contains("8.5")));
    fs::write(game.join("odd.rpa"), b"XYZ-9.9 nope\n").unwrap();
    fs::write(
        game.join("script.rpy"),
        "label start:\n    return\ninit python:\n    config.archive_handlers.append(None)\n",
    )
    .unwrap();
    let again = Project::open(&root).unwrap();
    let odd = again
        .archives
        .iter()
        .find(|a| a.info.path == "odd.rpa")
        .unwrap();
    let err = odd.info.error.as_deref().unwrap_or("");
    assert!(err.contains("unsupported"), "{err}");
    assert!(err.contains("script.rpy"), "{err}");
    assert_eq!(odd.info.version, "XYZ-9.9");
    assert!(!version_at_least(Some("8.4.1"), 8, 5));
    assert!(version_at_least(Some("8.5.0"), 8, 5));
    let hashed = sha256_file(&game.join("scripts.rpa")).unwrap();
    assert_eq!(hashed.len(), 64);
    let _ = fs::remove_dir_all(&root);
}
