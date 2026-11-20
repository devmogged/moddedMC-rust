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
mod runtime;
mod store;

use tauri::{
    Manager, WindowEvent,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};

fn show_main(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Closing the window keeps the app in the tray so servers stay up; Quit in the tray menu stops
/// the servers and everything else, then exits.
fn tray(app: &tauri::App) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open moddedMC", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit and stop servers", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;
    let mut builder = TrayIconBuilder::new()
        .tooltip("moddedMC")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show_main(app),
            "quit" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    runtime::agent_shutdown().await;
                    app.exit(0);
                });
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

pub fn run() {
    let result = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_opener::init())
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
            tray(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
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
            runtime::runtime_status,
            runtime::runtime_install,
            runtime::runtime_server_exe,
            runtime::agent_ensure,
            mods::mods_index,
            mods::mod_install,
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
