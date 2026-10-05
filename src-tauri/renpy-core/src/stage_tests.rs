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
    assert!(open
        .assumptions
        .iter()
        .any(|a| a.contains("could not be decided")));
    assert!(open
        .assumptions
        .iter()
        .any(|a| a.contains("first branch was used")));
    assert!(open.assumptions.iter().any(|a| a.contains("needs `time`")));
    let inside = estimate(&project, "script.rpy", 5).unwrap();
    assert_eq!(background(&inside), vec!["liv_day"]);
    assert!(
        inside
            .assumptions
            .iter()
            .all(|a| !a.contains("first branch was used")),
        "{:?}",
        inside.assumptions
    );
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
    assert!(night
        .assumptions
        .iter()
        .all(|a| !a.contains("could not be decided")));
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
    let pinned =
        super::estimate(&project, "script.rpy", 10, &[("time".into(), "12".into())]).unwrap();
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
    let night =
        super::estimate(&project, "script.rpy", 14, &[("time".into(), "22".into())]).unwrap();
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
        est.notes
            .iter()
            .any(|n| n.contains("condition could not be read")),
        "{:?}",
        est.notes
    );
    assert!(est
        .vars
        .iter()
        .all(|v| v.name != "x" && v.name != "persistent"));
    let _ = fs::remove_dir_all(root);
}
