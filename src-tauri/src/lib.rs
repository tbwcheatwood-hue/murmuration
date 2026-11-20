mod commands;
mod discovery;
mod fit;
mod gateway;
mod mcp;
mod orchestrator;
mod state;
mod telemetry;

use tauri::Emitter;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let shared = state::initial();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(shared.clone())
        .setup(move |app| {
            let handle = app.handle().clone();
            gateway::spawn(handle.clone(), shared.clone());
            telemetry::spawn(handle.clone(), shared.clone());
            let probe_state = shared.clone();
            let probe_handle = handle.clone();
            std::thread::spawn(move || {
                let probe = discovery::probe();
                {
                    let mut st = state::write_lock(&probe_state);
                    discovery::apply(&mut st, probe);
                }
                let snap = state::read_lock(&probe_state).snapshot();
                let _ = probe_handle.emit("swarm-tick", snap);
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_snapshot,
            commands::refresh_discovery,
            commands::set_mode,
            commands::activate_blueprint,
            commands::update_params,
            commands::mount_model,
            commands::pull_model,
            commands::rebalance,
            commands::calculate_fit,
            commands::commit_shard,
            commands::set_demo_mesh,
            commands::inject_load,
            commands::set_node_status,
            commands::set_node_hardware,
            commands::plan_objective,
            commands::set_inference_active,
            commands::apply_exit_node,
            commands::set_tool_policy,
            commands::git_worktree_apply
        ])
        .run(tauri::generate_context!())
        .expect("error while running Murmuration");
}
