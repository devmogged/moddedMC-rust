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
    /// Check first, then generate/load if missing. Reports false if the job is interrupted.
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

        assert_eq!(offset_of!(ContentV1, size), 0);
        assert_eq!(offset_of!(ContentV1, add_block), 8);
        assert_eq!(offset_of!(ContentV1, state), 16);
        assert_eq!(offset_of!(ContentV1, on_tick), 24);
        assert_eq!(offset_of!(ContentV1, on_replaced), 32);
        assert_eq!(offset_of!(ContentV1, set_comparator_output), 40);
        assert_eq!(offset_of!(ContentV1, on_random_tick), 48);
        assert_eq!(offset_of!(BlockWorldV1, raw_brightness), 184);
        assert_eq!(offset_of!(ItemsV1, size), 0);
        assert_eq!(offset_of!(ItemsV1, add_item), 8);
        assert_eq!(offset_of!(ContentItemDefinition, carrier), 32);
        assert_eq!(
            offset_of!(ItemStacksV1Fuel, fuel),
            size_of::<ItemStacksV1>()
        );
        assert_eq!(offset_of!(BlockWorldV1, size), 0);
        assert_eq!(offset_of!(BlockWorldV1, read), 8);
        assert_eq!(offset_of!(BlockWorldV1, write), 16);
        assert_eq!(offset_of!(BlockWorldV1, command_world), 24);
        assert_eq!(offset_of!(BlockWorldV1, read_entity), 32);
        assert_eq!(offset_of!(BlockWorldV1, write_entity), 40);
        assert_eq!(offset_of!(BlockWorldV1, rotate), 48);
        assert_eq!(offset_of!(BlockWorldV1, dimension_id), 160);
        assert_eq!(offset_of!(BlockWorldV1, is_thundering), 168);
        assert_eq!(offset_of!(InteractionV1, on_vanilla_use_on_block), 104);
        assert_eq!(offset_of!(InteractionV1, on_item_use), 120);
        assert_eq!(offset_of!(InteractionV1, action_bar), 136);
        assert_eq!(offset_of!(InteractionV1, on_use_mob), 144);
        assert_eq!(size_of::<ShapeBox>(), 12);

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
pub const CONTENT: &str = "moddedmc:content";
pub const BLOCK_WORLD: &str = "moddedmc:block_world";
pub const FLUID_WORLD: &str = "moddedmc:fluid_world";
pub const ITEMS: &str = "moddedmc:items";
pub const ITEM_DEFAULTS: &str = "moddedmc:item_defaults";

#[repr(C)]
pub struct ItemDefaultsV1 {
    pub size: usize,
    /// The named-root ItemStack NBT describes a one-count stack of the vanilla carrier.
    /// Its explicit persistent components become the custom item's default overrides.
    pub add_item: unsafe extern "C" fn(Reg, *const ContentItemDefinition, Str) -> i32,
}
pub const UNLOADED_STATE: u32 = u32::MAX;
pub const MAX_WORLD_BATCH: usize = 4096;

pub const UPDATE_NEIGHBORS: u8 = 1;
pub const UPDATE_CLIENTS: u8 = 2;
pub const UPDATE_ALL: u8 = UPDATE_NEIGHBORS | UPDATE_CLIENTS;
/// Vanilla `UPDATE_MOVE_BY_PISTON | UPDATE_SUPPRESS_DROPS`, and the replaced block entity is
/// removed without its side effects, so a captured container doesn't scatter. Hosts before it
/// reject the whole batch.
pub const UPDATE_MOVED: u8 = 4;
/// Skip neighbour shape updates. Requires the fluid world tick-control extension.
pub const UPDATE_KNOWN_SHAPE: u8 = 8;

#[repr(C)]
pub struct StateDefinition {
    /// Comma-separated name=value pairs, in any order.
    pub properties: Str,
    /// Vanilla state selector used for physics and sound.
    pub physical: Str,
    /// Static block model and optional rotating display model.
    pub model: Str,
    pub rotating_model: Str,
    /// Static model rotation in degrees about Y, then X.
    pub x: i16,
    pub y: i16,
    /// Initial orientation for the rotating model, as an XYZW unit quaternion.
    pub orientation: [f32; 4],
    /// Optional replacement for physics and outline boxes, in sixteenths of a block.
    pub shape: *const ShapeBox,
    pub shape_count: usize,
}

/// A collision box in blocks, relative to the block's minimum corner.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CollisionBox {
    pub min: [f64; 3],
    pub max: [f64; 3],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ShapeBox {
    pub min: [i16; 3],
    pub max: [i16; 3],
}

#[repr(C)]
pub struct ContentBlockDefinition {
    pub id: Str,
    pub display_name: Str,
    /// Empty for a block with no persistent block entity.
    pub entity: Str,
    pub item_model: Str,
    pub states: *const StateDefinition,
    pub count: usize,
    pub on_place: Option<ContentPlacement>,
}

pub type WorldRef = *const c_void;

#[repr(C)]
pub struct Placement {
    pub world: WorldRef,
    pub state: u32,
    pub position: Pos,
    /// Vanilla directions in order down, up, north, south, west, east.
    pub face: u8,
    pub sneak: bool,
    pub yaw: f32,
    pub pitch: f32,
}

pub type ContentPlacement = unsafe extern "C" fn(placement: *const Placement) -> u32;

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BlockUpdate {
    pub position: Pos,
    pub state: u32,
    pub flags: u8,
}

pub const STATE_CHANGED: u8 = 0;
pub const NEIGHBOR_CHANGED: u8 = 1;
pub const LOADED: u8 = 2;
pub const UNLOADED: u8 = 3;
/// Replaced by an `UPDATE_MOVED` write, so a mod can detach the block without breaking it.
pub const MOVED_STATE_CHANGED: u8 = 4;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ContentEvent {
    pub position: Pos,
    pub previous: u32,
    pub state: u32,
    pub reason: u8,
}

pub type ContentTick =
    unsafe extern "C" fn(world: WorldRef, tick: u64, events: *const ContentEvent, count: usize);

pub type ContentRandomTick = unsafe extern "C" fn(world: WorldRef, pos: Pos, state: u32);

pub type ContentReplaced = unsafe extern "C" fn(world: WorldRef, event: *const ContentEvent);

#[repr(C)]
pub struct ContentV1 {
    pub size: usize,
    pub add_block: unsafe extern "C" fn(reg: Reg, def: *const ContentBlockDefinition) -> i32,
    /// Exact name[single=value,...] selector. Missing definitions return UNLOADED_STATE.
    pub state: unsafe extern "C" fn(selector: Str) -> u32,
    pub on_tick: unsafe extern "C" fn(reg: Reg, callback: ContentTick) -> i32,
    /// Appended. Called after the state write, before the old block entity is removed.
    pub on_replaced: unsafe extern "C" fn(reg: Reg, callback: ContentReplaced) -> i32,
    /// Sets the comparator output of a state already staged by this mod.
    pub set_comparator_output: unsafe extern "C" fn(reg: Reg, selector: Str, strength: u8) -> i32,
    pub on_random_tick:
        unsafe extern "C" fn(reg: Reg, block: Str, callback: ContentRandomTick) -> i32,
}

pub const CONTENT_V1_BASE_SIZE: usize = std::mem::offset_of!(ContentV1, on_replaced);

#[repr(C)]
pub struct ContentItemDefinition {
    pub id: Str,
    pub display_name: Str,
    /// Vanilla item to copy components from (stack size, food, tool). Stock clients see it, drawn
    /// with `model`.
    pub carrier: Str,
    pub model: Str,
}

#[repr(C)]
pub struct ItemsV1 {
    pub size: usize,
    pub add_item: unsafe extern "C" fn(reg: Reg, def: *const ContentItemDefinition) -> i32,
    /// Set the maximum damage on a registered standalone, single-stack item.
    pub item_durability: unsafe extern "C" fn(reg: Reg, item: Str, max_damage: u16) -> i32,
}

pub const ITEMS_V1_BASE_SIZE: usize = std::mem::offset_of!(ItemsV1, item_durability);

#[repr(C)]
pub struct BlockWorldV1 {
    pub size: usize,
    pub read:
        unsafe extern "C" fn(world: WorldRef, pos: *const Pos, out: *mut u32, count: usize) -> bool,
    /// Validate every entry, then apply in input order. Returns the count applied.
    pub write:
        unsafe extern "C" fn(world: WorldRef, updates: *const BlockUpdate, count: usize) -> usize,
    pub command_world: unsafe extern "C" fn(source: *const c_void) -> WorldRef,
    /// Read/write Java named-root binary NBT for a custom block entity.
    pub read_entity: unsafe extern "C" fn(world: WorldRef, pos: Pos, out: Sink) -> bool,
    pub write_entity: unsafe extern "C" fn(world: WorldRef, pos: Pos, nbt: Str) -> bool,
    /// Rotation rate in RPM about positive X (0), Y (1), or Z (2).
    pub rotate: unsafe extern "C" fn(world: WorldRef, pos: Pos, axis: u8, rpm: f32) -> bool,
    // Fields below were appended after the first ABI 1 release; check `size` before reading them.
    /// Vanilla `Level.hasNeighborSignal`. False for unloaded positions.
    pub has_neighbor_signal: unsafe extern "C" fn(world: WorldRef, pos: Pos) -> bool,
    /// Vanilla `Level.destroyBlock(pos, drop)`: break particles, loot when `drop`, then air or the
    /// block's water. False when nothing was broken.
    pub destroy: unsafe extern "C" fn(world: WorldRef, pos: Pos, drop: bool) -> bool,
    /// Vanilla `BlockState.isFaceSturdy(level, pos, face)`, face in down, up, north, south, west,
    /// east order. False for unloaded positions.
    pub face_sturdy: unsafe extern "C" fn(world: WorldRef, pos: Pos, face: u8) -> bool,
    /// Vanilla `BlockState.getCollisionShape(level, pos)` with an empty collision context, as
    /// boxes including the block's random offset. Writes up to `capacity` boxes to `out` and
    /// returns how many there are; -1 for unloaded positions.
    pub collision_shape: unsafe extern "C" fn(
        world: WorldRef,
        pos: Pos,
        out: *mut CollisionBox,
        capacity: usize,
    ) -> i32,
    /// The state's full `namespace:block[key=value,...]` selector, the form `Content::state`
    /// reads, with every property. False for an unknown state.
    pub describe_state: unsafe extern "C" fn(state: u32, out: Sink) -> bool,
    /// Vanilla `FluidState.getType()` registry id at the position, including the water in
    /// waterlogged and aquatic blocks, `minecraft:empty` for none. False for unloaded positions.
    pub fluid_id: unsafe extern "C" fn(world: WorldRef, pos: Pos, out: Sink) -> bool,
    /// The block entity's full saved NBT as Java named-root binary NBT, vanilla's
    /// `saveWithFullMetadata`: type id, position and fields, for any block entity. False when
    /// there is none or the position is unloaded.
    pub read_block_entity: unsafe extern "C" fn(world: WorldRef, pos: Pos, out: Sink) -> bool,
    /// Replaces the block entity at the position with saved NBT from `read_block_entity`, after
    /// the destination block is placed. The saved id must match the destination's block entity;
    /// the saved position is ignored. Marks the chunk dirty and sends clients the new data.
    /// False for unloaded positions, a mismatched id or invalid NBT.
    pub restore_block_entity: unsafe extern "C" fn(world: WorldRef, pos: Pos, nbt: Str) -> bool,
    /// Vanilla `Block.getDrops(state, level, pos, blockEntity, null, tool)` for the loaded block:
    /// one saved stack per sink call, without changing the block or spawning anything. An empty
    /// `tool` is an empty hand. False when unloaded or the tool can't be read.
    pub block_drops: unsafe extern "C" fn(world: WorldRef, pos: Pos, tool: Str, out: Sink) -> bool,
    /// The overworld clock's total ticks, vanilla 26.3 `Level.getOverworldClockTime()`, which is
    /// the day time every dimension shared in 1.21.1, and whether this dimension has a fixed time.
    /// False when the world is gone.
    pub day_time: unsafe extern "C" fn(world: WorldRef, out: *mut DayTime) -> bool,
    /// Vanilla `BlockState.getDestroySpeed(level, pos)` for the loaded state: its hardness,
    /// negative for unbreakable blocks. False for unloaded positions.
    pub destroy_speed: unsafe extern "C" fn(world: WorldRef, pos: Pos, out: *mut f32) -> bool,
    /// Vanilla `BlockState.canSurvive(level, pos)` for `state` as if it stood at `pos`, without
    /// changing the world. 1 or 0, or -1 for an unknown state or an unloaded position.
    pub can_survive: unsafe extern "C" fn(world: WorldRef, pos: Pos, state: u32) -> i32,
    /// Vanilla `Block.asItem()` for the state's block as a namespaced item id, `minecraft:air`
    /// when the block has no item. False for an unknown state.
    pub block_item: unsafe extern "C" fn(state: u32, out: Sink) -> bool,
    /// The world's canonical dimension registry id. Static, or empty for an invalid handle.
    pub dimension_id: unsafe extern "C" fn(world: WorldRef) -> Str,
    pub is_thundering: unsafe extern "C" fn(world: WorldRef) -> bool,
    /// Vanilla `Level.getBestNeighborSignal`. Gives 0 for unloaded positions.
    pub best_neighbor_signal: unsafe extern "C" fn(world: WorldRef, pos: Pos) -> u8,
    /// Vanilla raw brightness with explicit sky darkening; -1 when unloaded.
    pub raw_brightness: unsafe extern "C" fn(world: WorldRef, pos: Pos, sky_darken: u8) -> i32,
    /// Apply a tool's block transformer along a one-block outline ray from the origin's center.
    pub apply_block_transformer:
        unsafe extern "C" fn(world: WorldRef, origin: Pos, direction: u8, tool: Str) -> bool,
    /// -1 when unloaded, 1 if the state conducts redstone, 0 otherwise.
    pub redstone_conductor: unsafe extern "C" fn(world: WorldRef, pos: Pos) -> i32,
    /// Roll block loot with the saved tool and an unspawned player-typed actor.
    pub block_drops_with_player_actor:
        unsafe extern "C" fn(world: WorldRef, pos: Pos, tool: Str, out: Sink) -> bool,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DayTime {
    pub ticks: i64,
    /// Vanilla `DimensionType.hasFixedTime()`. In vanilla it holds exactly for the dimensions
    /// 1.21.1 called not `natural` (the Nether and the End).
    pub fixed_time: bool,
}

/// Size of the block world table before `has_neighbor_signal` was appended.
pub const BLOCK_WORLD_V1_BASE_SIZE: usize = std::mem::offset_of!(BlockWorldV1, has_neighbor_signal);

pub const FLUID_SOURCE: u32 = 1;
pub const FLUID_REPLACEABLE: u32 = 2;
pub const FLUID_HAS_WATERLOGGED: u32 = 4;
pub const FLUID_WATERLOGGED: u32 = 8;
pub const FLUID_LIQUID_BLOCK: u32 = 16;
pub const FLUID_BLOCKS_MOTION: u32 = 32;
/// Request the fluid's normal delay for the dimension.
pub const DEFAULT_FLUID_TICK_DELAY: u32 = u32::MAX;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct FluidCell {
    pub state: u32,
    pub flags: u32,
}

#[repr(C)]
pub struct FluidWorldV1 {
    pub size: usize,
    /// Return the loaded cell's block state and flags. The sink gets the still-fluid registry id
    /// once when there is fluid. An empty loaded cell succeeds without calling the sink.
    pub inspect:
        unsafe extern "C" fn(world: WorldRef, pos: Pos, cell: *mut FluidCell, fluid: Sink) -> bool,
    pub schedule_tick:
        unsafe extern "C" fn(world: WorldRef, pos: Pos, fluid: Str, delay: u32) -> bool,
    /// Source conversion for a hypothetical flowing block. Returns -1 if unavailable.
    pub would_regenerate_source: unsafe extern "C" fn(world: WorldRef, pos: Pos, fluid: Str) -> i32,
    /// Vanilla 26.3 WATER_EVAPORATES environment attribute.
    pub water_evaporates: unsafe extern "C" fn(world: WorldRef, pos: Pos) -> bool,
}

#[repr(C)]
pub struct FluidWorldV1TickControl {
    pub base: FluidWorldV1,
    /// Clear pending fluid ticks at a loaded position.
    pub clear_ticks: unsafe extern "C" fn(world: WorldRef, pos: Pos) -> bool,
}

#[repr(C)]
pub struct FluidWorldV1Flow {
    pub base: FluidWorldV1TickControl,
    /// Read the flow vector of a loaded cell. Loaded dry cells return a zero vector.
    pub velocity: unsafe extern "C" fn(world: WorldRef, pos: Pos, out: *mut Vec3) -> bool,
}

pub const ENTITIES: &str = "moddedmc:entities";

/// An entity borrowed for one callback.
pub type EntityRef = *const c_void;

pub const ENTITY_PLAYER: u8 = 1;
pub const ENTITY_LIVING: u8 = 2;
pub const ENTITY_ITEM: u8 = 4;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EntityView {
    /// `ENTITY_*` bits.
    pub kind: u8,
    pub on_ground: bool,
    pub position: [f64; 3],
    pub motion: [f64; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub head_yaw: f32,
    pub body_yaw: f32,
}

/// A vector passed by value; bare arrays aren't FFI-safe arguments.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec3(pub [f64; 3]);

/// An entity's collision box and its movement input (vanilla `xxa`, `yya`, `zza`; zero for
/// entities that aren't living). A player's comes from the movement keys their client last sent.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EntityBody {
    pub min: [f64; 3],
    pub max: [f64; 3],
    pub input: [f64; 3],
}

pub const SET_MOTION: u32 = 1;
pub const SET_YAW: u32 = 2;
pub const SET_HEAD_YAW: u32 = 4;
pub const SET_BODY_YAW: u32 = 8;
pub const SET_ON_GROUND: u32 = 16;

/// Fields to change on an entity; `mask` (`SET_*` bits) says which ones apply.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EntityChange {
    pub mask: u32,
    pub on_ground: bool,
    pub motion: [f64; 3],
    pub yaw: f32,
    pub head_yaw: f32,
    pub body_yaw: f32,
}

/// Vanilla `Block.entityInside` for one of the mod's content blocks: the entity's box overlaps
/// the block at `pos`, which has `state`.
pub type EntityInside =
    unsafe extern "C" fn(world: WorldRef, pos: Pos, state: u32, entity: EntityRef);

#[repr(C)]
pub struct EntitiesV1 {
    pub size: usize,
    /// One callback per mod, staged during registration.
    pub on_entity_inside: unsafe extern "C" fn(reg: Reg, callback: EntityInside) -> i32,
    pub view: unsafe extern "C" fn(entity: EntityRef, out: *mut EntityView) -> bool,
    /// The entity type's resource name, such as `minecraft:zombie`. Static.
    pub type_id: unsafe extern "C" fn(entity: EntityRef) -> Str,
    /// Apply an `EntityChange`; motion changes are sent to clients like vanilla `hurtMarked`.
    pub change: unsafe extern "C" fn(entity: EntityRef, change: *const EntityChange) -> bool,
    /// Appended after the first release: ids, lookup and movement. Require
    /// `EntitiesExtension::Movement`.
    pub id: unsafe extern "C" fn(entity: EntityRef) -> i32,
    /// Call `visit` with the loaded entity `id` in `world`, valid only during that call. Returns
    /// false when there is no such entity.
    pub with_entity: unsafe extern "C" fn(
        world: WorldRef,
        id: i32,
        ctx: *mut c_void,
        visit: unsafe extern "C" fn(ctx: *mut c_void, entity: EntityRef),
    ) -> bool,
    /// `ENTITY_ALIVE`, `ENTITY_SNEAKING`, `ENTITY_FLYING`, `ENTITY_HANGING` and
    /// `ENTITY_PUSH_IGNORED` bits.
    pub flags: unsafe extern "C" fn(entity: EntityRef) -> u32,
    /// Vanilla `Entity.move(MoverType.SELF, delta)`: collide with blocks and step up.
    pub move_by: unsafe extern "C" fn(entity: EntityRef, delta: Vec3),
    pub set_position: unsafe extern "C" fn(entity: EntityRef, position: Vec3),
    pub body: unsafe extern "C" fn(entity: EntityRef, out: *mut EntityBody) -> bool,
    pub reset_fall_distance: unsafe extern "C" fn(entity: EntityRef),
    /// Vanilla `LivingEntity.addEffect` for a status effect id such as `minecraft:slowness`.
    /// False when the entity isn't living or the effect is unknown.
    pub add_effect: unsafe extern "C" fn(
        entity: EntityRef,
        effect: Str,
        duration: i32,
        amplifier: u8,
        ambient: bool,
        visible: bool,
    ) -> bool,
    /// The `minecraft:step_height` base value, or NaN when the entity isn't living.
    pub step_height: unsafe extern "C" fn(entity: EntityRef) -> f64,
    pub set_step_height: unsafe extern "C" fn(entity: EntityRef, value: f64),
    /// Ids of the entities whose boxes intersect `min..max`, except `except`. Returns how many
    /// there are, which can exceed `capacity`.
    pub entities_in: unsafe extern "C" fn(
        world: WorldRef,
        min: Vec3,
        max: Vec3,
        except: i32,
        out: *mut i32,
        capacity: usize,
    ) -> usize,
    /// Vanilla `Entity.hasIndirectPassenger`: `other` rides `entity`, directly or further up.
    pub has_indirect_passenger: unsafe extern "C" fn(entity: EntityRef, other: i32) -> bool,
    /// Appended after `has_indirect_passenger`: mod data saved with the entity, the counterpart
    /// of NeoForge `Entity.getPersistentData()`. Require `EntitiesExtension::Data`.
    /// The compound saved under `key` as Java named-root NBT; false when there is none.
    pub data: unsafe extern "C" fn(entity: EntityRef, key: Str, out: Sink) -> bool,
    /// Replace the compound under `key` with Java named-root NBT of at most
    /// `MAX_ENTITY_DATA_BYTES`; empty removes it.
    pub set_data: unsafe extern "C" fn(entity: EntityRef, key: Str, nbt: Str) -> bool,
    /// Vanilla `Player.mayBuild()`: false for other entities and for Adventure and Spectator
    /// players.
    pub may_build: unsafe extern "C" fn(entity: EntityRef) -> bool,
    /// Appended after `may_build`: carriers, discarding and riding. Require
    /// `EntitiesExtension::Carriers`.
    /// Spawn a saved, non-living `CARRIER_*` entity at `position`, with Java named-root NBT
    /// `nbt` stored under mod data `key` before it is added (an empty key stores nothing).
    /// Returns its id, or -1 for an unknown kind, invalid input or an unloaded position.
    pub spawn_carrier:
        unsafe extern "C" fn(world: WorldRef, kind: u8, position: Vec3, key: Str, nbt: Str) -> i32,
    /// Vanilla `Entity.discard()`: removed without drops or a save, its riders dismounted. False
    /// for players and for entities already removed.
    pub discard: unsafe extern "C" fn(entity: EntityRef) -> bool,
    /// Vanilla `Entity.startRiding(vehicle, force)`: dismounts from any current vehicle first and
    /// sends the passenger list. Without `force` the rider must not be sneaking and the vehicle
    /// must have no passengers. False for an unloaded vehicle, its current vehicle, a cycle or a
    /// cancelled mount event.
    pub start_riding: unsafe extern "C" fn(entity: EntityRef, vehicle: i32, force: bool) -> bool,
    /// Vanilla `Entity.stopRiding()`, placing a living rider at its dismount position. False when
    /// it wasn't riding.
    pub stop_riding: unsafe extern "C" fn(entity: EntityRef) -> bool,
    /// The id of the entity this one rides, 0 when it rides none.
    pub vehicle: unsafe extern "C" fn(entity: EntityRef) -> i32,
    /// `entities_in`, skipping entities that have any of the `exclude` flag bits before counting,
    /// and leaving out the first `skip` matches. Returns how many match after `skip`, which can
    /// exceed `capacity`; call again with a larger `skip` for the rest.
    pub entities_matching: unsafe extern "C" fn(
        world: WorldRef,
        min: Vec3,
        max: Vec3,
        except: i32,
        exclude: u32,
        skip: usize,
        out: *mut i32,
        capacity: usize,
    ) -> usize,
    /// Borrow a player handle from an entity for the same callback lifetime.
    pub as_player: unsafe extern "C" fn(entity: EntityRef) -> PlayerRef,
    pub with_player_context: unsafe extern "C" fn(
        player: PlayerRef,
        context: *mut c_void,
        visit: unsafe extern "C" fn(*mut c_void, WorldRef, EntityRef),
    ) -> bool,
    /// Hurt this entity with a registered, unattributed damage type.
    pub hurt: unsafe extern "C" fn(entity: EntityRef, damage_type: Str, amount: f32) -> bool,
}

/// A vanilla `minecraft:marker`: saved, never sent to clients.
pub const CARRIER_MARKER: u8 = 0;
/// An empty `minecraft:item_display`: saved and invisible, with its passenger seat at its own
/// position, so clients draw a rider sitting there.
pub const CARRIER_SEAT: u8 = 1;

/// Largest compound `EntitiesV1::set_data` accepts.
pub const MAX_ENTITY_DATA_BYTES: usize = 64 << 10;
/// Longest `EntitiesV1::data` key.
pub const MAX_ENTITY_DATA_KEY: usize = 64;
/// Largest `restore_block_entity` input, vanilla's 2 MiB network NBT budget.
pub const MAX_BLOCK_ENTITY_BYTES: usize = 2 << 20;

/// Size of the entities table before `id` was appended.
pub const ENTITIES_V1_BASE_SIZE: usize = std::mem::offset_of!(EntitiesV1, id);

pub const ENTITY_ALIVE: u32 = 1;
pub const ENTITY_SNEAKING: u32 = 2;
pub const ENTITY_FLYING: u32 = 4;
/// A vanilla `HangingEntity`: a painting or an item frame.
pub const ENTITY_HANGING: u32 = 8;
/// Vanilla `getPistonPushReaction()` is `IGNORE_ENTITY`: markers, displays, interactions, area
/// effect clouds, ominous item spawners and marker armor stands.
pub const ENTITY_PUSH_IGNORED: u32 = 16;
/// Vanilla `Entity.noPhysics`. Hosts set it only with `EntitiesExtension::Carriers`.
pub const ENTITY_NO_PHYSICS: u32 = 32;
/// The entity rides another one. Hosts set it only with `EntitiesExtension::Carriers`.
pub const ENTITY_PASSENGER: u32 = 64;

pub const INTERACTION: &str = "moddedmc:interaction";

/// The player's held stack, borrowed for one callback.
pub type StackRef = *mut c_void;

pub const USE_PASS: i32 = 0;
pub const USE_SUCCESS: i32 = 1;
pub const USE_CONSUME: i32 = 2;
pub const USE_FAIL: i32 = 3;

/// Vanilla `UseOnContext` for one of the mod's items.
#[repr(C)]
pub struct UseOnBlock {
    /// The held content item's id.
    pub item: Str,
    pub position: Pos,
    /// Vanilla directions in order down, up, north, south, west, east.
    pub face: u8,
    /// Hit point inside the clicked block, each axis 0 to 1.
    pub hit: [f32; 3],
    pub sneaking: bool,
    pub creative: bool,
}

/// Returns a `USE_*` code. `PASS` lets the server place the item's block, if it has one.
pub type UseOnBlockFn = unsafe extern "C" fn(
    world: WorldRef,
    player: EntityRef,
    stack: StackRef,
    context: *const UseOnBlock,
) -> i32;

pub const ITEM_USE_START: u8 = 0;
pub const ITEM_USE_FINISH: u8 = 1;
pub const ITEM_USE_RELEASE: u8 = 2;

/// The held item and ray hit from a normal item-use action.
#[repr(C)]
pub struct ItemUseContext {
    pub item: Str,
    pub hit: [f64; 3],
    pub phase: u8,
    pub main_hand: bool,
    pub creative: bool,
}

/// Return zero to pass, -1 to handle without starting a timer, or a positive duration on START.
pub type ItemUseFn = unsafe extern "C" fn(
    world: WorldRef,
    player: EntityRef,
    stack: StackRef,
    other_hand: StackRef,
    context: *const ItemUseContext,
) -> i32;

#[repr(C)]
pub struct InteractionV1 {
    pub size: usize,
    /// One callback per mod for all of its content items, staged during registration.
    pub on_use_on_block: unsafe extern "C" fn(reg: Reg, callback: UseOnBlockFn) -> i32,
    pub stack_count: unsafe extern "C" fn(stack: StackRef) -> u32,
    pub stack_shrink: unsafe extern "C" fn(stack: StackRef, count: u32),
    /// Copy the stack's mod data into `out`. Returns its length, which can exceed `capacity`,
    /// or `usize::MAX` when the stack has none.
    pub stack_data: unsafe extern "C" fn(stack: StackRef, out: *mut u8, capacity: usize) -> usize,
    /// Replace the stack's mod data, at most `MAX_STACK_DATA` bytes. Empty removes it.
    pub set_stack_data: unsafe extern "C" fn(stack: StackRef, data: *const u8, len: usize) -> bool,
    /// Vanilla `ItemCooldowns.addCooldown` for the item's cooldown group.
    pub cooldown: unsafe extern "C" fn(player: EntityRef, item: Str, ticks: u32),
    // Appended after the first release: require `InteractionExtension::BlockUse`.
    /// One callback per mod for its content blocks, staged during registration.
    pub on_use_block: unsafe extern "C" fn(reg: Reg, callback: UseBlockFn) -> i32,
    /// The stack as saved NBT (vanilla `ItemStack.CODEC`). False when it is empty.
    pub stack_read: unsafe extern "C" fn(stack: StackRef, out: Sink) -> bool,
    /// Replace the stack from saved NBT; empty clears it.
    pub stack_write: unsafe extern "C" fn(stack: StackRef, nbt: Str) -> bool,
    /// Vanilla `Inventory.placeItemBackInInventory`: add a saved stack to the player's inventory
    /// and drop what doesn't fit.
    pub give: unsafe extern "C" fn(player: EntityRef, stack: Str) -> bool,
    // Appended later: require `InteractionExtension::PlayerWillDestroy`.
    /// One callback per mod for its content blocks, staged during registration.
    pub on_player_will_destroy:
        unsafe extern "C" fn(reg: Reg, callback: PlayerWillDestroyFn) -> i32,
    // Appended later: require `InteractionExtension::Redstone`.
    /// One pair per mod for its content blocks, staged during registration.
    pub on_redstone: unsafe extern "C" fn(
        reg: Reg,
        signal: RedstoneSignalFn,
        connects: RedstoneConnectsFn,
    ) -> i32,
    /// One callback per mod, before the vanilla item behaviour. PASS continues dispatch.
    pub on_vanilla_use_on_block: unsafe extern "C" fn(reg: Reg, callback: UseOnBlockFn) -> i32,
    /// Place the current hand's block through normal player placement. The caller spends items.
    pub place_held_block: unsafe extern "C" fn(player: EntityRef, clicked: Pos, face: u8) -> bool,
    /// One callback per mod for normal uses of its content items.
    pub on_item_use: unsafe extern "C" fn(reg: Reg, callback: ItemUseFn) -> i32,
    /// Give a registered item stock-client held-use prediction for the given tick duration.
    pub item_use_animation: unsafe extern "C" fn(reg: Reg, item: Str, ticks: u16) -> i32,
    /// Show a short status message to the player in an interaction callback.
    pub action_bar: unsafe extern "C" fn(player: EntityRef, text: Str),
    // Appended later: require `InteractionExtension::MobUse`.
    /// During registration: vanilla `Mob.mobInteract` for a mod mob type (`moddedmc:mobs`),
    /// one callback per type. A result other than `USE_PASS` ends the click there.
    pub on_use_mob: unsafe extern "C" fn(reg: Reg, kind: Str, callback: UseMobFn) -> i32,
    /// During registration: one direct melee modifier for the mod's content items.
    pub on_melee_damage: unsafe extern "C" fn(reg: Reg, callback: MeleeDamageFn) -> i32,
    /// One Item.use callback per mod, staged during registration.
    pub on_use_item: unsafe extern "C" fn(reg: Reg, callback: UseItemFn) -> i32,
    pub on_player_placed: unsafe extern "C" fn(reg: Reg, callback: PlayerPlacedFn) -> i32,
    /// Vanilla Player.causeFoodExhaustion.
    pub add_exhaustion: unsafe extern "C" fn(player: EntityRef, amount: f32),
}

/// A content item used in the air, with the rotation sent for that action.
#[repr(C)]
pub struct UseItem {
    pub item: Str,
    pub yaw: f32,
    pub pitch: f32,
}

pub type PlayerPlacedFn = unsafe extern "C" fn(WorldRef, EntityRef, Pos, u32);

pub type UseItemFn = unsafe extern "C" fn(
    world: WorldRef,
    player: EntityRef,
    stack: StackRef,
    context: *const UseItem,
) -> i32;

/// Returns replacement damage before defenses. Empty `target_kind` means a vanilla entity.
pub type MeleeDamageFn = unsafe extern "C" fn(item: Str, target_kind: Str, damage: f32) -> f32;

/// A player right-clicking a mod mob with one hand.
#[repr(C)]
pub struct UseMob {
    pub sneaking: bool,
    pub creative: bool,
    pub main_hand: bool,
}

/// Returns a `USE_*` code. `stack` is the stack in the hand used, possibly empty.
pub type UseMobFn = unsafe extern "C" fn(
    world: WorldRef,
    player: EntityRef,
    stack: StackRef,
    mob: EntityRef,
    context: *const UseMob,
) -> i32;

/// Vanilla `BlockState.getSignal(level, pos, direction)`, or `getDirectSignal` when `direct`, for
/// one of the mod's content blocks: 0 to 15. `direction` is vanilla's argument, in order down, up,
/// north, south, west, east.
pub type RedstoneSignalFn =
    unsafe extern "C" fn(world: WorldRef, pos: Pos, state: u32, direction: u8, direct: bool) -> u8;
/// Vanilla `isSignalSource()` together with NeoForge `canConnectRedstone(direction)` for the
/// state: whether redstone treats it as a source on that side. Pure: it gets no world.
pub type RedstoneConnectsFn = unsafe extern "C" fn(state: u32, direction: u8) -> bool;

/// Vanilla `Block.playerWillDestroy`: a player is about to break one of the mod's blocks, after
/// the break was accepted and before drops and removal. `creative` is the player's game mode.
pub type PlayerWillDestroyFn = unsafe extern "C" fn(
    world: WorldRef,
    player: EntityRef,
    position: Pos,
    state: u32,
    creative: bool,
);

/// Size of the interaction table before `on_use_block` was appended.
pub const INTERACTION_V1_BASE_SIZE: usize = std::mem::offset_of!(InteractionV1, on_use_block);

/// From `useItemOn`: try `useWithoutItem` next (vanilla `TRY_WITH_EMPTY_HAND`).
pub const USE_TRY_EMPTY_HAND: i32 = 4;

/// A player using one of the mod's content blocks.
#[repr(C)]
pub struct UseBlock {
    pub position: Pos,
    pub state: u32,
    /// Vanilla directions in order down, up, north, south, west, east.
    pub face: u8,
    /// Hit point inside the block, each axis 0 to 1.
    pub hit: [f32; 3],
    pub sneaking: bool,
    pub creative: bool,
    pub main_hand: bool,
    /// True for vanilla `useItemOn` (the held stack, possibly empty), false for `useWithoutItem`
    /// (an empty stack).
    pub with_item: bool,
}

/// Returns a `USE_*` code.
pub type UseBlockFn = unsafe extern "C" fn(
    world: WorldRef,
    player: EntityRef,
    stack: StackRef,
    context: *const UseBlock,
) -> i32;

pub const EFFECTS: &str = "moddedmc:effects";

/// Vanilla `SoundSource` ids, in declaration order.
pub const SOUND_MASTER: u8 = 0;
pub const SOUND_BLOCKS: u8 = 4;
pub const SOUND_NEUTRAL: u8 = 6;
pub const SOUND_PLAYERS: u8 = 7;
pub const SOUND_AMBIENT: u8 = 8;

#[repr(C)]
pub struct EffectsV1 {
    pub size: usize,
    /// Vanilla `Level.playSound(null, ...)` for players in range. `sound` is a sound event id;
    /// ids outside `minecraft:` play from the resource pack. False for an unknown source.
    pub play_sound: unsafe extern "C" fn(
        world: WorldRef,
        position: Vec3,
        sound: Str,
        source: u8,
        volume: f32,
        pitch: f32,
    ) -> bool,
    /// Vanilla `ServerLevel.sendParticles` for a particle type without options. False for an
    /// unknown particle.
    pub particles: unsafe extern "C" fn(
        world: WorldRef,
        particle: Str,
        position: Vec3,
        spread: Vec3,
        speed: f32,
        count: u32,
    ) -> bool,
}

pub const MAX_STACK_DATA: usize = 1024;

pub const ITEM_STACKS: &str = "moddedmc:item_stacks";

/// Longest saved stack the host accepts.
pub const MAX_STACK_BYTES: usize = 64 * 1024;

/// Stacks cross this interface as vanilla `ItemStack.CODEC` NBT: Java named-root binary with
/// `id`, `count` and `components`.
#[repr(C)]
pub struct ItemStacksV1 {
    pub size: usize,
    /// The stack an item entity carries. False when the entity isn't an item or holds nothing.
    pub entity_stack: unsafe extern "C" fn(entity: EntityRef, out: Sink) -> bool,
    /// Replace an item entity's stack. An empty `stack` discards the entity.
    pub set_entity_stack: unsafe extern "C" fn(entity: EntityRef, stack: Str) -> bool,
    /// Vanilla `new ItemEntity(level, x, y, z, stack)`, `setDeltaMovement(motion)`,
    /// `setDefaultPickUpDelay()` and `addFreshEntity`. Returns the entity id, 0 when nothing
    /// spawned.
    pub spawn:
        unsafe extern "C" fn(world: WorldRef, position: Vec3, motion: Vec3, stack: Str) -> i32,
    /// False for a stack the host can't read.
    pub info: unsafe extern "C" fn(stack: Str, out: *mut StackInfo) -> bool,
    // Fields below were appended after the first ABI 1 release; check `size` before reading them.
    /// The stack's effective component, its patch over the item's defaults, as named-root NBT
    /// `{value: <component>}`. An empty root means the component is present but has no value
    /// (unit components, or ones Pumpkin doesn't encode yet). False when absent, for an unknown
    /// component id or an unreadable stack.
    pub component: unsafe extern "C" fn(stack: Str, component: Str, out: Sink) -> bool,
}

#[repr(C)]
pub struct ItemStacksV1Fuel {
    pub base: ItemStacksV1,
    /// Effective furnace fuel ticks. The sink receives the saved crafting remainder, if any.
    pub fuel: unsafe extern "C" fn(stack: Str, remainder: Sink) -> u32,
}

#[repr(C)]
pub struct ItemStacksV1MaxEnchantment {
    pub base: ItemStacksV1Fuel,
    /// Whether an applied enchantment reaches its defined maximum level.
    pub has_maxed_enchantment: unsafe extern "C" fn(stack: Str) -> bool,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StackInfo {
    pub count: u32,
    /// Vanilla `ItemStack.getMaxStackSize`.
    pub max_count: u32,
    /// The item places a block (vanilla `BlockItem`).
    pub block: bool,
}

pub const DISPLAYS: &str = "moddedmc:displays";

/// Where and how an item display is drawn.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ItemDisplay {
    pub position: [f64; 3],
    /// XYZW unit quaternion, vanilla `Display.left_rotation`.
    pub rotation: [f32; 4],
    pub scale: f32,
    /// Vanilla `ItemDisplayContext` id: 0 none, 1 and 2 third person, 3 and 4 first person,
    /// 5 head, 6 gui, 7 ground, 8 fixed, 9 on shelf.
    pub context: u8,
    /// Ticks clients take to move to a new position, vanilla `teleport_duration`, 0 to 59.
    pub interpolation: i32,
}

/// Item displays a mod draws for things that aren't entities, such as items on a belt. They are
/// never saved; the id stops working when the display's chunk unloads.
#[repr(C)]
pub struct DisplaysV1 {
    pub size: usize,
    /// Returns the display's id, 0 when nothing spawned.
    pub spawn_item:
        unsafe extern "C" fn(world: WorldRef, stack: Str, display: *const ItemDisplay) -> i32,
    /// False when the id no longer names a live display; spawn a new one then.
    pub update: unsafe extern "C" fn(world: WorldRef, id: i32, display: *const ItemDisplay) -> bool,
    /// Replace the displayed stack.
    pub set_item: unsafe extern "C" fn(world: WorldRef, id: i32, stack: Str) -> bool,
    pub remove: unsafe extern "C" fn(world: WorldRef, id: i32),
}

pub const CRAFTING: &str = "moddedmc:crafting";
pub const MAX_CRAFTING_SIDE: u8 = 9;

/// The engine's crafting matcher and effective crafting remainders.
#[repr(C)]
pub struct CraftingV1 {
    pub size: usize,
    /// The grid is width * height saved-stack slices in row order. Empty slices mean empty slots.
    /// Each side is 1..=MAX_CRAFTING_SIDE. On success the sink receives the saved result (key 0),
    /// a recipe id if known (key 1), and a serializer id (key 2).
    pub match_recipe: unsafe extern "C" fn(
        world: WorldRef,
        width: u8,
        height: u8,
        grid: *const Str,
        out: Sink,
    ) -> bool,
    /// The ingredient's default crafting remainder as saved stack NBT, if any.
    pub remainder: unsafe extern "C" fn(stack: Str, out: Sink) -> bool,
}

pub const RESOURCES: &str = "moddedmc:resources";

/// The largest datapack file `ResourcesV1::read` returns.
pub const MAX_RESOURCE_BYTES: usize = 4 << 20;

/// The server's merged datapack files: mod data, the world's datapacks and Pumpkin's bundled
/// packs, as vanilla `ResourceManager` sees them. Calls read the disk, so cache what you parse
/// and reparse when `generation` changes.
#[repr(C)]
pub struct ResourcesV1 {
    pub size: usize,
    /// Vanilla `listResources(directory, ...)`: every file under `data/<namespace>/<directory>/`
    /// as a `namespace:directory/...` id, each once and sorted, one `out` call per id. False for
    /// an invalid directory or before datapacks load.
    pub list: unsafe extern "C" fn(directory: Str, out: Sink) -> bool,
    /// The bytes of the highest-priority pack's file for a `list` id, in one `out` call. False
    /// when no pack has it or it is larger than `MAX_RESOURCE_BYTES`.
    pub read: unsafe extern "C" fn(id: Str, out: Sink) -> bool,
    /// 0 before datapacks first load, then a new value after every load, `/reload` included.
    pub generation: unsafe extern "C" fn() -> u64,
}

pub const CONTAINERS: &str = "moddedmc:containers";

/// The side for a container lookup without one, NeoForge's `null` side: every slot, no face rules.
pub const NO_FACE: u8 = 6;

/// Vanilla block containers (chests, barrels, hoppers, furnaces, shulker boxes, the composter and
/// the rest a hopper sees) as NeoForge's `IItemHandler`, so ported mod code that moves items keeps
/// its slot-by-slot semantics. `face` is the container's side being accessed, in vanilla order
/// down, up, north, south, west, east, or `NO_FACE`; a sided container then shows only that face's
/// slots. Stacks are saved `ItemStack` NBT as in `moddedmc:item_stacks`.
#[repr(C)]
pub struct ContainersV1 {
    pub size: usize,
    /// `getSlots`, or -1 when there is no container at the position.
    pub slots: unsafe extern "C" fn(world: WorldRef, position: Pos, face: u8) -> i32,
    /// `getStackInSlot`, one `out` call. False for an empty slot or no container.
    pub stack: unsafe extern "C" fn(
        world: WorldRef,
        position: Pos,
        face: u8,
        slot: u32,
        out: Sink,
    ) -> bool,
    /// `getSlotLimit`, 0 when there is no such slot.
    pub slot_limit:
        unsafe extern "C" fn(world: WorldRef, position: Pos, face: u8, slot: u32) -> u32,
    /// `insertItem`: what didn't go in goes to `out`, nothing when all of it did. False, leaving
    /// the stack with the caller, for no container or an unreadable stack.
    pub insert: unsafe extern "C" fn(
        world: WorldRef,
        position: Pos,
        face: u8,
        slot: u32,
        stack: Str,
        simulate: bool,
        out: Sink,
    ) -> bool,
    /// `ItemHandlerHelper.insertItemStacked`: matching stacks first, then empty slots, in one call.
    /// The remainder goes to `out` as for `insert`.
    pub insert_stacked: unsafe extern "C" fn(
        world: WorldRef,
        position: Pos,
        face: u8,
        stack: Str,
        simulate: bool,
        out: Sink,
    ) -> bool,
    /// `extractItem`: the stack taken goes to `out`. False when nothing could be taken.
    pub extract: unsafe extern "C" fn(
        world: WorldRef,
        position: Pos,
        face: u8,
        slot: u32,
        amount: u32,
        simulate: bool,
        out: Sink,
    ) -> bool,
}

pub const ITEM_INPUT: &str = "moddedmc:item_input";

/// A hopper or dropper offers saved stack NBT. Return the accepted item count.
pub type ItemInputFn = unsafe extern "C" fn(
    world: WorldRef,
    position: Pos,
    state: u32,
    face: u8,
    stack: Str,
    simulate: bool,
) -> u32;

#[repr(C)]
pub struct ItemInputV1 {
    pub size: usize,
    /// Attach to a previously staged content block of the registering mod.
    pub add: unsafe extern "C" fn(reg: Reg, block: Str, receive: ItemInputFn) -> i32,
}

pub const TAGS: &str = "moddedmc:tags";

/// Vanilla tags merged with every loaded datapack's, by id string so content mods' blocks and
/// items count too. `registry` is a registry name such as `block`, `item`, `fluid` or
/// `entity_type`; `tag` is `namespace:path`, with or without `#`, `minecraft:` when bare. Nested
/// tags are flattened. Results change with `moddedmc:resources` `generation`; cache them.
#[repr(C)]
pub struct TagsV1 {
    pub size: usize,
    /// Every entry id of the tag, sorted, one `out` call each. False for an unknown registry or
    /// a tag no pack defines.
    pub values: unsafe extern "C" fn(registry: Str, tag: Str, out: Sink) -> bool,
    /// Whether `id` is in the tag.
    pub contains: unsafe extern "C" fn(registry: Str, tag: Str, id: Str) -> bool,
}

#[repr(C)]
pub struct TagsV1Memberships {
    pub base: TagsV1,
    /// Every tag containing an entry, sorted by resource location.
    pub memberships: unsafe extern "C" fn(registry: Str, id: Str, out: Sink) -> bool,
}

pub const BLOCK_DISPLAYS: &str = "moddedmc:block_displays";

/// Where and how a block display is drawn. The client draws the block's model from its corner at
/// `translation`, turned by `rotation` and sized by `scale` about the display's position, which is
/// how a moving contraption keeps every block on one pivot.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlockDisplay {
    pub position: [f64; 3],
    pub translation: [f32; 3],
    /// XYZW unit quaternion, vanilla `Display.left_rotation`.
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
    /// Ticks clients take to blend into a new translation, rotation and scale, vanilla
    /// `interpolation_duration`; each update restarts the blend.
    pub transform_interpolation: i32,
    /// Ticks clients take to move to a new position, vanilla `teleport_duration`, 0 to 59.
    pub position_interpolation: i32,
    /// Vanilla `brightness_override` packed as `block << 4 | sky << 20`, or -1 for world light.
    pub brightness: i32,
}

/// Block displays a mod draws for blocks that aren't in the world, such as a moving contraption.
/// They are never saved; the id stops working when the display's chunk unloads. Content states
/// show as their vanilla carrier on stock clients.
#[repr(C)]
pub struct BlockDisplaysV1 {
    pub size: usize,
    /// Returns the display's id, 0 when nothing spawned.
    pub spawn:
        unsafe extern "C" fn(world: WorldRef, state: u32, display: *const BlockDisplay) -> i32,
    /// False when the id no longer names a live display; spawn a new one then.
    pub update:
        unsafe extern "C" fn(world: WorldRef, id: i32, display: *const BlockDisplay) -> bool,
    /// Replace the displayed state.
    pub set_state: unsafe extern "C" fn(world: WorldRef, id: i32, state: u32) -> bool,
    pub remove: unsafe extern "C" fn(world: WorldRef, id: i32),
}

pub const MOBS: &str = "moddedmc:mobs";

/// A base attribute value for a mod mob, such as `minecraft:max_health` 40.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct MobAttribute {
    pub id: Str,
    pub value: f64,
}

/// A mob type a mod adds. In the world it is a vanilla mob (the carrier) that remembers its mod
/// type and the mod's state; stock clients see the carrier.
#[repr(C)]
pub struct MobDefinition {
    /// `namespace:path`, unique among every mod's content.
    pub id: Str,
    /// The vanilla mob it is carried by, such as `minecraft:ravager`. Its physics, goals and
    /// look apply until the mod changes them.
    pub carrier: Str,
    /// Base values set when the mob spawns, then saved with it like any attribute.
    pub attributes: *const MobAttribute,
    pub attribute_count: usize,
}

/// Mob types for mods: register, spawn, and keep state on each mob.
#[repr(C)]
pub struct MobsV1 {
    pub size: usize,
    /// Stage a mob type during registration.
    pub add_mob: unsafe extern "C" fn(reg: Reg, def: *const MobDefinition) -> i32,
    /// Spawn a mob of a registered type, or of a vanilla mob id, at `position`. Returns its
    /// entity id, or -1.
    pub spawn: unsafe extern "C" fn(world: WorldRef, kind: Str, position: Vec3) -> i32,
    /// The registered mob type of `entity`, or empty when it isn't a mod mob. Static.
    pub kind: unsafe extern "C" fn(entity: EntityRef) -> Str,
    /// The mod's bytes saved on a mod mob, empty until set. False when it isn't a mod mob.
    pub state: unsafe extern "C" fn(entity: EntityRef, out: Sink) -> bool,
    /// Replace them, at most 64 KiB. They are saved with the mob.
    pub set_state: unsafe extern "C" fn(entity: EntityRef, state: Str) -> bool,
    // Appended after the first release: require `MobsExtension::Taming`.
    /// During registration: hear each mob of `kind` die (vanilla `die`), before its loot drops.
    /// One callback per type.
    pub on_death: unsafe extern "C" fn(reg: Reg, kind: Str, callback: MobDeathFn) -> i32,
    /// The entity's UUID (vanilla `getUUID`) into `out`.
    pub uuid: unsafe extern "C" fn(entity: EntityRef, out: *mut [u8; 16]) -> bool,
    /// The loaded entity with this UUID, or -1.
    pub by_uuid: unsafe extern "C" fn(world: WorldRef, uuid: *const [u8; 16]) -> i32,
    /// Health, and `getMaxHealth` into `max` when not null. -1 for an entity that isn't living.
    pub health: unsafe extern "C" fn(entity: EntityRef, max: *mut f32) -> f32,
    /// Vanilla `LivingEntity.heal`.
    pub heal: unsafe extern "C" fn(entity: EntityRef, amount: f32) -> bool,
    /// Entity ids of the direct passengers, the controlling one first, into `out`; returns how
    /// many there are, which can exceed `capacity`.
    pub passengers:
        unsafe extern "C" fn(entity: EntityRef, out: *mut i32, capacity: usize) -> usize,
    /// Vanilla `broadcastEntityEvent` 7 (hearts) or 6 (smoke) after a taming try.
    pub tame_effect: unsafe extern "C" fn(world: WorldRef, entity: EntityRef, tamed: bool) -> bool,
    /// Vanilla `setNoGravity`.
    pub set_no_gravity: unsafe extern "C" fn(entity: EntityRef, no_gravity: bool) -> bool,
    /// Vanilla `igniteForTicks`; 0 puts the fire out (`clearFire`).
    pub set_on_fire: unsafe extern "C" fn(entity: EntityRef, ticks: u32) -> bool,
    /// Vanilla `invulnerableTime = 0`: the next hit lands even during the hurt cooldown.
    pub reset_hurt_cooldown: unsafe extern "C" fn(entity: EntityRef) -> bool,
    /// A player's movement keys from their last input packet (`PLAYER_INPUT_*` bits), or -1
    /// for anything else.
    pub player_input: unsafe extern "C" fn(player: EntityRef) -> i32,
    /// A player's game mode (0 survival, 1 creative, 2 adventure, 3 spectator), or -1.
    pub game_mode: unsafe extern "C" fn(player: EntityRef) -> i32,
    /// Vanilla `displayClientMessage`: plain text in chat, or above the hotbar.
    pub message: unsafe extern "C" fn(player: EntityRef, text: Str, action_bar: bool) -> bool,
    /// Vanilla `Mob.setPersistenceRequired`: keep this mob from naturally despawning.
    pub make_persistent: unsafe extern "C" fn(entity: EntityRef) -> bool,
}

/// A mod mob dying; the entity is still in the world.
pub type MobDeathFn = unsafe extern "C" fn(world: WorldRef, mob: EntityRef);

/// Size of the mobs table before `on_death` was appended.
pub const MOBS_V1_BASE_SIZE: usize = std::mem::offset_of!(MobsV1, on_death);

pub const MOB_LOOT: &str = "moddedmc:mob_loot";

#[repr(C)]
pub struct MobLootV1 {
    pub size: usize,
    /// Save an empty death loot table override on a registered mob.
    pub suppress: unsafe extern "C" fn(entity: EntityRef) -> bool,
}

pub const STRUCTURE_MOBS: &str = "moddedmc:structure_mobs";

/// Initialize a registered mob from a structure's saved entity tag, before adding it to the world.
pub type StructureMobInitFn = unsafe extern "C" fn(mob: EntityRef, nbt: Str);

#[repr(C)]
pub struct StructureMobsV1 {
    pub size: usize,
    pub on_load: unsafe extern "C" fn(reg: Reg, kind: Str, callback: StructureMobInitFn) -> i32,
}

pub const MOB_STORAGE: &str = "moddedmc:mob_storage";

#[repr(C)]
pub struct MobStorageV1 {
    pub size: usize,
    /// Save a living registered mob as named-root NBT, excluding passengers.
    pub save: unsafe extern "C" fn(entity: EntityRef, out: Sink) -> bool,
    /// Restore a saved registered mob at a loaded, unoccupied position with its original UUID.
    /// Returns its new runtime entity ID, or -1 if it could not be spawned.
    pub restore: unsafe extern "C" fn(world: WorldRef, kind: Str, nbt: Str, at: Vec3) -> i32,
}

pub const FISH_BUCKETS: &str = "moddedmc:fish_buckets";

#[repr(C)]
pub struct FishBucketsV1 {
    pub size: usize,
    /// Associate a staged COD_BUCKET-carried item and a staged fish-carried mob.
    pub bind: unsafe extern "C" fn(reg: Reg, item: Str, mob: Str) -> i32,
}

pub const ARROW_AMMO: &str = "moddedmc:arrow_ammo";

/// A living target was successfully damaged by this arrow. The target is borrowed for the call.
pub type ArrowHurtFn = unsafe extern "C" fn(item: Str, target: EntityRef);

#[repr(C)]
pub struct ArrowAmmoV1 {
    pub size: usize,
    /// Configure an arrow-carried item already staged by this mod.
    pub bind: unsafe extern "C" fn(
        reg: Reg,
        item: Str,
        velocity_multiplier: f32,
        water_inertia: f64,
        after_hurt: ArrowHurtFn,
    ) -> i32,
}

pub const MOB_HEALTH: &str = "moddedmc:mob_health";

#[repr(C)]
pub struct MobHealthV1 {
    pub size: usize,
    /// Set the health of a live registered mob without issuing a damage or heal event.
    /// Only finite values above zero and at or below the mob's maximum are accepted.
    pub set_health: unsafe extern "C" fn(entity: EntityRef, health: f32) -> bool,
    pub set_attribute_base:
        unsafe extern "C" fn(entity: EntityRef, attribute: Str, value: f64) -> bool,
}

/// Vanilla `Input` bits, as the client sends them.
pub const PLAYER_INPUT_FORWARD: i32 = 1;
pub const PLAYER_INPUT_BACKWARD: i32 = 2;
pub const PLAYER_INPUT_LEFT: i32 = 4;
pub const PLAYER_INPUT_RIGHT: i32 = 8;
pub const PLAYER_INPUT_JUMP: i32 = 16;
pub const PLAYER_INPUT_SNEAK: i32 = 32;
pub const PLAYER_INPUT_SPRINT: i32 = 64;

#[cfg(test)]
mod mobs_layout {
    use super::*;
    use std::mem::offset_of;

    #[test]
    fn mobs_v1() {
        assert_eq!(offset_of!(MobsV1, add_mob), 8);
        assert_eq!(offset_of!(MobsV1, spawn), 16);
        assert_eq!(offset_of!(MobsV1, kind), 24);
        assert_eq!(offset_of!(MobsV1, state), 32);
        assert_eq!(offset_of!(MobsV1, set_state), 40);
        assert_eq!(MOBS_V1_BASE_SIZE, 48);
        assert_eq!(offset_of!(MobsV1, message), 144);
        assert_eq!(offset_of!(MobsV1, make_persistent), 152);
        assert_eq!(size_of::<MobsV1>(), 160);
        assert_eq!(size_of::<MobAttribute>(), 24);
        assert_eq!(offset_of!(MobDefinition, attributes), 32);
        assert_eq!(size_of::<MobDefinition>(), 48);
    }
}

pub const MOB_AI: &str = "moddedmc:mob_ai";

/// What a goal callback is asked: vanilla `Goal.canUse`, `canContinueToUse`, `start`, `stop`
/// and `tick`. Only the answer to `CAN_START` and `CONTINUE` is read.
pub mod goal_phase {
    pub const CAN_START: u32 = 0;
    pub const CONTINUE: u32 = 1;
    pub const START: u32 = 2;
    pub const STOP: u32 = 3;
    pub const TICK: u32 = 4;
}

/// What a running goal takes over (vanilla `Goal.Flag`); goals sharing one don't run together.
pub mod goal_control {
    pub const MOVE: u32 = 1;
    pub const LOOK: u32 = 2;
    pub const JUMP: u32 = 4;
    /// The goal picks targets and runs in the target selector.
    pub const TARGET: u32 = 8;
}

/// A goal phase for one mob; `key` is the mod's from its `GoalDefinition`. Runs every tick the
/// goal is asked, like a vanilla goal that `requiresUpdateEveryTick`.
pub type GoalCallback =
    unsafe extern "C" fn(phase: u32, key: u64, world: WorldRef, mob: EntityRef) -> bool;

/// A goal every mob of a mod mob type gets, whether spawned or loaded.
#[repr(C)]
pub struct GoalDefinition {
    /// A mod mob type, registered earlier by any mod.
    pub kind: Str,
    pub key: u64,
    /// Lower runs first, as in vanilla.
    pub priority: u32,
    /// `goal_control` bits.
    pub controls: u32,
    pub callback: GoalCallback,
}

/// AI for mod mobs: their goals, walking, looking and targets.
#[repr(C)]
pub struct MobAiV1 {
    pub size: usize,
    /// Stage a goal during registration.
    pub add_goal: unsafe extern "C" fn(reg: Reg, def: *const GoalDefinition) -> i32,
    /// During registration: mobs of `kind` drop their carrier's own goals and targets.
    pub replace_goals: unsafe extern "C" fn(reg: Reg, kind: Str) -> i32,
    /// Path to `target` at `speed` times the movement speed. False when the mob can't navigate.
    pub walk_to: unsafe extern "C" fn(mob: EntityRef, target: Vec3, speed: f64) -> bool,
    /// Drop the current path.
    pub stop: unsafe extern "C" fn(mob: EntityRef),
    /// True while the mob follows a path.
    pub navigating: unsafe extern "C" fn(mob: EntityRef) -> bool,
    /// Turn the head toward `position` this tick.
    pub look_at: unsafe extern "C" fn(mob: EntityRef, position: Vec3) -> bool,
    /// Set the attack target to the loaded entity `target`, or clear it with -1.
    pub set_target: unsafe extern "C" fn(world: WorldRef, mob: EntityRef, target: i32) -> bool,
    /// The attack target's entity id, or -1.
    pub target: unsafe extern "C" fn(mob: EntityRef) -> i32,
    // Appended after the first release: require `MobAiExtension::Combat`.
    /// During registration: `callback` hears every hit a mob of `kind` is about to take, as
    /// vanilla `hurt` starts. One callback per type.
    pub on_hurt: unsafe extern "C" fn(reg: Reg, kind: Str, callback: HurtCallback) -> i32,
    /// Vanilla `getLastHurtByMob`: the entity id of whoever last hurt the mob, or -1. Writes
    /// `getLastHurtByMobTimestamp` (the mob's tick count then) to `timestamp` when not null.
    pub last_hurt_by: unsafe extern "C" fn(mob: EntityRef, timestamp: *mut i32) -> i32,
    /// `target.hurt(damageSources().mobAttack(mob), amount)` on the loaded entity `target`. True
    /// when it took the hit.
    pub attack:
        unsafe extern "C" fn(world: WorldRef, mob: EntityRef, target: i32, amount: f32) -> bool,
}

/// A hit a mod mob is about to take: the vanilla damage type id (`minecraft:fall`) and the
/// amount before armour. False cancels it, like vanilla `hurt` returning false.
pub type HurtCallback =
    unsafe extern "C" fn(world: WorldRef, mob: EntityRef, damage_type: Str, amount: f32) -> bool;

/// Size of the mob AI table before `on_hurt` was appended.
pub const MOB_AI_V1_BASE_SIZE: usize = std::mem::offset_of!(MobAiV1, on_hurt);

#[cfg(test)]
mod mob_ai_layout {
    use super::*;
    use std::mem::offset_of;

    #[test]
    fn mob_ai_v1() {
        assert_eq!(offset_of!(MobAiV1, add_goal), 8);
        assert_eq!(offset_of!(MobAiV1, replace_goals), 16);
        assert_eq!(offset_of!(MobAiV1, walk_to), 24);
        assert_eq!(offset_of!(MobAiV1, stop), 32);
        assert_eq!(offset_of!(MobAiV1, navigating), 40);
        assert_eq!(offset_of!(MobAiV1, look_at), 48);
        assert_eq!(offset_of!(MobAiV1, set_target), 56);
        assert_eq!(offset_of!(MobAiV1, target), 64);
        assert_eq!(MOB_AI_V1_BASE_SIZE, 72);
        assert_eq!(offset_of!(MobAiV1, last_hurt_by), 80);
        assert_eq!(offset_of!(MobAiV1, attack), 88);
        assert_eq!(size_of::<MobAiV1>(), 96);
        assert_eq!(offset_of!(GoalDefinition, key), 16);
        assert_eq!(offset_of!(GoalDefinition, controls), 28);
        assert_eq!(offset_of!(GoalDefinition, callback), 32);
        assert_eq!(size_of::<GoalDefinition>(), 40);
    }
}

/// A mod-registered, save-global byte payload. Keys belong to the registering mod
/// and are saved by Pumpkin with the world, not with a chunk or entity.
pub const SAVED_DATA: &str = "moddedmc:saved_data";
/// Maximum payload size for a save-global key (8 MiB).
pub const MAX_SAVED_DATA_BYTES: usize = 8 << 20;

#[repr(C)]
pub struct SavedDataV1 {
    pub size: usize,
    /// Reserve a mod-local key in a staged registration; returns a process-local handle.
    /// The handle becomes usable only if the full registration succeeds. Returns a code.
    pub register_key: unsafe extern "C" fn(reg: Reg, name: Str, out: *mut u64) -> i32,
    /// Read bytes from the current world's save. False if the key is absent or the
    /// handle/world is invalid. Sink key 0 contains the complete borrowed value.
    pub read: unsafe extern "C" fn(world: WorldRef, key: u64, out: Sink) -> bool,
    /// Atomically replace a key in the world's in-memory save data (including empty
    /// bytes); persisted by the standard world-save lifecycle. False on invalid input.
    pub write: unsafe extern "C" fn(world: WorldRef, key: u64, value: Str) -> bool,
}

#[cfg(all(test, target_pointer_width = "64"))]
mod saved_data_layout {
    use super::*;
    use std::mem::offset_of;

    #[test]
    fn saved_data_v1() {
        assert_eq!(offset_of!(SavedDataV1, size), 0);
        assert_eq!(offset_of!(SavedDataV1, register_key), 8);
        assert_eq!(offset_of!(SavedDataV1, read), 16);
        assert_eq!(offset_of!(SavedDataV1, write), 24);
        assert_eq!(size_of::<SavedDataV1>(), 32);
    }
}

/// A claim on the pending inventory drops from one player death.
pub const PLAYER_DEATH: &str = "moddedmc:player_death";

#[repr(C)]
pub struct PlayerDeathStack {
    pub slot: u32,
    pub stack: Str,
}

#[repr(C)]
pub struct PlayerDeathContext {
    pub world: WorldRef,
    pub player: PlayerRef,
    pub player_uuid: [u8; 16],
    pub position: [f64; 3],
    pub drops: *const PlayerDeathStack,
    pub count: usize,
    /// Opaque handle for the current callback only.
    pub transaction: *mut c_void,
    /// Commit one value under a registered key owned by this mod and claim the pending drops.
    pub claim: unsafe extern "C" fn(transaction: *mut c_void, key: u64, value: Str) -> bool,
}

pub type PlayerDeathCallback = unsafe extern "C" fn(context: *const PlayerDeathContext);

#[repr(C)]
pub struct PlayerDeathV1 {
    pub size: usize,
    /// One callback per mod. A committed claim stops dispatch.
    pub on_drops: unsafe extern "C" fn(reg: Reg, callback: PlayerDeathCallback) -> i32,
}

pub const NATIVE_ENTITIES: &str = "moddedmc:native_entities";

pub mod native_entity_phase {
    pub const LOADED: u8 = 0;
    pub const TICK: u8 = 1;
    pub const USED: u8 = 2;
    pub const UNLOADED: u8 = 3;
    pub const REMOVED: u8 = 4;
}

#[repr(C)]
pub struct NativeEntitySpawn {
    pub world: WorldRef,
    pub kind: u64,
    pub uuid: [u8; 16],
    pub position: Vec3,
    pub width: f32,
    pub height: f32,
    /// Enable collision for moves made by the mod. The host applies no gravity.
    pub physics: bool,
    pub data: Str,
}

#[repr(C)]
pub struct NativeEntityContext {
    pub world: WorldRef,
    pub entity: EntityRef,
    pub player: PlayerRef,
    pub uuid: [u8; 16],
    pub phase: u8,
}

pub type NativeEntityCallback = unsafe extern "C" fn(*const NativeEntityContext) -> bool;

#[repr(C)]
pub struct NativeEntitiesV1 {
    pub size: usize,
    pub register: unsafe extern "C" fn(Reg, NativeEntityCallback, out: *mut u64) -> i32,
    pub spawn: unsafe extern "C" fn(*const NativeEntitySpawn) -> bool,
    pub data: unsafe extern "C" fn(EntityRef, Sink) -> bool,
    pub set_data: unsafe extern "C" fn(EntityRef, Str) -> bool,
}

pub const INVENTORY_MENU: &str = "moddedmc:inventory_menu";
pub const MENU_TAKE: u8 = 1;
pub const MENU_INSERT: u8 = 2;
pub const MENU_ANY_SLOT: u8 = u8::MAX;

#[repr(C)]
pub struct InventoryMenuOpen {
    pub player: PlayerRef,
    pub kind: u64,
    pub inventory: u64,
    pub title: Str,
    pub access: *const u8,
    pub count: usize,
    pub initialize_context: *mut c_void,
    /// Load the current saved stacks on the first open. Fill all `count` entries.
    pub initialize: unsafe extern "C" fn(*mut c_void, *mut Str, usize) -> bool,
}

#[repr(C)]
pub struct InventoryMenuChange {
    pub world: WorldRef,
    pub player_uuid: [u8; 16],
    pub inventory: u64,
    /// Proposed complete menu, using saved item stacks. Empty bytes indicate an empty slot.
    pub stacks: *const Str,
    pub count: usize,
    /// Stacks displaced during matching-slot restore that did not fit in the player inventory.
    pub overflow: *const Str,
    pub overflow_count: usize,
    pub transaction: *mut c_void,
    pub commit: unsafe extern "C" fn(transaction: *mut c_void, key: u64, value: Str) -> bool,
}

pub type InventoryMenuCallback = unsafe extern "C" fn(*const InventoryMenuChange);

#[repr(C)]
pub struct InventoryMenuV1 {
    pub size: usize,
    pub register: unsafe extern "C" fn(Reg, InventoryMenuCallback, out: *mut u64) -> i32,
    pub open: unsafe extern "C" fn(*const InventoryMenuOpen) -> bool,
    pub restore_matching:
        unsafe extern "C" fn(PlayerRef, u64, u64, *const InventoryMenuTarget, usize) -> bool,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct InventoryMenuTarget {
    pub menu_slot: u8,
    pub player_slot: u8,
}

#[cfg(all(test, target_pointer_width = "64"))]
#[test]
fn player_death_v1_layout() {
    use std::mem::offset_of;

    assert_eq!(size_of::<PlayerDeathV1>(), 16);
    assert_eq!(size_of::<PlayerDeathStack>(), 24);
    assert_eq!(offset_of!(PlayerDeathContext, position), 32);
    assert_eq!(offset_of!(PlayerDeathContext, drops), 56);
    assert_eq!(offset_of!(PlayerDeathContext, claim), 80);
    assert_eq!(size_of::<PlayerDeathContext>(), 88);
}

/// Register process-lifetime, independently versioned C interface tables supplied by mods.
pub const SERVICES: &str = "moddedmc:services";

#[repr(C)]
pub struct ServicesV1 {
    pub size: usize,
    /// Stage a service owned by this mod; published only if its registration succeeds.
    /// The table pointer and bytes must remain immutable until the server exits.
    pub publish:
        unsafe extern "C" fn(reg: Reg, name: Str, version: u32, table: *const c_void) -> i32,
}

#[cfg(all(test, target_pointer_width = "64"))]
mod services_layout {
    use super::*;
    use std::mem::offset_of;

    #[test]
    fn services_v1() {
        assert_eq!(offset_of!(ServicesV1, size), 0);
        assert_eq!(offset_of!(ServicesV1, publish), 8);
        assert_eq!(size_of::<ServicesV1>(), 16);
    }
}
