//! The desktop app's backend: device list, agent proxy, console streams, SSH tunnels and
//! server-list pings for friend mode. The UI talks only to these commands.

mod agent;
mod console;
mod deploy;
mod devices;
mod install;
mod join;
mod mods;
mod packs;
mod ping;
mod store;

use tauri::Manager;

pub fn run() {
    let result = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            app.manage(store::Store::load(dir));
            app.manage(devices::Tunnels::default());
            app.manage(console::Consoles::default());
            app.manage(join::JoinLock::default());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            devices::devices_list,
            devices::device_add,
            devices::device_remove,
            agent::agent,
            console::console_open,
            console::console_send,
            console::console_close,
            ping::friends_list,
            ping::friend_add,
            ping::friend_remove,
            join::friend_join,
            ping::ping,
            mods::mods_index,
            deploy::build_inspect,
            deploy::deploy,
            deploy::mod_upload,
            install::install_agent,
            packs::packs_list,
            packs::pack_add,
            packs::pack_remove,
            packs::pack_add_mod,
            packs::pack_remove_mod,
            packs::pack_build,
        ])
        .run(tauri::generate_context!());
    if let Err(err) = result {
        eprintln!("app failed: {err}");
        std::process::exit(1);
    }
}
