//! The desktop app's backend: device list, agent proxy, console streams, SSH tunnels and
//! server-list pings for friend mode. The UI talks only to these commands.

mod agent;
mod console;
mod devices;
mod mods;
mod ping;
mod store;

use tauri::Manager;

pub fn run() {
    let result = tauri::Builder::default()
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            app.manage(store::Store::load(dir));
            app.manage(devices::Tunnels::default());
            app.manage(console::Consoles::default());
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
            ping::ping,
            mods::mods_index,
        ])
        .run(tauri::generate_context!());
    if let Err(err) = result {
        eprintln!("app failed: {err}");
        std::process::exit(1);
    }
}
