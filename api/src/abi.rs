//! The C ABI between the server and a mod library (see docs/MOD-ABI.md).
//!
//! Everything here is `#[repr(C)]` and calls are `extern "C"`, so a mod and the server need not
//! come from the same compiler or build. Nothing allocated on one side is freed on the other:
//! data is borrowed for the length of a call, and bytes going back to the server are copied
//! through a [`Sink`].
//!
//! Compatibility: [`ABI_VERSION`] changes only for breaking changes. New host functions are
//! appended to [`HostApi`]; `HostApi::size` tells a mod which ones the running server has.

use std::ffi::c_void;

/// Major version. The server refuses a library whose `MODDEDMC_ABI` differs.
pub const ABI_VERSION: u32 = 1;
/// `static MODDEDMC_ABI: u32` exported by every mod library.
pub const ABI_SYMBOL: &[u8] = b"MODDEDMC_ABI\0";
/// `static MODDEDMC_ID: Str`, the mod's id: namespace of everything it adds and name of its
/// data folder. Read before the mod is started.
pub const ID_SYMBOL: &[u8] = b"MODDEDMC_ID\0";
/// `extern "C" fn moddedmc_mod(host: *const HostApi) -> *const ModApi`, null on failure.
pub const ENTRY_SYMBOL: &[u8] = b"moddedmc_mod\0";

/// Borrowed bytes (UTF-8 where a string is meant). Valid for the call it is passed in, unless
/// documented otherwise.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Str {
    pub ptr: *const u8,
    pub len: usize,
}

impl Str {
    pub const EMPTY: Self = Self::new(&[]);

    pub const fn new(bytes: &[u8]) -> Self {
        Self {
            ptr: bytes.as_ptr(),
            len: bytes.len(),
        }
    }

    /// # Safety
    /// `ptr` must point to `len` readable bytes that outlive `'a`.
    pub const unsafe fn bytes<'a>(self) -> &'a [u8] {
        if self.len == 0 {
            &[]
        } else {
            // SAFETY: the caller's contract.
            unsafe { std::slice::from_raw_parts(self.ptr, self.len) }
        }
    }

    /// Invalid UTF-8 reads as an empty string.
    ///
    /// # Safety
    /// As for [`Str::bytes`].
    pub unsafe fn str<'a>(self) -> &'a str {
        // SAFETY: the caller's contract.
        std::str::from_utf8(unsafe { self.bytes() }).unwrap_or_default()
    }
}

// SAFETY: a Str is a read-only borrow; whoever hands one out keeps the bytes alive and unchanged.
unsafe impl Send for Str {}
// SAFETY: as above.
unsafe impl Sync for Str {}

impl From<&str> for Str {
    fn from(s: &str) -> Self {
        Self::new(s.as_bytes())
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct Pos {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

/// An online player, valid only during the call it was passed in. Null means "no player".
#[repr(transparent)]
#[derive(Clone, Copy, Debug)]
pub struct PlayerRef(pub *const c_void);

/// Bytes handed back to the server, which copies them before `put` returns.
#[repr(C)]
pub struct Sink {
    pub ctx: *mut c_void,
    pub put: unsafe extern "C" fn(ctx: *mut c_void, key: u64, value: Str),
}

#[repr(C)]
pub struct MenuButton {
    pub label: Str,
    pub tooltip: Str,
    /// Sent back to every mod's `menu_click` when the player picks this button.
    pub action: Str,
}

#[repr(C)]
pub struct TickEntry {
    pub entity_type: u32,
    pub pos: Pos,
    pub state: Str,
}

#[repr(C)]
pub struct BlockCall {
    pub entity_type: u32,
    pub pos: Pos,
    pub dimension: Str,
    pub state: Str,
    pub player: PlayerRef,
}

#[repr(C)]
pub struct CommandCall {
    /// Pass to `HostApi::reply`.
    pub source: *const c_void,
    /// Null when the console ran the command.
    pub player: PlayerRef,
    /// The whole command line, without the leading `/`.
    pub input: Str,
}

/// Registration handle, valid only during `ModApi::register`.
pub type Reg = *mut c_void;

/// Registration results: 0 is success.
pub mod code {
    pub const OK: i32 = 0;
    pub const INVALID_DEFINITION: i32 = 1;
    pub const DUPLICATE_ID: i32 = 2;
    pub const UNKNOWN_ENTITY_TYPE: i32 = 3;
    pub const UNSUPPORTED_STAND_IN: i32 = 4;
    /// The mod panicked.
    pub const PANICKED: i32 = 255;
}

/// Log levels for `HostApi::log`.
pub mod level {
    pub const ERROR: u32 = 0;
    pub const WARN: u32 = 1;
    pub const INFO: u32 = 2;
}

/// The server, as seen by a mod. Lives for the whole process. Every function may be called from
/// any thread.
#[repr(C)]
pub struct HostApi {
    pub abi_version: u32,
    /// `size_of::<HostApi>()` on the server; fields past a mod's own size do not exist there.
    pub size: usize,
    pub log: unsafe extern "C" fn(level: u32, message: Str),
    /// `<server>/config/<mod id>`, already created. Static.
    pub data_dir: Str,
    /// `tick_interval` 0 means event-driven only. Writes the entity type index to `out_index`.
    pub add_block_entity: unsafe extern "C" fn(
        reg: Reg,
        id: Str,
        state_version: u32,
        initial_state: Str,
        tick_interval: u32,
        out_index: *mut u32,
    ) -> i32,
    /// `block_entity` is the id of a block entity type this mod registered.
    pub add_block: unsafe extern "C" fn(
        reg: Reg,
        id: Str,
        display_name: Str,
        stand_in: Str,
        block_entity: Str,
    ) -> i32,
    /// A root command open to every player. `ModApi::command` gets `tag` and the whole input.
    pub add_command: unsafe extern "C" fn(reg: Reg, name: Str, description: Str, tag: u64) -> i32,
    /// Valid as long as the player reference.
    pub player_name: unsafe extern "C" fn(player: PlayerRef) -> Str,
    /// e.g. `minecraft:overworld`. Static.
    pub player_dimension: unsafe extern "C" fn(player: PlayerRef) -> Str,
    /// x, y, z, yaw, pitch.
    pub player_position: unsafe extern "C" fn(player: PlayerRef, out: *mut [f64; 5]),
    pub send_message: unsafe extern "C" fn(player: PlayerRef, text: Str, action_bar: bool),
    /// Answers a command's source, player or console.
    pub reply: unsafe extern "C" fn(source: *const c_void, text: Str),
    /// Puts the item that places `block_id` in the player's hand. False if no such block.
    pub give_block: unsafe extern "C" fn(player: PlayerRef, block_id: Str) -> bool,
    /// True if the move was accepted (a move to another dimension finishes shortly after).
    /// NaN yaw or pitch keeps the current one. False if no such dimension.
    pub teleport: unsafe extern "C" fn(
        player: PlayerRef,
        dimension: Str,
        x: f64,
        y: f64,
        z: f64,
        yaw: f32,
        pitch: f32,
    ) -> bool,
    pub show_menu: unsafe extern "C" fn(
        player: PlayerRef,
        title: Str,
        body: Str,
        buttons: *const MenuButton,
        count: usize,
    ),
    pub tick_count: unsafe extern "C" fn() -> u64,
    /// The n-th loaded dimension, or an empty string past the end. Static.
    pub dimension: unsafe extern "C" fn(index: usize) -> Str,
    /// Finds a process-lifetime interface table by exact name and version, or returns null.
    pub get_interface: unsafe extern "C" fn(name: Str, version: u32) -> *const c_void,
}

pub const WORLD: &str = "moddedmc:world";
pub const CHUNKS: &str = "moddedmc:chunks";
pub const COMMANDS: &str = "moddedmc:commands";
pub const LIFECYCLE: &str = "moddedmc:lifecycle";

#[repr(C)]
pub struct WorldV1 {
    pub size: usize,
    /// The n-th loaded dimension, or empty past the end. Static.
    pub dimension: unsafe extern "C" fn(index: usize) -> Str,
    /// The command source's dimension, or empty if it has none. Static.
    pub command_dimension: unsafe extern "C" fn(source: *const c_void) -> Str,
}

/// Called exactly once for every accepted request. May run before the submit call returns.
pub type ChunkCompletion = unsafe extern "C" fn(ctx: *mut c_void, generated: bool);
pub type ChunkRequest = unsafe extern "C" fn(
    dimension: Str,
    x: i32,
    z: i32,
    ctx: *mut c_void,
    completed: ChunkCompletion,
) -> bool;

#[repr(C)]
pub struct ChunksV1 {
    pub size: usize,
    /// Submit an async check for a loaded or saved chunk. False means rejected; no callback.
    pub is_generated: ChunkRequest,
    /// Check first, then generate/load if missing. Reports true once generated.
    pub ensure_generated: ChunkRequest,
}

pub const COMMAND_ROOT: u32 = u32::MAX;
pub const COMMAND_LITERAL: u32 = 0;
pub const COMMAND_INTEGER: u32 = 1;

/// A child in a command tree. Parents come before children.
#[repr(C)]
pub struct CommandNode {
    pub name: Str,
    /// Index in the same array, or COMMAND_ROOT for a direct child of the root.
    pub parent: u32,
    pub kind: u32,
    /// Inclusive bounds for an integer argument.
    pub min: i32,
    pub max: i32,
    /// Executable nodes call ModApi::command with tag.
    pub executable: bool,
    pub tag: u64,
}

#[repr(C)]
pub struct CommandsV1 {
    pub size: usize,
    /// Register a root literal with typed children and a default operator level (0 through 4).
    pub add: unsafe extern "C" fn(
        reg: Reg,
        name: Str,
        description: Str,
        op_level: u8,
        nodes: *const CommandNode,
        count: usize,
    ) -> i32,
    /// Read a parsed integer argument. False if it isn't present.
    pub integer: unsafe extern "C" fn(source: *const c_void, name: Str, out: *mut i32) -> bool,
    /// Send feedback, respecting the command source's feedback settings.
    pub feedback: unsafe extern "C" fn(source: *const c_void, message: Str),
}

pub type StopCallback = unsafe extern "C" fn();

#[repr(C)]
pub struct LifecycleV1 {
    pub size: usize,
    /// Register one callback, run on a blocking thread at shutdown before worlds save.
    pub on_stopping: unsafe extern "C" fn(reg: Reg, callback: StopCallback) -> i32,
}

/// A mod, as seen by the server. Returned by `moddedmc_mod`; lives for the whole process. Calls
/// may come from several threads at once.
#[repr(C)]
pub struct ModApi {
    pub abi_version: u32,
    pub size: usize,
    /// Static.
    pub version: Str,
    pub instance: *const c_void,
    /// Once, on the server's first tick. Returns a [`code`].
    pub register: unsafe extern "C" fn(instance: *const c_void, reg: Reg) -> i32,
    /// Due block entities of one dimension. Sink key = entry index, value = new state.
    pub tick: unsafe extern "C" fn(
        instance: *const c_void,
        tick: u64,
        dimension: Str,
        entries: *const TickEntry,
        count: usize,
        out: Sink,
    ),
    /// Sink key 0 = new state, key 1 = action-bar message.
    pub use_block: unsafe extern "C" fn(instance: *const c_void, call: *const BlockCall, out: Sink),
    /// Sink key 0 = replacement for the initial state.
    pub place_block:
        unsafe extern "C" fn(instance: *const c_void, call: *const BlockCall, out: Sink),
    pub break_block: unsafe extern "C" fn(instance: *const c_void, call: *const BlockCall),
    /// Returns the command's result (0 = nothing happened).
    pub command:
        unsafe extern "C" fn(instance: *const c_void, tag: u64, call: *const CommandCall) -> i32,
    /// Every mod sees every menu click; act only on your own action ids.
    pub menu_click: unsafe extern "C" fn(instance: *const c_void, player: PlayerRef, action: Str),
}

// SAFETY: a ModApi is immutable after creation and its instance is `Send + Sync` (see `Mod`).
unsafe impl Send for ModApi {}
// SAFETY: as above.
unsafe impl Sync for ModApi {}
// SAFETY: a HostApi is immutable and its functions are thread-safe.
unsafe impl Send for HostApi {}
// SAFETY: as above.
unsafe impl Sync for HostApi {}

/// The published layout. A failure here means an existing field moved, which breaks every mod
/// built so far: append new fields instead (CONTRIBUTING.md).
#[cfg(all(test, target_pointer_width = "64"))]
mod layout {
    use std::mem::offset_of;

    use super::*;

    #[test]
    fn host_api_v1() {
        let fields = [
            (offset_of!(HostApi, abi_version), 0),
            (offset_of!(HostApi, size), 8),
            (offset_of!(HostApi, log), 16),
            (offset_of!(HostApi, data_dir), 24),
            (offset_of!(HostApi, add_block_entity), 40),
            (offset_of!(HostApi, add_block), 48),
            (offset_of!(HostApi, add_command), 56),
            (offset_of!(HostApi, player_name), 64),
            (offset_of!(HostApi, player_dimension), 72),
            (offset_of!(HostApi, player_position), 80),
            (offset_of!(HostApi, send_message), 88),
            (offset_of!(HostApi, reply), 96),
            (offset_of!(HostApi, give_block), 104),
            (offset_of!(HostApi, teleport), 112),
            (offset_of!(HostApi, show_menu), 120),
            (offset_of!(HostApi, tick_count), 128),
            (offset_of!(HostApi, dimension), 136),
            (offset_of!(HostApi, get_interface), 144),
        ];
        for (i, (actual, expected)) in fields.into_iter().enumerate() {
            assert_eq!(actual, expected, "HostApi field #{i} moved");
        }
        assert!(size_of::<HostApi>() >= 152);
    }

    #[test]
    fn mod_api_v1() {
        let fields = [
            (offset_of!(ModApi, abi_version), 0),
            (offset_of!(ModApi, size), 8),
            (offset_of!(ModApi, version), 16),
            (offset_of!(ModApi, instance), 32),
            (offset_of!(ModApi, register), 40),
            (offset_of!(ModApi, tick), 48),
            (offset_of!(ModApi, use_block), 56),
            (offset_of!(ModApi, place_block), 64),
            (offset_of!(ModApi, break_block), 72),
            (offset_of!(ModApi, command), 80),
            (offset_of!(ModApi, menu_click), 88),
        ];
        for (i, (actual, expected)) in fields.into_iter().enumerate() {
            assert_eq!(actual, expected, "ModApi field #{i} moved");
        }
        assert!(size_of::<ModApi>() >= 96);
    }

    #[test]
    fn interfaces_v1() {
        assert_eq!(offset_of!(WorldV1, size), 0);
        assert_eq!(offset_of!(WorldV1, dimension), 8);
        assert_eq!(offset_of!(WorldV1, command_dimension), 16);
        assert!(size_of::<WorldV1>() >= 24);

        assert_eq!(offset_of!(ChunksV1, size), 0);
        assert_eq!(offset_of!(ChunksV1, is_generated), 8);
        assert_eq!(offset_of!(ChunksV1, ensure_generated), 16);
        assert!(size_of::<ChunksV1>() >= 24);

        assert_eq!(offset_of!(CommandsV1, size), 0);
        assert_eq!(offset_of!(CommandsV1, add), 8);
        assert_eq!(offset_of!(CommandsV1, integer), 16);
        assert_eq!(offset_of!(CommandsV1, feedback), 24);
        assert!(size_of::<CommandsV1>() >= 32);

        assert_eq!(offset_of!(LifecycleV1, size), 0);
        assert_eq!(offset_of!(LifecycleV1, on_stopping), 8);
        assert!(size_of::<LifecycleV1>() >= 16);

        assert_eq!(offset_of!(CommandNode, name), 0);
        assert_eq!(offset_of!(CommandNode, parent), 16);
        assert_eq!(offset_of!(CommandNode, kind), 20);
        assert_eq!(offset_of!(CommandNode, min), 24);
        assert_eq!(offset_of!(CommandNode, max), 28);
        assert_eq!(offset_of!(CommandNode, executable), 32);
        assert_eq!(offset_of!(CommandNode, tag), 40);
        assert_eq!(size_of::<CommandNode>(), 48);
    }

    #[test]
    fn payloads_v1() {
        assert_eq!((size_of::<Str>(), size_of::<Pos>()), (16, 12));
        assert_eq!(size_of::<TickEntry>(), 32);
        assert_eq!(size_of::<BlockCall>(), 56);
        assert_eq!(size_of::<CommandCall>(), 32);
        assert_eq!(size_of::<MenuButton>(), 48);
    }
}
