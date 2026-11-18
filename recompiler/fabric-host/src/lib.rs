//! Native Pumpkin plugin around one Fabric mod that `TeaVM` compiled to C (recompiler/README.md).
//! The Java side (recompiler/fabric-shim) calls the `mmc_*` functions below through
//! `@Import`; we call its `@Export`s. Strings cross as UTF-8 (pointer, length). Player handle
//! 0 is "whoever ran the command"; handle n > 0 is the n-th player of the current call.
//!
//! `TeaVM`'s runtime is single-threaded, so every entry into Java holds [`JAVA`]. The `mmc_*`
//! callbacks run on the same thread inside that call and read its context from [`CALL`].

use std::{
    cell::RefCell,
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock, PoisonError, atomic::Ordering},
};

use moddedmc_api::{
    TextComponent, Vector3,
    pumpkin_core::{
        command::{
            CommandSource,
            argument_builder::{ArgumentBuilder, command},
            context::CommandContext,
            node::{CommandExecutor, CommandExecutorResult},
        },
        entity::{EntityBase, player::Player},
        plugin::{Context, Plugin, PluginFuture},
        server::Server,
    },
};

#[cfg(teavm)]
unsafe extern "C" {
    /// TeaVM's `main`, renamed at compile time: heap, class init, then `ModRuntime.main`.
    fn teavm_main(argc: i32, argv: *const *const u8) -> i32;
    fn mmc_run_command(player: i32, input: *const u8, len: i32) -> i32;
}

// Built without a TeaVM object (workspace builds, clippy): nothing to run.
#[cfg(not(teavm))]
const unsafe fn teavm_main(_: i32, _: *const *const u8) -> i32 {
    -1
}
#[cfg(not(teavm))]
const unsafe fn mmc_run_command(_: i32, _: *const u8, _: i32) -> i32 {
    0
}

/// Serializes every call into the Java runtime.
static JAVA: Mutex<()> = Mutex::new(());
static SERVER: OnceLock<Arc<Server>> = OnceLock::new();
/// Fabric's `config/`, inside the plugin's data folder.
static CONFIG_DIR: OnceLock<String> = OnceLock::new();

/// What the `mmc_*` callbacks may touch during one call into Java.
#[derive(Default)]
struct Call {
    context: Option<Arc<Context>>,
    /// The command's source; valid for the duration of the call only.
    source: Option<*const CommandSource>,
    players: Vec<Arc<Player>>,
}

thread_local! {
    static CALL: RefCell<Call> = RefCell::new(Call::default());
}

/// Runs `java` with `call` installed, holding the runtime lock.
fn enter<R>(call: Call, java: impl FnOnce() -> R) -> R {
    let _java = JAVA.lock().unwrap_or_else(PoisonError::into_inner);
    CALL.set(call);
    let result = java();
    CALL.set(Call::default());
    result
}

/// # Safety
/// `ptr` must point to `len` readable bytes; `TeaVM` passes a live byte array.
unsafe fn read(ptr: *const u8, len: i32) -> String {
    // SAFETY: the caller's contract.
    let bytes = unsafe { std::slice::from_raw_parts(ptr, usize::try_from(len).unwrap_or(0)) };
    String::from_utf8_lossy(bytes).into_owned()
}

fn player(handle: i32) -> Option<Arc<Player>> {
    let index = usize::try_from(handle).ok()?.checked_sub(1)?;
    CALL.with_borrow(|call| call.players.get(index).cloned())
}

#[unsafe(no_mangle)]
const extern "C" fn mmc_never() -> i32 {
    0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn mmc_register_command(name: *const u8, len: i32) {
    // SAFETY: Java passes a live UTF-8 byte array and its length.
    let name = unsafe { read(name, len) };
    let Some(context) = CALL.with_borrow(|call| call.context.clone()) else {
        tracing::warn!("fabric-host: command {name} registered after load; ignored");
        return;
    };
    // Fabric commands without .requires() are open to every player.
    // shortcut: root literal only, so arguments never reach Brigadier; add a greedy tail when a corpus mod needs one.
    let node = command(name.clone(), "").executes(Forward { input: name });
    context.register_command(node, "minecraft:command.help");
}

#[unsafe(no_mangle)]
unsafe extern "C" fn mmc_text(op: i32, handle: i32, buffer: *mut u8, capacity: i32) -> i32 {
    let text = match op {
        1 => player(handle).map(|p| p.gameprofile.name.clone()),
        2 => player(handle).map(|p| p.world().dimension.minecraft_name.to_owned()),
        3 => CONFIG_DIR.get().cloned(),
        4 => SERVER.get().and_then(|s| {
            let worlds = s.worlds.load();
            let world = worlds.get(usize::try_from(handle).ok()?)?;
            Some(world.dimension.minecraft_name.to_owned())
        }),
        _ => None,
    };
    let Some(text) = text else {
        return -1;
    };
    let n = text.len().min(usize::try_from(capacity).unwrap_or(0));
    // SAFETY: Java passes a buffer of `capacity` bytes; `n` never exceeds it.
    unsafe { std::ptr::copy_nonoverlapping(text.as_ptr(), buffer, n) };
    i32::try_from(text.len()).unwrap_or(i32::MAX)
}

#[unsafe(no_mangle)]
extern "C" fn mmc_player_num(handle: i32, field: i32) -> f64 {
    player(handle).map_or(0.0, |p| {
        let entity = &p.living_entity.entity;
        let pos = entity.pos.load();
        match field {
            0 => pos.x,
            1 => pos.y,
            2 => pos.z,
            3 => f64::from(entity.yaw.load()),
            4 => f64::from(entity.pitch.load()),
            _ => 0.0,
        }
    })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn mmc_send_message(handle: i32, text: *const u8, len: i32) {
    // SAFETY: Java passes a live UTF-8 byte array and its length.
    let text = TextComponent::text(unsafe { read(text, len) });
    if handle == 0 {
        if let Some(source) = CALL.with_borrow(|call| call.source) {
            // SAFETY: set by `Forward::execute` for exactly the duration of this call.
            unsafe { &*source }.send_message(text);
        }
    } else if let Some(p) = player(handle) {
        p.send_system_message(&text);
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
    // SAFETY: Java passes a live UTF-8 byte array and its length.
    let dimension = unsafe { read(dimension, len) };
    let world = SERVER.get().and_then(|s| {
        s.worlds
            .load()
            .iter()
            .find(|w| w.dimension.minecraft_name == dimension)
            .cloned()
    });
    let Some(world) = world else {
        tracing::warn!("fabric-host: cannot teleport to unknown world {dimension}");
        return;
    };
    if let Some(p) = player(handle) {
        p.teleport(Vector3::new(x, y, z), Some(yaw), Some(pitch), world);
    }
}

#[unsafe(no_mangle)]
extern "C" fn mmc_tick_count() -> i32 {
    SERVER
        .get()
        .map_or(0, |s| s.tick_count.load(Ordering::Relaxed))
}

#[unsafe(no_mangle)]
extern "C" fn mmc_world_count() -> i32 {
    SERVER
        .get()
        .map_or(0, |s| i32::try_from(s.worlds.load().len()).unwrap_or(0))
}

/// Hands one Pumpkin command to the mod's Brigadier dispatcher.
struct Forward {
    input: String,
}

impl CommandExecutor for Forward {
    fn execute(&self, context: &CommandContext) -> CommandExecutorResult {
        let player = context.source.as_player();
        let handle = i32::from(player.is_some());
        let call = Call {
            context: None,
            source: Some(std::ptr::from_ref(&context.source)),
            players: player.into_iter().collect(),
        };
        let len = i32::try_from(self.input.len()).unwrap_or(0);
        // SAFETY: `input` outlives the call; the runtime lock is held by `enter`.
        Ok(enter(call, || unsafe {
            mmc_run_command(handle, self.input.as_ptr(), len)
        }))
    }
}

struct FabricMod;

impl Plugin for FabricMod {
    fn on_load(&self, context: Arc<Context>) -> PluginFuture<'_, Result<(), String>> {
        Box::pin(async move {
            context.init_log();
            // A Fabric server always has config/, so mods may write into it without creating it.
            let config: PathBuf = context.get_data_folder().join("config");
            std::fs::create_dir_all(&config)
                .map_err(|e| format!("cannot create {}: {e}", config.display()))?;
            let _ = CONFIG_DIR.set(config.to_string_lossy().replace('\\', "/"));
            let _ = SERVER.set(context.server.clone());
            // Runs the mod's onInitialize and the command registration callbacks.
            let call = Call {
                context: Some(context.clone()),
                ..Call::default()
            };
            // SAFETY: called once, under the runtime lock; TeaVM's main ignores argv when argc is 0.
            let status = enter(call, || unsafe { teavm_main(0, std::ptr::null()) });
            if status == 0 {
                Ok(())
            } else {
                Err(format!("Java runtime exited with {status}"))
            }
        })
    }
}

moddedmc_api::export_plugin!(
    FabricMod,
    env!("MODDEDMC_MOD_ID"),
    "Fabric mod recompiled by TeaVM"
);
