//! A moddedMC mod (stable ABI) around one Fabric mod that `TeaVM` compiled to C
//! (recompiler/README.md). The Java side (recompiler/fabric-shim) calls the `mmc_*` functions
//! below through `@Import`; we call its `@Export`s. Strings cross as UTF-8 (pointer, length).
//! Player handle 0 is "whoever ran the command"; handle n > 0 is the n-th player of the call.
//!
//! `TeaVM`'s runtime is single-threaded, so every entry into Java holds [`JAVA`]. The `mmc_*`
//! callbacks run on the same thread inside that call and read its context from [`CALL`].

use std::{
    cell::RefCell,
    sync::{Mutex, OnceLock, PoisonError},
};

use moddedmc_api::{Command, Mod, Player, RegistrationError, Registry};

#[cfg(teavm)]
unsafe extern "C" {
    /// TeaVM's `main`, renamed at compile time: heap, class init, then `ModRuntime.main`.
    fn teavm_main(argc: i32, argv: *const *const u8) -> i32;
    fn mmc_run_command(player: i32, input: *const u8, len: i32) -> i32;
}

// Built without a TeaVM object (plain workspace builds): nothing to run.
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
/// Fabric's `config/`: the parent of this mod's own data folder.
static CONFIG_DIR: OnceLock<String> = OnceLock::new();

/// What the `mmc_*` callbacks may touch during one call into Java. Everything in it is valid
/// only while that call runs; [`enter`] clears it afterwards.
#[derive(Default)]
struct Call {
    registry: Option<*mut Registry<'static>>,
    command: Option<*const Command<'static>>,
    players: Vec<Player<'static>>,
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

fn player(handle: i32) -> Option<Player<'static>> {
    let index = usize::try_from(handle).ok()?.checked_sub(1)?;
    CALL.with_borrow(|call| call.players.get(index).copied())
}

#[unsafe(no_mangle)]
const extern "C" fn mmc_never() -> i32 {
    0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn mmc_register_command(name: *const u8, len: i32) {
    // SAFETY: Java passes a live UTF-8 byte array and its length.
    let name = unsafe { read(name, len) };
    let Some(registry) = CALL.with_borrow(|call| call.registry) else {
        moddedmc_api::warn(&format!(
            "fabric-host: command {name} registered after load; ignored"
        ));
        return;
    };
    // shortcut: root literal only, so arguments never reach Brigadier; add a greedy tail when a corpus mod needs one.
    // SAFETY: set by `register` for exactly the duration of this call.
    if let Err(e) = unsafe { &mut *registry }.add_command(&name, "", 0) {
        moddedmc_api::warn(&format!("fabric-host: command {name}: {e:?}"));
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn mmc_text(op: i32, handle: i32, buffer: *mut u8, capacity: i32) -> i32 {
    let text = match op {
        1 => player(handle).map(|p| p.name().to_owned()),
        2 => player(handle).map(|p| p.dimension().to_owned()),
        3 => CONFIG_DIR.get().cloned(),
        4 => usize::try_from(handle)
            .ok()
            .and_then(|i| moddedmc_api::dimensions().nth(i))
            .map(str::to_owned),
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
        let pos = p.position();
        match field {
            0 => pos.x,
            1 => pos.y,
            2 => pos.z,
            3 => f64::from(pos.yaw),
            4 => f64::from(pos.pitch),
            _ => 0.0,
        }
    })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn mmc_send_message(handle: i32, text: *const u8, len: i32) {
    // SAFETY: Java passes a live UTF-8 byte array and its length.
    let text = unsafe { read(text, len) };
    if handle == 0 {
        if let Some(command) = CALL.with_borrow(|call| call.command) {
            // SAFETY: set by `command` for exactly the duration of this call.
            unsafe { &*command }.reply(&text);
        }
    } else if let Some(p) = player(handle) {
        p.send_message(&text);
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
    if let Some(p) = player(handle)
        && !p.teleport(&dimension, x, y, z, Some((yaw, pitch)))
    {
        moddedmc_api::warn(&format!(
            "fabric-host: cannot teleport to unknown world {dimension}"
        ));
    }
}

#[unsafe(no_mangle)]
extern "C" fn mmc_tick_count() -> i32 {
    // Wraps like Java's int tick counter.
    #[expect(clippy::cast_possible_truncation, reason = "wraps like Java's int")]
    let ticks = moddedmc_api::tick_count() as i32;
    ticks
}

#[unsafe(no_mangle)]
extern "C" fn mmc_world_count() -> i32 {
    i32::try_from(moddedmc_api::dimensions().count()).unwrap_or(0)
}

struct FabricMod;

impl Mod for FabricMod {
    fn register(&self, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
        // A Fabric server always has config/, so mods may write into it without creating it.
        let data = moddedmc_api::data_dir();
        let config = data.parent().unwrap_or(data);
        // Absolute: TeaVM's Files.createDirectories throws on an existing one-level relative path.
        let config = std::path::absolute(config).unwrap_or_else(|_| config.to_path_buf());
        let _ = CONFIG_DIR.set(config.to_string_lossy().replace('\\', "/"));
        // Runs the mod's onInitialize and its command registration callbacks.
        let call = Call {
            registry: Some(std::ptr::from_mut(registry).cast()),
            ..Call::default()
        };
        // SAFETY: called once, under the runtime lock; TeaVM's main ignores argv when argc is 0.
        let status = enter(call, || unsafe { teavm_main(0, std::ptr::null()) });
        if status == 0 {
            Ok(())
        } else {
            moddedmc_api::error(&format!("fabric-host: Java runtime exited with {status}"));
            Err(RegistrationError::InvalidDefinition)
        }
    }

    fn command(&self, _tag: u64, command: Command<'_>) -> i32 {
        let handle = i32::from(command.player.is_some());
        let call = Call {
            registry: None,
            command: Some(std::ptr::from_ref(&command).cast()),
            // SAFETY: lifetime erasure only; `enter` clears `CALL` before `command` is dropped.
            players: command
                .player
                .map(|p| unsafe { std::mem::transmute::<Player<'_>, Player<'static>>(p) })
                .into_iter()
                .collect(),
        };
        let input = command.input;
        let len = i32::try_from(input.len()).unwrap_or(0);
        // SAFETY: `input` outlives the call; the runtime lock is held by `enter`.
        enter(call, || unsafe {
            mmc_run_command(handle, input.as_ptr(), len)
        })
    }
}

moddedmc_api::export_mod!(FabricMod, id = env!("MODDEDMC_MOD_ID"));
