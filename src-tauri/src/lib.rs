/// The test harness has no application manifest, so Windows loads comctl32 v5,
/// which does not export `TaskDialogIndirect`. The app binary gets v6 from Tauri.
#[cfg(all(windows, test))]
#[used]
#[link_section = ".drectve"]
static COMCTL_V6: [u8; include_bytes!("comctl.drectve").len()] = *include_bytes!("comctl.drectve");

use tauri::Manager;

mod commands;
mod edit;
mod error;
mod fs_tree;
mod git;
mod ide;
mod launch;
mod live;
mod modexport;
mod patch;
mod process;
mod pylsp;
mod sdk;
mod settings;
mod spell;
mod util;
mod watch;

fn sweep_stale_temp() {
    let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !(name.starts_with("vn-ide-") || name.starts_with("vnide-live-")) {
            continue;
        }
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        // A live dir's files are rewritten in place, which does not touch the
        // folder's own mtime. Judge age by the newest thing inside it too.
        let mut newest = entry.metadata().and_then(|m| m.modified()).ok();
        if let Ok(children) = std::fs::read_dir(&path) {
            for child in children.flatten() {
                if let Ok(t) = child.metadata().and_then(|m| m.modified()) {
                    if newest.is_none_or(|n| t > n) {
                        newest = Some(t);
                    }
                }
            }
        }
        let Some(newest) = newest else { continue };
        if newest.elapsed().unwrap_or_default() > std::time::Duration::from_secs(6 * 60 * 60) {
            let _ = std::fs::remove_dir_all(&path);
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    sweep_stale_temp();
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(commands::AppState::default())
        .invoke_handler(tauri::generate_handler![
            commands::initial_project,
            commands::initial_label,
            commands::initial_actions,
            commands::engine_dump,
            commands::engine_lint,
            commands::stage_images,
            commands::open_project,
            commands::get_project_info,
            commands::get_project_map,
            commands::get_diagnostics,
            commands::get_label_graph,
            commands::get_label_lines,
            commands::read_file,
            commands::edit_status,
            commands::write_file,
            commands::revert_file,
            commands::check_syntax,
            commands::initial_patch,
            commands::initial_revert,
            commands::launch_game,
            commands::warp_to,
            commands::archive_fingerprints,
            commands::layout_cache_get,
            commands::layout_cache_put,
            commands::archive_list,
            commands::archive_read_entry,
            commands::archive_extract,
            commands::archive_build,
            commands::archive_cancel,
            commands::patch_bake,
            commands::patch_undo,
            commands::patch_remove,
            commands::patch_rebase,
            commands::mod_toggles_get,
            commands::mod_toggles_set,
            commands::mod_export,
            commands::save_list,
            commands::save_inspect,
            ide::get_catalog,
            ide::get_translations,
            ide::label_routes,
            ide::search_project,
            ide::replace_text,
            ide::find_references,
            ide::resolve_symbol,
            ide::preview_rename,
            ide::rename_symbol,
            ide::create_script,
            ide::rename_script,
            ide::delete_script,
            ide::update_translation,
            ide::asset_report,
            ide::list_dir,
            fs_tree::fs_create,
            fs_tree::fs_rename,
            fs_tree::fs_move,
            fs_tree::fs_delete,
            fs_tree::fs_reveal,
            ide::read_asset,
            ide::read_preview,
            ide::read_logs,
            ide::autoreload_enabled,
            ide::set_autoreload,
            ide::stage_at,
            ide::scene_edit,
            ide::scene_undo,
            ide::scene_redo,
            ide::scene_history,
            ide::scene_parse,
            ide::create_project,
            sdk::sdk_list,
            sdk::sdk_set_folder,
            sdk::sdk_add,
            sdk::sdk_remove,
            sdk::sdk_catalog,
            sdk::sdk_install,
            sdk::sdk_install_web,
            sdk::sdk_cancel,
            sdk::build_pc,
            sdk::build_web,
            sdk::build_cancel,
            sdk::reveal_path,
            git::git_status,
            git::git_init,
            git::git_write_ignore,
            git::git_ignore,
            git::git_branches,
            git::git_switch,
            git::git_create_branch,
            git::git_fetch,
            git::git_pull,
            git::git_push,
            git::git_add_remote,
            git::git_commit,
            git::git_undo_commit,
            git::git_stage,
            git::git_unstage,
            git::git_discard,
            git::git_show,
            git::git_stage_text,
            git::git_log,
            git::git_commit_files,
            pylsp::pylsp_status,
            pylsp::pylsp_env,
            pylsp::pylsp_install,
            pylsp::pylsp_cancel,
            pylsp::pylsp_remove,
            pylsp::pylsp_start,
            pylsp::pylsp_send,
            pylsp::pylsp_stop,
            spell::spell_check,
            spell::spell_suggest,
            spell::spell_add_word,
            spell::spell_words,
            spell::spell_remove_word,
            settings::settings_export,
            settings::settings_import,
            live::live_start,
            live::live_replay,
            live::live_jump,
            live::live_jump_label,
            live::live_reload,
            live::live_images,
            live::live_stop,
            live::live_set_watch,
            live::live_shots,
            live::live_shot,
            ide::diagnostic_bundle,
        ])
        .build(tauri::generate_context!())
        .expect("error while running Ren'Inspector");
    app.run(|app_handle, event| {
        if matches!(event, tauri::RunEvent::Exit) {
            live::on_exit(app_handle);
            if let Some(state) = app_handle.try_state::<commands::AppState>() {
                pylsp::stop(&state);
            }
        }
    });
}
