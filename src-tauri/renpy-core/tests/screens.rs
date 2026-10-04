//! Screen-driven navigation and compiled-only detection on a tiny synthetic project.

use std::fs;
use std::path::PathBuf;

use renpy_core::Project;

fn project(name: &str, files: &[(&str, &str)]) -> (PathBuf, Project) {
    let root = std::env::temp_dir().join(format!("vn-ide-test-{name}-{}", std::process::id()));
    let game = root.join("game");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&game).unwrap();
    for (rel, text) in files {
        let p = game.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, text).unwrap();
    }
    let p = Project::open(&root).unwrap();
    (root, p)
}

#[test]
fn screens_become_map_nodes_with_action_edges() {
    let script = r#"
screen town_map():
    use hud
    textbutton "Shop" action Jump("shop")
    textbutton "Home" action Jump("home")
    textbutton "Gone" action Jump("nowhere")

screen hud():
    textbutton "Phone" action Jump("phone")

screen say_box():
    text "just ui"

label start:
    show screen say_box
    call screen town_map
    return

label shop:
    "shop"
    return

label home:
    "home"
    return

label phone:
    "phone"
    return

label orphan:
    return
"#;
    let (root, p) = project("screens", &[("script.rpy", script)]);
    let map = &p.analysis.map;
    let ids: Vec<&str> = map.nodes.iter().map(|n| n.id.as_str()).collect();
    assert!(ids.contains(&"screen:town_map"));
    assert!(ids.contains(&"screen:hud"));
    assert!(
        !ids.contains(&"screen:say_box"),
        "pure UI screens stay off the map"
    );

    let has = |from: &str, to: &str, kind: &str| {
        map.edges
            .iter()
            .any(|e| e.from == from && e.to == to && e.kind == kind)
    };
    assert!(has("start", "screen:town_map", "screen"));
    assert!(has("screen:town_map", "shop", "action"));
    assert!(has("screen:town_map", "screen:hud", "screen"));
    assert!(has("screen:hud", "phone", "action"));

    // Labels reached only through the screen are reachable; the orphan is not.
    let reach = |id: &str| map.nodes.iter().find(|n| n.id == id).unwrap().reachable;
    assert!(reach("shop") && reach("phone"));
    assert!(!reach("orphan"));

    // A Jump to a label that does not exist is reported against the screen.
    let missing = p
        .analysis
        .diagnostics
        .items
        .iter()
        .find(|d| d.code == "missing-label")
        .expect("missing label");
    assert!(missing.message.contains("screen town_map") && missing.message.contains("nowhere"));

    // The label graph shows the screen with jump-out nodes for its actions.
    let g = p.label_graph("start", false).unwrap();
    let screen = g
        .nodes
        .iter()
        .find(|n| n.target.as_deref() == Some("town_map"))
        .unwrap();
    assert!(screen.targets.contains(&"shop".to_string()));
    assert!(
        g.nodes
            .iter()
            .filter(|n| n.target.as_deref() == Some("shop"))
            .count()
            >= 1
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn compiled_only_scripts_soften_missing_labels() {
    let (root, p) = project(
        "compiled",
        &[
            ("script.rpy", "label start:\n    jump hidden\n"),
            ("secret.rpyc", "x"),
            ("both.rpy", "label both:\n    return\n"),
            ("both.rpyc", "x"),
        ],
    );
    // Each entry names the file, then why it could not be decompiled.
    assert_eq!(p.compiled_only.len(), 1);
    assert!(
        p.compiled_only[0].starts_with("secret.rpyc ("),
        "{:?}",
        p.compiled_only
    );
    let d = p
        .analysis
        .diagnostics
        .items
        .iter()
        .find(|d| d.code == "missing-label")
        .unwrap();
    assert_eq!(d.severity, "warning");
    assert!(p
        .analysis
        .diagnostics
        .items
        .iter()
        .any(|d| d.code == "compiled-only"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn engine_labels_resolve_missing_targets() {
    use renpy_core::{EngineDump, EngineRun};
    let (root, mut p) = project(
        "engine",
        &[("script.rpy", "label start:\n    jump hidden\n")],
    );
    assert!(p
        .analysis
        .diagnostics
        .items
        .iter()
        .any(|d| d.code == "missing-label"));
    let mut dump = EngineDump::default();
    dump.labels.insert("start".into(), ("script.rpy".into(), 1));
    dump.labels.insert("hidden".into(), ("vault.rpy".into(), 7));
    p.set_engine(EngineRun {
        dump,
        ..Default::default()
    });
    assert!(!p
        .analysis
        .diagnostics
        .items
        .iter()
        .any(|d| d.code == "missing-label"));
    let hidden = p
        .analysis
        .map
        .nodes
        .iter()
        .find(|n| n.id == "hidden")
        .unwrap();
    assert_eq!(hidden.kind, "compiled");
    assert!(p.analysis.engine_diff.as_ref().unwrap().is_empty());
    let _ = fs::remove_dir_all(root);
}
