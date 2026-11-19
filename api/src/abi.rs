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
/// `extern "C" fn moddedmc_mod(host: &'static HostApi) -> *const ModApi`, null on failure.
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
    // ---- 1.1 ----
    /// A named, versioned interface table such as [`WORLD_V1`], or null if this server does not
    /// offer it. Tables live for the whole process and start with their own `size`. Check
    /// `HostApi::size` before calling: servers older than 1.1 do not have this field.
    pub query_interface: unsafe extern "C" fn(name: Str) -> *const c_void,
}

/// `HostApi` size of ABI 1.0, before `query_interface`.
pub const HOST_API_V1_0_SIZE: usize = std::mem::offset_of!(HostApi, query_interface);

/// [`WorldV1`]: batched block reads and writes.
pub const WORLD_V1: &str = "moddedmc:world/1";

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct BlockWrite {
    pub pos: Pos,
    pub state: u32,
}

/// Block states are vanilla state ids: fast to compare and pass around, valid for this Minecraft
/// version only. Save names (`state_name`), never ids.
#[repr(C)]
pub struct WorldV1 {
    pub size: usize,
    /// State ids of `count` positions in one dimension, written to `out`. Positions in unloaded
    /// chunks or outside the world read [`NO_STATE`]. False if there is no such dimension.
    pub get_blocks: unsafe extern "C" fn(
        dimension: Str,
        positions: *const Pos,
        count: usize,
        out: *mut u32,
    ) -> bool,
    /// Writes blocks in order, with neighbor and client updates like a player placing them.
    /// Unloaded or out-of-world positions and unknown states are skipped. Returns how many were
    /// written.
    pub set_blocks:
        unsafe extern "C" fn(dimension: Str, writes: *const BlockWrite, count: usize) -> usize,
    /// `minecraft:oak_log[axis=x]` to a state id; properties left out take the block's default.
    /// [`NO_STATE`] if the block or a property is unknown.
    pub state_id: unsafe extern "C" fn(name: Str) -> u32,
    /// A state id to its full name (`minecraft:oak_log[axis=x]`), through `out` with key 0.
    /// False if unknown.
    pub state_name: unsafe extern "C" fn(state: u32, out: Sink) -> bool,
}

/// "No block state": unloaded, outside the world, or unknown.
pub const NO_STATE: u32 = u32::MAX;

/// [`BlocksV1`]: real modded blocks.
pub const BLOCKS_V1: &str = "moddedmc:blocks/1";

/// Handle for defining blocks, valid only during [`ModApi::define_blocks`].
pub type Defs = *mut c_void;

/// A block state property and its values, e.g. `axis` = `x`, `y`, `z`.
#[repr(C)]
pub struct Property {
    pub name: Str,
    pub values: *const Str,
    pub count: usize,
}

/// Modded blocks get their own state ids after vanilla's, are saved by name with the chunk and
/// work with `moddedmc:world/1` like vanilla blocks. A stock client sees each state as a vanilla
/// look-alike (its appearance), whose shape, hardness and light the block also gets.
#[repr(C)]
pub struct BlocksV1 {
    pub size: usize,
    /// Defines a block. `name` is `namespace:path`, any namespace but `minecraft`. Its states are
    /// every combination of the property values, the last property changing fastest; the first
    /// is the default. `appearances` names one vanilla state per state, in that order
    /// (`minecraft:end_rod[facing=east]`). Returns a [`code`].
    pub define_block: unsafe extern "C" fn(
        defs: Defs,
        name: Str,
        properties: *const Property,
        property_count: usize,
        appearances: *const Str,
        appearance_count: usize,
    ) -> i32,
    /// Sends the block at `pos` a [`block_event::TICK`] `delay` ticks from now (at least 1), if
    /// it is still one of this mod's blocks then. Scheduled ticks are not saved: after a restart,
    /// schedule again. False if there is no such dimension.
    pub schedule_tick: unsafe extern "C" fn(dimension: Str, pos: Pos, delay: u32) -> bool,
    /// Whether the block at `pos` receives redstone power: 1 yes, 0 no, -1 not loaded.
    pub is_powered: unsafe extern "C" fn(dimension: Str, pos: Pos) -> i32,
}

/// [`EntitiesV1`]: creatures and other entities a mod drives.
pub const ENTITIES_V1: &str = "moddedmc:entities/1";

/// An entity's UUID. It stays the same across restarts, so mods may save it.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct EntityId(pub [u8; 16]);

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

/// Entities a mod drives. A stock client knows only vanilla entity types, so a modded creature
/// is a vanilla entity (its look-alike) that the mod moves, names and removes; Pumpkin saves it
/// with the world. Calls on an entity that is gone or in an unloaded chunk return false.
#[repr(C)]
pub struct EntitiesV1 {
    pub size: usize,
    /// Spawns a vanilla entity type (`minecraft:armor_stand`) and writes its id to `out`. False
    /// if the type or dimension is unknown.
    pub spawn:
        unsafe extern "C" fn(dimension: Str, kind: Str, at: Vec3, out: *mut EntityId) -> bool,
    /// Writes its position to `out`.
    pub position: unsafe extern "C" fn(dimension: Str, id: EntityId, out: *mut Vec3) -> bool,
    /// Moves it, turning it to `yaw` and `pitch` (degrees; NaN keeps the current one).
    pub move_to:
        unsafe extern "C" fn(dimension: Str, id: EntityId, at: Vec3, yaw: f32, pitch: f32) -> bool,
    /// Shows `name` above it; an empty name hides it.
    pub set_name: unsafe extern "C" fn(dimension: Str, id: EntityId, name: Str) -> bool,
    pub remove: unsafe extern "C" fn(dimension: Str, id: EntityId) -> bool,
}

/// What happened to a `blocks/1` block, in [`BlockEvent::kind`].
pub mod block_event {
    /// A player used (right-clicked) it.
    pub const USE: u32 = 0;
    /// A player placed it from an item ([`HostApi::give_block`]).
    pub const PLACE: u32 = 1;
    /// It is being broken; `state` is its last state.
    pub const BREAK: u32 = 2;
    /// A tick it asked for with [`super::BlocksV1::schedule_tick`].
    pub const TICK: u32 = 3;
    /// A neighboring block changed (also redstone power). Delivered at the start of the next
    /// server tick, once per block and tick.
    pub const NEIGHBOR: u32 = 4;
    /// Its chunk loaded (also the first time after a start). Delivered at the start of the next
    /// server tick; there is no player.
    pub const LOAD: u32 = 5;
}

#[repr(C)]
pub struct BlockEvent {
    pub kind: u32,
    pub dimension: Str,
    pub pos: Pos,
    pub state: u32,
    /// Null when no player caused it.
    pub player: PlayerRef,
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
    // ---- 1.1 ----
    /// Once, before any world loads: define `blocks/1` blocks with `defs`. Returns a [`code`];
    /// on failure none of this mod's blocks exist.
    pub define_blocks: unsafe extern "C" fn(instance: *const c_void, defs: Defs) -> i32,
    /// Something happened to one of this mod's `blocks/1` blocks. For [`block_event::USE`],
    /// true means handled and skips the appearance's own interaction.
    pub block_event:
        unsafe extern "C" fn(instance: *const c_void, event: *const BlockEvent) -> bool,
}

/// `ModApi` size of ABI 1.0, before `define_blocks`.
pub const MOD_API_V1_0_SIZE: usize = std::mem::offset_of!(ModApi, define_blocks);

// SAFETY: interface tables are immutable and their functions are thread-safe.
unsafe impl Sync for WorldV1 {}
// SAFETY: as above.
unsafe impl Sync for BlocksV1 {}
// SAFETY: as above.
unsafe impl Sync for EntitiesV1 {}

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
            (offset_of!(HostApi, query_interface), 144),
        ];
        for (i, (actual, expected)) in fields.into_iter().enumerate() {
            assert_eq!(actual, expected, "HostApi field #{i} moved");
        }
        assert_eq!(HOST_API_V1_0_SIZE, 144);
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
            (offset_of!(ModApi, define_blocks), 96),
            (offset_of!(ModApi, block_event), 104),
        ];
        for (i, (actual, expected)) in fields.into_iter().enumerate() {
            assert_eq!(actual, expected, "ModApi field #{i} moved");
        }
        assert_eq!(MOD_API_V1_0_SIZE, 96);
        assert!(size_of::<ModApi>() >= 112);
    }

    #[test]
    fn payloads_v1() {
        assert_eq!((size_of::<Str>(), size_of::<Pos>()), (16, 12));
        assert_eq!(size_of::<TickEntry>(), 32);
        assert_eq!(size_of::<BlockCall>(), 56);
        assert_eq!(size_of::<CommandCall>(), 32);
        assert_eq!(size_of::<MenuButton>(), 48);
        assert_eq!(size_of::<BlockWrite>(), 16);
        assert_eq!(size_of::<Property>(), 32);
        assert_eq!(size_of::<BlockEvent>(), 48);
    }

    #[test]
    fn blocks_v1() {
        assert_eq!(offset_of!(BlocksV1, size), 0);
        assert_eq!(offset_of!(BlocksV1, define_block), 8);
        assert_eq!(offset_of!(BlocksV1, schedule_tick), 16);
        assert_eq!(offset_of!(BlocksV1, is_powered), 24);
        assert_eq!(offset_of!(BlockEvent, dimension), 8);
        assert_eq!(offset_of!(BlockEvent, pos), 24);
        assert_eq!(offset_of!(BlockEvent, state), 36);
        assert_eq!(offset_of!(BlockEvent, player), 40);
    }

    #[test]
    fn entities_v1() {
        let fields = [
            (offset_of!(EntitiesV1, size), 0),
            (offset_of!(EntitiesV1, spawn), 8),
            (offset_of!(EntitiesV1, position), 16),
            (offset_of!(EntitiesV1, move_to), 24),
            (offset_of!(EntitiesV1, set_name), 32),
            (offset_of!(EntitiesV1, remove), 40),
        ];
        for (i, (actual, expected)) in fields.into_iter().enumerate() {
            assert_eq!(actual, expected, "EntitiesV1 field #{i} moved");
        }
        assert_eq!((size_of::<EntityId>(), size_of::<Vec3>()), (16, 24));
    }

    #[test]
    fn world_v1() {
        let fields = [
            (offset_of!(WorldV1, size), 0),
            (offset_of!(WorldV1, get_blocks), 8),
            (offset_of!(WorldV1, set_blocks), 16),
            (offset_of!(WorldV1, state_id), 24),
            (offset_of!(WorldV1, state_name), 32),
        ];
        for (i, (actual, expected)) in fields.into_iter().enumerate() {
            assert_eq!(actual, expected, "WorldV1 field #{i} moved");
        }
    }
}
