//! Rust SDK for moddedMC-rust mods.
//!
//! A mod is a `cdylib` that implements [`Mod`] and calls [`export_mod!`]. It talks to the server
//! only through the stable C ABI in [`abi`], so it does not contain the server and does not have
//! to be rebuilt when the server is. Package it with `scripts\pack-mod.ps1` and drop the `.mmc`
//! file into the server's `mods/` folder.
//!
//! ```ignore
//! struct Hello;
//! impl moddedmc_api::Mod for Hello {
//!     fn register(&self, r: &mut moddedmc_api::Registry<'_>) -> Result<(), moddedmc_api::RegistrationError> {
//!         r.add_command("hello", "Says hello", 0)
//!     }
//!     fn command(&self, _tag: u64, command: moddedmc_api::Command<'_>) -> i32 {
//!         command.reply("Hello!");
//!         1
//!     }
//! }
//! moddedmc_api::export_mod!(Hello);
//! ```

pub mod abi;
pub mod biomes;
pub mod blocks;
pub mod entities;
pub mod menus;
pub mod world;

use std::{
    ffi::c_void,
    marker::PhantomData,
    panic::{AssertUnwindSafe, catch_unwind},
    path::Path,
    sync::OnceLock,
};

pub use abi::{ABI_VERSION, Pos};
use abi::{HostApi, ModApi, PlayerRef, Reg, Sink, Str, code};

static HOST: OnceLock<&'static HostApi> = OnceLock::new();

fn host() -> &'static HostApi {
    HOST.get()
        .copied()
        .expect("moddedmc: host API used before the mod was loaded")
}

fn s(text: &str) -> Str {
    Str::from(text)
}

pub fn info(message: &str) {
    // SAFETY: host functions accept any borrowed string.
    unsafe { (host().log)(abi::level::INFO, s(message)) }
}
pub fn warn(message: &str) {
    // SAFETY: as above.
    unsafe { (host().log)(abi::level::WARN, s(message)) }
}
pub fn error(message: &str) {
    // SAFETY: as above.
    unsafe { (host().log)(abi::level::ERROR, s(message)) }
}

/// `<server>/config/<mod id>`, already created.
pub fn data_dir() -> &'static Path {
    // SAFETY: the host documents `data_dir` as static.
    Path::new(unsafe { host().data_dir.str() })
}

/// Server ticks since start.
pub fn tick_count() -> u64 {
    // SAFETY: no arguments.
    unsafe { (host().tick_count)() }
}

/// Loaded dimensions, e.g. `minecraft:overworld`.
pub fn dimensions() -> impl Iterator<Item = &'static str> {
    // SAFETY: the host documents dimension names as static.
    (0..)
        .map(|i| unsafe { (host().dimension)(i).str() })
        .take_while(|d| !d.is_empty())
}

/// A named interface table (e.g. [`abi::WORLD_V1`]) as `T`, if the server offers it with at least
/// the fields `T` has. Every interface table starts with its own byte size.
fn interface<T: Sync + 'static>(name: &str) -> Option<&'static T> {
    let host = host();
    if host.size <= abi::HOST_API_V1_0_SIZE {
        return None; // a 1.0 server: no interfaces
    }
    // SAFETY: the field exists (checked above); the name is borrowed for the call.
    let table = unsafe { (host.query_interface)(s(name)) };
    if table.is_null() {
        return None;
    }
    // SAFETY: interface tables live for the process and start with their size as a `usize`.
    let size = unsafe { *table.cast::<usize>() };
    // SAFETY: the server's table has at least the fields `T` declares.
    (size >= size_of::<T>()).then(|| unsafe { &*table.cast::<T>() })
}

/// Runs `call` with a sink that hands every value to `on_put` (key, bytes) as it arrives.
fn with_sink<R>(call: impl FnOnce(Sink) -> R, mut on_put: impl FnMut(u64, &[u8])) -> R {
    unsafe extern "C" fn put(ctx: *mut c_void, key: u64, value: Str) {
        // SAFETY: `ctx` is the `&mut dyn FnMut` below, live for the call; `value` is borrowed.
        unsafe { (*ctx.cast::<&mut dyn FnMut(u64, &[u8])>())(key, value.bytes()) };
    }
    let mut on_put: &mut dyn FnMut(u64, &[u8]) = &mut on_put;
    call(Sink {
        ctx: std::ptr::from_mut(&mut on_put).cast(),
        put,
    })
}

/// A block entity type. Its state is opaque bytes owned by the mod; the server saves them with
/// the block and hands them back on every call.
pub struct BlockEntityDef {
    /// Namespaced id, e.g. `demo:counter`.
    pub id: String,
    /// Bump when the state layout changes; saved next to the state.
    pub state_version: u32,
    pub initial_state: Vec<u8>,
    /// `Some(n)`: loaded instances tick every `n` ticks (n > 0). `None`: event-driven only.
    pub tick_interval: Option<u32>,
}

pub struct BlockDef {
    /// Namespaced id, e.g. `demo:counter`.
    pub id: String,
    pub display_name: String,
    /// Vanilla block the client sees: `minecraft:barrel`, `minecraft:dropper` or `minecraft:dispenser`.
    pub stand_in: String,
    /// Id of a block entity type registered by this mod.
    pub block_entity: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistrationError {
    InvalidDefinition,
    DuplicateId,
    UnknownEntityType,
    UnsupportedStandIn,
}

impl RegistrationError {
    const fn from_code(code: i32) -> Result<(), Self> {
        match code {
            code::OK => Ok(()),
            code::DUPLICATE_ID => Err(Self::DuplicateId),
            code::UNKNOWN_ENTITY_TYPE => Err(Self::UnknownEntityType),
            code::UNSUPPORTED_STAND_IN => Err(Self::UnsupportedStandIn),
            _ => Err(Self::InvalidDefinition),
        }
    }

    const fn code(self) -> i32 {
        match self {
            Self::InvalidDefinition => code::INVALID_DEFINITION,
            Self::DuplicateId => code::DUPLICATE_ID,
            Self::UnknownEntityType => code::UNKNOWN_ENTITY_TYPE,
            Self::UnsupportedStandIn => code::UNSUPPORTED_STAND_IN,
        }
    }
}

/// Open only during [`Mod::register`].
pub struct Registry<'a> {
    reg: Reg,
    // Not Send: valid only on the calling thread, during the call.
    _call: PhantomData<(&'a mut (), *const ())>,
}

impl Registry<'_> {
    /// Returns the entity type index that later calls carry.
    pub fn add_block_entity(&mut self, def: BlockEntityDef) -> Result<u32, RegistrationError> {
        let mut index = 0;
        // SAFETY: `reg` is live for this registration; strings are borrowed for the call.
        let code = unsafe {
            (host().add_block_entity)(
                self.reg,
                s(&def.id),
                def.state_version,
                Str::new(&def.initial_state),
                def.tick_interval.unwrap_or(0),
                &raw mut index,
            )
        };
        RegistrationError::from_code(code).map(|()| index)
    }

    pub fn add_block(&mut self, def: BlockDef) -> Result<(), RegistrationError> {
        // SAFETY: as above.
        RegistrationError::from_code(unsafe {
            (host().add_block)(
                self.reg,
                s(&def.id),
                s(&def.display_name),
                s(&def.stand_in),
                s(&def.block_entity),
            )
        })
    }

    /// A root command open to every player. [`Mod::command`] receives `tag` when it runs.
    pub fn add_command(
        &mut self,
        name: &str,
        description: &str,
        tag: u64,
    ) -> Result<(), RegistrationError> {
        // SAFETY: as above.
        RegistrationError::from_code(unsafe {
            (host().add_command)(self.reg, s(name), s(description), tag)
        })
    }
}

/// An online player, usable only during the call that handed it over.
#[derive(Clone, Copy)]
pub struct Player<'a> {
    raw: PlayerRef,
    // Not Send: valid only on the calling thread, during the call.
    _call: PhantomData<(&'a (), *const ())>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Position {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw: f32,
    pub pitch: f32,
}

impl<'a> Player<'a> {
    fn new(raw: PlayerRef) -> Option<Self> {
        (!raw.0.is_null()).then_some(Self {
            raw,
            _call: PhantomData,
        })
    }

    pub fn name(&self) -> &'a str {
        // SAFETY: the name lives as long as the player reference.
        unsafe { (host().player_name)(self.raw).str() }
    }

    pub fn dimension(&self) -> &'static str {
        // SAFETY: dimension names are static.
        unsafe { (host().player_dimension)(self.raw).str() }
    }

    pub fn position(&self) -> Position {
        let mut out = [0.0; 5];
        // SAFETY: `out` is the 5 doubles the host writes.
        unsafe { (host().player_position)(self.raw, &raw mut out) };
        #[expect(
            clippy::cast_possible_truncation,
            reason = "yaw and pitch are f32 on the server"
        )]
        Position {
            x: out[0],
            y: out[1],
            z: out[2],
            yaw: out[3] as f32,
            pitch: out[4] as f32,
        }
    }

    pub fn send_message(&self, text: &str) {
        // SAFETY: borrowed for the call.
        unsafe { (host().send_message)(self.raw, s(text), false) }
    }

    pub fn action_bar(&self, text: &str) {
        // SAFETY: borrowed for the call.
        unsafe { (host().send_message)(self.raw, s(text), true) }
    }

    /// Puts the item that places `block_id` in the player's hand. False if no such block.
    pub fn give_block(&self, block_id: &str) -> bool {
        // SAFETY: borrowed for the call.
        unsafe { (host().give_block)(self.raw, s(block_id)) }
    }

    /// Moves the player, across dimensions if needed. `rotation` is (yaw, pitch); `None` keeps it.
    pub fn teleport(
        &self,
        dimension: &str,
        x: f64,
        y: f64,
        z: f64,
        rotation: Option<(f32, f32)>,
    ) -> bool {
        let (yaw, pitch) = rotation.unwrap_or((f32::NAN, f32::NAN));
        // SAFETY: borrowed for the call.
        unsafe { (host().teleport)(self.raw, s(dimension), x, y, z, yaw, pitch) }
    }

    /// Shows a menu of buttons; a pick arrives in [`Mod::menu_click`] with the button's action.
    pub fn show_menu(&self, menu: &Menu) {
        let buttons: Vec<abi::MenuButton> = menu
            .buttons
            .iter()
            .map(|b| abi::MenuButton {
                label: s(&b.label),
                tooltip: s(&b.tooltip),
                action: s(&b.action),
            })
            .collect();
        // SAFETY: everything is borrowed for the call.
        unsafe {
            (host().show_menu)(
                self.raw,
                s(&menu.title),
                s(&menu.body),
                buttons.as_ptr(),
                buttons.len(),
            );
        }
    }
}

pub struct Menu {
    pub title: String,
    pub body: String,
    pub buttons: Vec<MenuButton>,
}

pub struct MenuButton {
    pub label: String,
    /// Empty for none.
    pub tooltip: String,
    /// A resource location, e.g. `mymod:go/1`.
    pub action: String,
}

/// One of this mod's blocks, for use, place and break calls.
pub struct Block<'a> {
    pub entity_type: u32,
    pub pos: Pos,
    pub dimension: &'a str,
    pub state: &'a [u8],
    /// `None` when the console placed or something else broke it.
    pub player: Option<Player<'a>>,
}

#[derive(Default)]
pub struct UseResult {
    /// New state, or `None` to keep it.
    pub state: Option<Vec<u8>>,
    /// Shown in the player's action bar.
    pub message: Option<String>,
}

/// The block entities of one dimension that are due this tick.
pub struct Tick<'a> {
    pub tick: u64,
    pub dimension: &'a str,
    entries: &'a [abi::TickEntry],
    out: &'a Sink,
}

pub struct Entry<'a> {
    pub index: usize,
    pub entity_type: u32,
    pub pos: Pos,
    pub state: &'a [u8],
}

impl<'a> Tick<'a> {
    pub fn entries(&self) -> impl Iterator<Item = Entry<'a>> + 'a {
        self.entries.iter().enumerate().map(|(index, e)| Entry {
            index,
            entity_type: e.entity_type,
            pos: e.pos,
            // SAFETY: entries borrow server memory for the call.
            state: unsafe { e.state.bytes() },
        })
    }

    /// Replaces the state of `entries()[index]`; it is saved with the block.
    pub fn set_state(&self, index: usize, state: &[u8]) {
        // SAFETY: the server copies the bytes before returning.
        unsafe { (self.out.put)(self.out.ctx, index as u64, Str::new(state)) }
    }
}

/// A command registered with [`Registry::add_command`] being run.
pub struct Command<'a> {
    /// `None` when the console ran it.
    pub player: Option<Player<'a>>,
    /// The whole command line, without the leading `/`.
    pub input: &'a str,
    source: *const c_void,
}

impl Command<'_> {
    /// Answers whoever ran the command.
    pub fn reply(&self, text: &str) {
        // SAFETY: `source` is live for this call.
        unsafe { (host().reply)(self.source, s(text)) }
    }
}

/// Implemented by a mod. Calls can arrive from several threads at once. A panic is caught,
/// logged, and treated as "did nothing".
pub trait Mod: Send + Sync + 'static {
    /// Called once, on the server's first tick.
    fn register(&self, registry: &mut Registry<'_>) -> Result<(), RegistrationError>;

    fn tick(&self, _tick: &Tick<'_>) {}

    fn use_block(&self, _block: Block<'_>) -> UseResult {
        UseResult::default()
    }

    /// Return a replacement for the initial state, or `None`.
    fn place_block(&self, _block: Block<'_>) -> Option<Vec<u8>> {
        None
    }

    /// The block is about to be removed; its state is final.
    fn break_block(&self, _block: Block<'_>) {}

    /// Returns the command's result (0 = nothing happened).
    fn command(&self, _tag: u64, _command: Command<'_>) -> i32 {
        0
    }

    /// A player picked a menu button. Every mod sees every click: check the action is yours.
    fn menu_click(&self, _player: Player<'_>, _action: &str) {}

    /// Once, before any world loads: define `blocks/1` blocks. An error disables all of them.
    fn define_blocks(
        &self,
        _blocks: &mut blocks::Definitions<'_>,
    ) -> Result<(), RegistrationError> {
        Ok(())
    }

    /// A player used, placed or broke one of this mod's `blocks/1` blocks. For a use, return
    /// true to skip the vanilla look-alike's own interaction.
    fn block_event(&self, _event: blocks::Event<'_>) -> bool {
        false
    }

    /// World generation: once per chunk, the first time it loads with this mod installed. Place
    /// ores and features with [`world`], inside chunk (`chunk_x`, `chunk_z`) only.
    fn chunk_generated(&self, _dimension: &str, _chunk_x: i32, _chunk_z: i32) {}

    /// Something happened to an entity: used, attacked, about to take damage, died. Every mod
    /// sees every one: act only on your own ids. Return true to cancel the interaction, the attack
    /// or the damage.
    fn entity_event(&self, _event: entities::Event<'_>) -> bool {
        false
    }

    /// A player closed a `menus/1` menu; `items` holds one item per slot. Every mod sees every
    /// one: act only on keys that start with your mod id.
    fn menu_closed(&self, _player: Player<'_>, _key: &str, _items: Vec<menus::Item>) {}

    /// A mob's goal selector asks about or reports on a goal added with
    /// [`entities::Entities::add_goal`]. Answer `CanStart` and `Continue` (true: yes) only for
    /// keys that start with your mod id; return false for others.
    fn goal_event(&self, _event: entities::GoalEvent<'_>) -> bool {
        false
    }
}

// ---------------------------------------------------------------- export glue

/// Panics must not unwind into the server (that aborts the process).
fn guard<R>(what: &str, fallback: R, call: impl FnOnce() -> R) -> R {
    catch_unwind(AssertUnwindSafe(call)).unwrap_or_else(|_| {
        error(&format!("panicked in {what}"));
        fallback
    })
}

/// # Safety
/// `instance` is the pointer `export` leaked for `M`.
const unsafe fn instance<'a, M>(instance: *const c_void) -> &'a M {
    // SAFETY: the caller's contract.
    unsafe { &*instance.cast::<M>() }
}

/// # Safety
/// `call` points to a live `BlockCall`.
unsafe fn block<'a>(call: *const abi::BlockCall) -> Block<'a> {
    // SAFETY: the caller's contract; everything in it is borrowed for the call.
    unsafe {
        let call = &*call;
        Block {
            entity_type: call.entity_type,
            pos: call.pos,
            dimension: call.dimension.str(),
            state: call.state.bytes(),
            player: Player::new(call.player),
        }
    }
}

unsafe extern "C" fn register<M: Mod>(inst: *const c_void, reg: Reg) -> i32 {
    guard("register", code::PANICKED, || {
        let mut registry = Registry {
            reg,
            _call: PhantomData,
        };
        // SAFETY: the server passes back our own instance.
        match unsafe { instance::<M>(inst) }.register(&mut registry) {
            Ok(()) => code::OK,
            Err(e) => e.code(),
        }
    })
}

unsafe extern "C" fn tick<M: Mod>(
    inst: *const c_void,
    tick: u64,
    dimension: Str,
    entries: *const abi::TickEntry,
    count: usize,
    out: Sink,
) {
    guard("tick", (), || {
        // SAFETY: the server passes `count` live entries and our own instance.
        let tick = unsafe {
            Tick {
                tick,
                dimension: dimension.str(),
                entries: if count == 0 {
                    &[]
                } else {
                    std::slice::from_raw_parts(entries, count)
                },
                out: &out,
            }
        };
        unsafe { instance::<M>(inst) }.tick(&tick);
    });
}

unsafe extern "C" fn use_block<M: Mod>(
    inst: *const c_void,
    call: *const abi::BlockCall,
    out: Sink,
) {
    guard("use_block", (), || {
        // SAFETY: live call and our own instance.
        let result = unsafe { instance::<M>(inst).use_block(block(call)) };
        // SAFETY: the server copies before returning.
        unsafe {
            if let Some(state) = &result.state {
                (out.put)(out.ctx, 0, Str::new(state));
            }
            if let Some(message) = &result.message {
                (out.put)(out.ctx, 1, s(message));
            }
        }
    });
}

unsafe extern "C" fn place_block<M: Mod>(
    inst: *const c_void,
    call: *const abi::BlockCall,
    out: Sink,
) {
    guard("place_block", (), || {
        // SAFETY: live call and our own instance.
        if let Some(state) = unsafe { instance::<M>(inst).place_block(block(call)) } {
            // SAFETY: the server copies before returning.
            unsafe { (out.put)(out.ctx, 0, Str::new(&state)) };
        }
    });
}

unsafe extern "C" fn break_block<M: Mod>(inst: *const c_void, call: *const abi::BlockCall) {
    // SAFETY: live call and our own instance.
    guard("break_block", (), || unsafe {
        instance::<M>(inst).break_block(block(call));
    });
}

unsafe extern "C" fn command<M: Mod>(
    inst: *const c_void,
    tag: u64,
    call: *const abi::CommandCall,
) -> i32 {
    guard("command", 0, || {
        // SAFETY: live call and our own instance.
        unsafe {
            let call = &*call;
            instance::<M>(inst).command(
                tag,
                Command {
                    player: Player::new(call.player),
                    input: call.input.str(),
                    source: call.source,
                },
            )
        }
    })
}

unsafe extern "C" fn menu_click<M: Mod>(inst: *const c_void, player: PlayerRef, action: Str) {
    guard("menu_click", (), || {
        let Some(player) = Player::new(player) else {
            return;
        };
        // SAFETY: live arguments and our own instance.
        unsafe { instance::<M>(inst).menu_click(player, action.str()) };
    });
}

unsafe extern "C" fn define_blocks<M: Mod>(inst: *const c_void, defs: abi::Defs) -> i32 {
    guard("define_blocks", code::PANICKED, || {
        let mut d = blocks::Definitions {
            defs,
            table: blocks::table(),
            _call: PhantomData,
        };
        // SAFETY: the server passes back our own instance.
        match unsafe { instance::<M>(inst) }.define_blocks(&mut d) {
            Ok(()) => code::OK,
            Err(e) => e.code(),
        }
    })
}

unsafe extern "C" fn block_event<M: Mod>(
    inst: *const c_void,
    event: *const abi::BlockEvent,
) -> bool {
    guard("block_event", false, || {
        // SAFETY: a live event borrowed for the call, and our own instance.
        unsafe {
            let e = &*event;
            let kind = match e.kind {
                abi::block_event::USE => blocks::EventKind::Use,
                abi::block_event::PLACE => blocks::EventKind::Place,
                abi::block_event::BREAK => blocks::EventKind::Break,
                abi::block_event::TICK => blocks::EventKind::Tick,
                abi::block_event::NEIGHBOR => blocks::EventKind::Neighbor,
                abi::block_event::LOAD => blocks::EventKind::Load,
                _ => return false,
            };
            instance::<M>(inst).block_event(blocks::Event {
                kind,
                dimension: e.dimension.str(),
                pos: e.pos,
                state: e.state,
                player: Player::new(e.player),
            })
        }
    })
}

unsafe extern "C" fn chunk_generated<M: Mod>(
    inst: *const c_void,
    dimension: Str,
    chunk_x: i32,
    chunk_z: i32,
) {
    guard("chunk_generated", (), || {
        // SAFETY: borrowed for the call, and our own instance.
        unsafe { instance::<M>(inst).chunk_generated(dimension.str(), chunk_x, chunk_z) };
    });
}

unsafe extern "C" fn entity_event<M: Mod>(
    inst: *const c_void,
    event: *const abi::EntityEvent,
) -> bool {
    guard("entity_event", false, || {
        // SAFETY: a live event borrowed for the call, and our own instance.
        unsafe {
            let e = &*event;
            let kind = match e.kind {
                abi::entity_event::USE => entities::EventKind::Use,
                abi::entity_event::ATTACK => entities::EventKind::Attack,
                abi::entity_event::DAMAGE => entities::EventKind::Damage {
                    amount: e.damage,
                    cause: e.cause.str(),
                },
                abi::entity_event::DEATH => entities::EventKind::Death,
                _ => return false,
            };
            instance::<M>(inst).entity_event(entities::Event {
                kind,
                dimension: e.dimension.str(),
                id: e.id,
                player: Player::new(e.player),
            })
        }
    })
}

unsafe extern "C" fn menu_closed<M: Mod>(
    inst: *const c_void,
    player: PlayerRef,
    key: Str,
    items: *const Str,
    count: usize,
) {
    guard("menu_closed", (), || {
        let Some(player) = Player::new(player) else {
            return;
        };
        // SAFETY: live arguments and our own instance; `count` borrowed items.
        unsafe {
            let items = if count == 0 || items.is_null() {
                &[]
            } else {
                std::slice::from_raw_parts(items, count)
            };
            let items = items.iter().map(|i| i.bytes().to_vec()).collect();
            instance::<M>(inst).menu_closed(player, key.str(), items);
        }
    });
}

unsafe extern "C" fn goal_event<M: Mod>(inst: *const c_void, event: *const abi::GoalEvent) -> bool {
    guard("goal_event", false, || {
        // SAFETY: a live event borrowed for the call, and our own instance.
        unsafe {
            let e = &*event;
            let phase = match e.phase {
                abi::goal_event::CAN_START => entities::GoalPhase::CanStart,
                abi::goal_event::CONTINUE => entities::GoalPhase::Continue,
                abi::goal_event::START => entities::GoalPhase::Start,
                abi::goal_event::TICK => entities::GoalPhase::Tick,
                abi::goal_event::STOP => entities::GoalPhase::Stop,
                _ => return false,
            };
            instance::<M>(inst).goal_event(entities::GoalEvent {
                phase,
                key: e.key.str(),
                dimension: e.dimension.str(),
                id: e.id,
            })
        }
    })
}

/// Used by [`export_mod!`].
#[doc(hidden)]
pub fn export<M: Mod>(
    host: &'static HostApi,
    version: &'static str,
    make: fn() -> M,
) -> *const ModApi {
    // Functions added after 1.0 are checked against `host.size` where they are used.
    if host.abi_version != ABI_VERSION || host.size < abi::HOST_API_V1_0_SIZE {
        return std::ptr::null();
    }
    let _ = HOST.set(host);
    let Some(module) = guard("startup", None, || Some(make())) else {
        return std::ptr::null();
    };
    let api = ModApi {
        abi_version: ABI_VERSION,
        size: size_of::<ModApi>(),
        version: s(version),
        instance: Box::into_raw(Box::new(module)).cast_const().cast(),
        register: register::<M>,
        tick: tick::<M>,
        use_block: use_block::<M>,
        place_block: place_block::<M>,
        break_block: break_block::<M>,
        command: command::<M>,
        menu_click: menu_click::<M>,
        define_blocks: define_blocks::<M>,
        block_event: block_event::<M>,
        chunk_generated: chunk_generated::<M>,
        entity_event: entity_event::<M>,
        menu_closed: menu_closed::<M>,
        goal_event: goal_event::<M>,
    };
    Box::into_raw(Box::new(api))
}

/// Exports a mod. The id defaults to the package name; it namespaces everything the mod adds.
///
/// ```ignore
/// moddedmc_api::export_mod!(MyMod);
/// moddedmc_api::export_mod!(MyMod::new(), id = "my_mod");
/// ```
#[macro_export]
macro_rules! export_mod {
    ($make:expr) => {
        $crate::export_mod!($make, id = env!("CARGO_PKG_NAME"));
    };
    ($make:expr, id = $id:expr) => {
        #[unsafe(no_mangle)]
        pub static MODDEDMC_ABI: u32 = $crate::abi::ABI_VERSION;

        #[unsafe(no_mangle)]
        pub static MODDEDMC_ID: $crate::abi::Str = $crate::abi::Str::new($id.as_bytes());

        #[unsafe(no_mangle)]
        pub extern "C" fn moddedmc_mod(
            host: &'static $crate::abi::HostApi,
        ) -> *const $crate::abi::ModApi {
            $crate::export(host, env!("CARGO_PKG_VERSION"), || $make)
        }
    };
}
