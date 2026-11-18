//! Pumpkin plugin around one Fabric mod that TeaVM compiled to C (recompiler/README.md).
//! The Java side (recompiler/fabric-shim) calls the `mmc_*` functions below through
//! `@Import`; we call its `@Export`s. Strings cross as UTF-8 (pointer, length) in the one
//! linear memory both sides share. Player handle 0 is "whoever ran the command"; handle
//! n > 0 is `PLAYERS[n - 1]`, valid only during that command.

use std::cell::RefCell;

use pumpkin_plugin_api::{
    Context, Plugin, PluginMetadata, Server,
    command::{Command, CommandError, CommandSender, ConsumedArgs},
    commands::CommandHandler,
    permissions::{FS_READ_DATA, FS_WRITE_DATA},
    player::Player,
    register_plugin,
    text::TextComponent,
};

unsafe extern "C" {
    /// TeaVM's `main`, renamed at compile time: heap, class init, then `ModRuntime.main`.
    fn teavm_main(argc: i32, argv: *const *const u8) -> i32;
    fn mmc_run_command(player: i32, input: *const u8, len: i32) -> i32;
}

/// Fabric's `config/`, inside the plugin data folder (WASI preopen "data"). Absolute and two
/// levels deep because TeaVM's `Files.createDirectories` stats the parent of a one-level path,
/// and neither "/" nor "." is visible under WASI.
const CONFIG_DIR: &str = "/data/config";

thread_local! {
    static LOADING: RefCell<Option<Context>> = const { RefCell::new(None) };
    static SERVER: RefCell<Option<Server>> = const { RefCell::new(None) };
    static SENDER: RefCell<Option<CommandSender>> = const { RefCell::new(None) };
    static PLAYERS: RefCell<Vec<Player>> = const { RefCell::new(Vec::new()) };
}

/// # Safety
/// `ptr` must point to `len` readable bytes; TeaVM passes a live byte array.
unsafe fn read(ptr: *const u8, len: i32) -> String {
    let bytes = unsafe { std::slice::from_raw_parts(ptr, usize::try_from(len).unwrap_or(0)) };
    String::from_utf8_lossy(bytes).into_owned()
}

fn with_player<R>(handle: i32, f: impl FnOnce(&Player) -> R) -> Option<R> {
    let index = usize::try_from(handle).ok()?.checked_sub(1)?;
    PLAYERS.with_borrow(|players| players.get(index).map(f))
}

fn with_server<R>(f: impl FnOnce(&Server) -> Option<R>) -> Option<R> {
    SERVER.with_borrow(|server| server.as_ref().and_then(f))
}

#[unsafe(no_mangle)]
extern "C" fn mmc_never() -> i32 {
    0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn mmc_register_command(name: *const u8, len: i32) {
    let name = unsafe { read(name, len) };
    LOADING.with_borrow(|context| {
        let Some(context) = context else {
            tracing::warn!("fabric-host: command {name} registered after load; ignored");
            return;
        };
        // Fabric commands without .requires() are open to every player.
        // shortcut: root literal only, so arguments never reach Brigadier; add a greedy tail when a corpus mod needs one.
        let command = Command::new(std::slice::from_ref(&name), "").execute(Forward { input: name });
        context.register_command(command, "minecraft:command.help");
    });
}

#[unsafe(no_mangle)]
unsafe extern "C" fn mmc_text(op: i32, handle: i32, buffer: *mut u8, capacity: i32) -> i32 {
    let text = match op {
        1 => with_player(handle, Player::get_name),
        2 => with_player(handle, |p| p.get_world().get_dimension()),
        3 => Some(CONFIG_DIR.to_owned()),
        4 => with_server(|s| {
            let worlds = s.get_all_worlds();
            worlds.get(usize::try_from(handle).ok()?).map(pumpkin_plugin_api::World::get_dimension)
        }),
        _ => None,
    };
    let Some(text) = text else {
        return -1;
    };
    let capacity = usize::try_from(capacity).unwrap_or(0);
    let n = text.len().min(capacity);
    unsafe { std::ptr::copy_nonoverlapping(text.as_ptr(), buffer, n) };
    i32::try_from(text.len()).unwrap_or(i32::MAX)
}

#[unsafe(no_mangle)]
extern "C" fn mmc_player_num(handle: i32, field: i32) -> f64 {
    with_player(handle, |p| {
        let (x, y, z) = p.get_position();
        match field {
            0 => x,
            1 => y,
            2 => z,
            3 => f64::from(p.get_yaw()),
            4 => f64::from(p.get_pitch()),
            _ => 0.0,
        }
    })
    .unwrap_or(0.0)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn mmc_send_message(handle: i32, text: *const u8, len: i32) {
    let text = TextComponent::text(&unsafe { read(text, len) });
    if handle == 0 {
        SENDER.with_borrow(|sender| sender.as_ref().map(|s| s.send_message(text)));
    } else {
        with_player(handle, |p| p.send_system_message(text, false));
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn mmc_teleport(
    handle: i32,
    dimension: *const u8,
    len: i32,
    x: f64,
    y: f64,
    z: f64,
    yaw: f32,
    pitch: f32,
) {
    let dimension = unsafe { read(dimension, len) };
    let Some(world) = with_server(|s| s.get_world_by_name(&dimension)) else {
        tracing::warn!("fabric-host: cannot teleport to unknown world {dimension}");
        return;
    };
    with_player(handle, |p| p.teleport((x, y, z), Some(yaw), Some(pitch), world));
}

#[unsafe(no_mangle)]
extern "C" fn mmc_tick_count() -> i32 {
    // shortcut: overworld age instead of ticks since boot; differences between readings are
    // the same, which is what cooldown-style mods use. Count ticks ourselves if a mod needs boot-relative values.
    #[expect(clippy::cast_possible_truncation, reason = "vanilla's tick counter is an int too")]
    with_server(|s| s.get_world_by_name("minecraft:overworld")).map_or(0, |w| w.get_world_age() as i32)
}

#[unsafe(no_mangle)]
extern "C" fn mmc_world_count() -> i32 {
    with_server(|s| i32::try_from(s.get_all_worlds().len()).ok()).unwrap_or(0)
}

/// Hands one Pumpkin command to the mod's Brigadier dispatcher.
struct Forward {
    input: String,
}

impl CommandHandler for Forward {
    fn handle(&self, sender: CommandSender, _: Server, _: ConsumedArgs) -> Result<i32, CommandError> {
        let handle = match sender.as_player() {
            Some(p) => {
                PLAYERS.set(vec![p]);
                1
            }
            None => 0,
        };
        SENDER.set(Some(sender));
        let len = i32::try_from(self.input.len()).unwrap_or(0);
        let result = unsafe { mmc_run_command(handle, self.input.as_ptr(), len) };
        SENDER.set(None);
        PLAYERS.set(Vec::new());
        Ok(result)
    }
}

struct FabricMod;

impl Plugin for FabricMod {
    fn new() -> Self {
        FabricMod
    }

    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            name: env!("MODDEDMC_MOD_ID").into(),
            version: env!("CARGO_PKG_VERSION").into(),
            authors: vec!["moddedMC-rust recompiler".into()],
            description: "Fabric mod recompiled by TeaVM".into(),
            dependencies: vec![],
            permissions: vec![FS_READ_DATA.into(), FS_WRITE_DATA.into()],
        }
    }

    fn on_load(&self, context: Context) -> Result<(), String> {
        // A Fabric server always has config/, so mods may write into it without creating it.
        std::fs::create_dir_all(CONFIG_DIR).map_err(|e| format!("cannot create {CONFIG_DIR}: {e}"))?;
        SERVER.set(Some(context.get_server()));
        LOADING.set(Some(context));
        // Runs the mod's onInitialize and the command registration callbacks.
        let status = unsafe { teavm_main(0, std::ptr::null()) };
        LOADING.set(None);
        if status == 0 { Ok(()) } else { Err(format!("Java runtime exited with {status}")) }
    }
}

register_plugin!(FabricMod);
