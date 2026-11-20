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
        assert_eq!(offset_of!(ItemsV1, size), 0);
        assert_eq!(offset_of!(ItemsV1, add_item), 8);
        assert_eq!(offset_of!(ContentItemDefinition, carrier), 32);
        assert_eq!(offset_of!(BlockWorldV1, size), 0);
        assert_eq!(offset_of!(BlockWorldV1, read), 8);
        assert_eq!(offset_of!(BlockWorldV1, write), 16);
        assert_eq!(offset_of!(BlockWorldV1, command_world), 24);
        assert_eq!(offset_of!(BlockWorldV1, read_entity), 32);
        assert_eq!(offset_of!(BlockWorldV1, write_entity), 40);
        assert_eq!(offset_of!(BlockWorldV1, rotate), 48);
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
pub const ITEMS: &str = "moddedmc:items";
pub const UNLOADED_STATE: u32 = u32::MAX;
pub const MAX_WORLD_BATCH: usize = 4096;

pub const UPDATE_NEIGHBORS: u8 = 1;
pub const UPDATE_CLIENTS: u8 = 2;
pub const UPDATE_ALL: u8 = UPDATE_NEIGHBORS | UPDATE_CLIENTS;

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

#[repr(C)]
pub struct ContentV1 {
    pub size: usize,
    pub add_block: unsafe extern "C" fn(reg: Reg, def: *const ContentBlockDefinition) -> i32,
    /// Exact name[single=value,...] selector. Missing definitions return UNLOADED_STATE.
    pub state: unsafe extern "C" fn(selector: Str) -> u32,
    pub on_tick: unsafe extern "C" fn(reg: Reg, callback: ContentTick) -> i32,
}

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
}

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
}

/// Size of the block world table before `has_neighbor_signal` was appended.
pub const BLOCK_WORLD_V1_BASE_SIZE: usize = std::mem::offset_of!(BlockWorldV1, has_neighbor_signal);

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
}

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
}

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
}

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
        assert_eq!(size_of::<MobsV1>(), 48);
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
}

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
        assert_eq!(size_of::<MobAiV1>(), 72);
        assert_eq!(offset_of!(GoalDefinition, key), 16);
        assert_eq!(offset_of!(GoalDefinition, controls), 28);
        assert_eq!(offset_of!(GoalDefinition, callback), 32);
        assert_eq!(size_of::<GoalDefinition>(), 40);
    }
}
