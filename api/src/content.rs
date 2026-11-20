/// Real block registration, ordered events, and batched access to loaded worlds.
use std::{ffi::c_void, marker::PhantomData, sync::OnceLock};

use crate::{
    Command, InterfaceUnavailable, Pos, RegistrationError, Registry, abi, guard, require_interface,
    require_interface_prefix, s,
};

pub use abi::{
    BlockUpdate, CollisionBox, ContentEvent, LOADED, MOVED_STATE_CHANGED, NEIGHBOR_CHANGED,
    STATE_CHANGED, ShapeBox, UNLOADED, UNLOADED_STATE, UPDATE_ALL, UPDATE_CLIENTS, UPDATE_MOVED,
    UPDATE_NEIGHBORS,
};

#[derive(Clone, Copy)]
pub struct Content(&'static abi::ContentV1, &'static abi::BlockWorldV1);

#[derive(Clone, Copy)]
pub struct World<'a> {
    raw: abi::WorldRef,
    api: &'static abi::BlockWorldV1,
    scope: PhantomData<&'a ()>,
}

pub struct Placement<'a> {
    pub world: World<'a>,
    pub state: u32,
    pub position: Pos,
    pub face: u8,
    pub sneak: bool,
    pub yaw: f32,
    pub pitch: f32,
}

pub type Place = fn(Placement<'_>) -> Option<u32>;
pub type Tick = fn(World<'_>, u64, &[ContentEvent]);

pub struct State<'a> {
    pub properties: &'a str,
    pub physical: &'a str,
    pub model: &'a str,
    pub rotating_model: &'a str,
    pub x: i16,
    pub y: i16,
    pub orientation: [f32; 4],
    pub shape: &'a [ShapeBox],
}

/// Block world functions added after the first ABI 1 release. Require the ones a mod uses in
/// `Mod::start`, so an older server skips the mod instead of running it without them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorldExtension {
    NeighborSignal,
    Destroy,
    FaceSturdy,
    CollisionShape,
    DescribeState,
    FluidId,
    BlockEntities,
    BlockDrops,
    /// `day_time`, `destroy_speed`, `can_survive` and `block_item`.
    Survival,
}

impl WorldExtension {
    const fn field(self) -> (usize, &'static str) {
        match self {
            Self::NeighborSignal => (
                std::mem::offset_of!(abi::BlockWorldV1, has_neighbor_signal),
                "moddedmc:block_world.has_neighbor_signal",
            ),
            Self::Destroy => (
                std::mem::offset_of!(abi::BlockWorldV1, destroy),
                "moddedmc:block_world.destroy",
            ),
            Self::FaceSturdy => (
                std::mem::offset_of!(abi::BlockWorldV1, face_sturdy),
                "moddedmc:block_world.face_sturdy",
            ),
            Self::CollisionShape => (
                std::mem::offset_of!(abi::BlockWorldV1, collision_shape),
                "moddedmc:block_world.collision_shape",
            ),
            Self::DescribeState => (
                std::mem::offset_of!(abi::BlockWorldV1, describe_state),
                "moddedmc:block_world.describe_state",
            ),
            Self::FluidId => (
                std::mem::offset_of!(abi::BlockWorldV1, fluid_id),
                "moddedmc:block_world.fluid_id",
            ),
            Self::BlockDrops => (
                std::mem::offset_of!(abi::BlockWorldV1, block_drops),
                "moddedmc:block_world.block_drops",
            ),
            Self::BlockEntities => (
                std::mem::offset_of!(abi::BlockWorldV1, restore_block_entity),
                "moddedmc:block_world.restore_block_entity",
            ),
            Self::Survival => (
                std::mem::offset_of!(abi::BlockWorldV1, block_item),
                "moddedmc:block_world.block_item",
            ),
        }
    }

    const fn available(self, api: &abi::BlockWorldV1) -> bool {
        api.size >= self.field().0 + size_of::<usize>()
    }
}

pub struct Block<'a> {
    pub id: &'a str,
    pub display_name: &'a str,
    pub entity: &'a str,
    pub item_model: &'a str,
    pub states: &'a [State<'a>],
}

static WORLD_TABLE: OnceLock<&'static abi::BlockWorldV1> = OnceLock::new();
static TICK: OnceLock<Tick> = OnceLock::new();
static PLACE: OnceLock<Place> = OnceLock::new();

pub fn require() -> Result<Content, InterfaceUnavailable> {
    // SAFETY: each table is read using its published type and version.
    let (content, world) = unsafe {
        (
            require_interface(abi::CONTENT, 1)?,
            require_interface_prefix(abi::BLOCK_WORLD, 1, abi::BLOCK_WORLD_V1_BASE_SIZE)?,
        )
    };
    let _ = WORLD_TABLE.set(world);
    Ok(Content(content, world))
}

unsafe extern "C" fn place(args: *const abi::Placement) -> u32 {
    guard("content placement", UNLOADED_STATE, || {
        let Some((&api, callback)) = WORLD_TABLE.get().zip(PLACE.get()) else {
            return UNLOADED_STATE;
        };
        // SAFETY: host passes a live placement and world for this callback.
        let args = unsafe { &*args };
        callback(Placement {
            world: World {
                raw: args.world,
                api,
                scope: PhantomData,
            },
            state: args.state,
            position: args.position,
            face: args.face,
            sneak: args.sneak,
            yaw: args.yaw,
            pitch: args.pitch,
        })
        .unwrap_or(UNLOADED_STATE)
    })
}

unsafe extern "C" fn tick(
    world: abi::WorldRef,
    time: u64,
    events: *const ContentEvent,
    count: usize,
) {
    guard("content tick", (), || {
        let Some((&api, callback)) = WORLD_TABLE.get().zip(TICK.get()) else {
            return;
        };
        // SAFETY: the event slice and world are live for this callback.
        let events = unsafe { std::slice::from_raw_parts(events, count) };
        callback(
            World {
                raw: world,
                api,
                scope: PhantomData,
            },
            time,
            events,
        );
    });
}

impl Content {
    pub fn require_extension(self, extension: WorldExtension) -> Result<(), InterfaceUnavailable> {
        if extension.available(self.1) {
            Ok(())
        } else {
            Err(InterfaceUnavailable {
                name: extension.field().1,
                version: 1,
            })
        }
    }

    pub fn add_block(
        self,
        reg: &mut Registry<'_>,
        block: &Block<'_>,
        placement: Option<Place>,
    ) -> Result<(), RegistrationError> {
        if let Some(callback) = placement {
            let registered = PLACE.get_or_init(|| callback);
            if !std::ptr::fn_addr_eq(*registered, callback) {
                return Err(RegistrationError::InvalidDefinition);
            }
        }
        let states: Vec<_> = block
            .states
            .iter()
            .map(|state| abi::StateDefinition {
                properties: s(state.properties),
                physical: s(state.physical),
                model: s(state.model),
                rotating_model: s(state.rotating_model),
                x: state.x,
                y: state.y,
                orientation: state.orientation,
                shape: state.shape.as_ptr(),
                shape_count: state.shape.len(),
            })
            .collect();
        let def = abi::ContentBlockDefinition {
            id: s(block.id),
            display_name: s(block.display_name),
            entity: s(block.entity),
            item_model: s(block.item_model),
            states: states.as_ptr(),
            count: states.len(),
            on_place: placement.map(|_| place as abi::ContentPlacement),
        };
        // SAFETY: registration and borrowed definitions are live for the call.
        RegistrationError::from_code(unsafe { (self.0.add_block)(reg.reg, &def) })
    }

    pub fn on_tick(self, reg: &mut Registry<'_>, callback: Tick) -> Result<(), RegistrationError> {
        TICK.set(callback)
            .map_err(|_| RegistrationError::DuplicateId)?;
        // SAFETY: live registration and a callback defined for the life of the mod.
        RegistrationError::from_code(unsafe { (self.0.on_tick)(reg.reg, tick) })
    }

    pub fn state(self, selector: &str) -> Option<u32> {
        // SAFETY: the selector is borrowed only for this call.
        let state = unsafe { (self.0.state)(s(selector)) };
        (state != UNLOADED_STATE).then_some(state)
    }

    /// The state's full selector with every property, the reverse of [`Content::state`].
    /// `None` for an unknown state or on a server without it.
    pub fn describe_state(self, state: u32) -> Option<String> {
        if !WorldExtension::DescribeState.available(self.1) {
            return None;
        }
        let mut out = Vec::new();
        // SAFETY: the size check covers the appended field; the sink fills `out` for this call.
        let found = unsafe { (self.1.describe_state)(state, sink(&mut out)) };
        found.then(|| String::from_utf8(out).ok()).flatten()
    }

    /// Vanilla `Block.asItem()` for the state's block, `minecraft:air` when it has none. `None`
    /// for an unknown state or on a server without `WorldExtension::Survival`.
    pub fn block_item(self, state: u32) -> Option<String> {
        if !WorldExtension::Survival.available(self.1) {
            return None;
        }
        let mut out = Vec::new();
        // SAFETY: the size check covers the appended field; the sink fills `out` for this call.
        let found = unsafe { (self.1.block_item)(state, sink(&mut out)) };
        found.then(|| String::from_utf8(out).ok()).flatten()
    }

    pub fn command_world<'a>(self, command: &'a Command<'_>) -> Option<World<'a>> {
        // SAFETY: the command source remains live for the returned borrow.
        let raw = unsafe { (self.1.command_world)(command.source) };
        (!raw.is_null()).then_some(World {
            raw,
            api: self.1,
            scope: PhantomData,
        })
    }
}

/// A sink that appends what the host writes to `out`.
fn sink(out: &mut Vec<u8>) -> abi::Sink {
    abi::Sink {
        ctx: (&raw mut *out).cast(),
        put: bytes,
    }
}

/// Collects each value the host writes as its own byte string.
unsafe extern "C" fn each(ctx: *mut c_void, _key: u64, value: abi::Str) {
    // SAFETY: ctx is the Vec the caller passed with this sink, and value is live until we return.
    unsafe { (&mut *ctx.cast::<Vec<Vec<u8>>>()).push(value.bytes().to_vec()) };
}

pub(crate) unsafe extern "C" fn bytes(ctx: *mut c_void, _key: u64, value: abi::Str) {
    // SAFETY: ctx is the Vec the caller passed with this sink, and value is live until we return.
    unsafe { (&mut *ctx.cast::<Vec<u8>>()).extend_from_slice(value.bytes()) };
}

impl World<'_> {
    pub(crate) const fn raw(&self) -> abi::WorldRef {
        self.raw
    }

    /// A world handle another interface's callback passed in, using the block-world table.
    pub(crate) fn borrowed(raw: abi::WorldRef) -> Option<Self> {
        Some(World {
            raw,
            api: WORLD_TABLE.get()?,
            scope: PhantomData,
        })
    }

    pub fn read(self, positions: &[Pos], states: &mut [u32]) -> bool {
        // SAFETY: both slices are live and disjoint; the output length is checked.
        positions.len() == states.len()
            && unsafe {
                (self.api.read)(
                    self.raw,
                    positions.as_ptr(),
                    states.as_mut_ptr(),
                    positions.len(),
                )
            }
    }

    pub fn write(self, updates: &[BlockUpdate]) -> usize {
        // SAFETY: the slice and world handle are live.
        unsafe { (self.api.write)(self.raw, updates.as_ptr(), updates.len()) }
    }

    pub fn entity(self, pos: Pos) -> Option<Vec<u8>> {
        let mut data = Vec::new();
        let sink = abi::Sink {
            ctx: (&raw mut data).cast(),
            put: bytes,
        };
        // SAFETY: the sink collects bytes synchronously into the live Vec.
        unsafe { (self.api.read_entity)(self.raw, pos, sink) }.then_some(data)
    }

    pub fn set_entity(self, pos: Pos, nbt: &[u8]) -> bool {
        // SAFETY: World and the bytes are borrowed for this call.
        unsafe { (self.api.write_entity)(self.raw, pos, abi::Str::new(nbt)) }
    }

    pub fn rotate(self, pos: Pos, axis: u8, rpm: f32) -> bool {
        // SAFETY: world is borrowed for this call.
        unsafe { (self.api.rotate)(self.raw, pos, axis, rpm) }
    }

    /// Vanilla `Level.hasNeighborSignal`. False when unloaded or on a server without it.
    pub fn has_neighbor_signal(self, pos: Pos) -> bool {
        // SAFETY: the size check covers the appended field; world is borrowed for this call.
        WorldExtension::NeighborSignal.available(self.api)
            && unsafe { (self.api.has_neighbor_signal)(self.raw, pos) }
    }

    /// Vanilla `Level.destroyBlock(pos, drop)`. False when nothing broke or the server lacks it.
    pub fn destroy(self, pos: Pos, drop: bool) -> bool {
        // SAFETY: the size check covers the appended field; world is borrowed for this call.
        WorldExtension::Destroy.available(self.api)
            && unsafe { (self.api.destroy)(self.raw, pos, drop) }
    }

    /// Vanilla `BlockState.isFaceSturdy`, face in down, up, north, south, west, east order.
    /// False when unloaded or on a server without it.
    pub fn face_sturdy(self, pos: Pos, face: u8) -> bool {
        // SAFETY: the size check covers the appended field; world is borrowed for this call.
        WorldExtension::FaceSturdy.available(self.api)
            && unsafe { (self.api.face_sturdy)(self.raw, pos, face) }
    }

    /// Vanilla `BlockState.getCollisionShape(level, pos)` as boxes relative to the block's
    /// minimum corner, empty for no collision. `None` when unloaded or on a server without it.
    pub fn collision_shape(self, pos: Pos) -> Option<Vec<CollisionBox>> {
        if !WorldExtension::CollisionShape.available(self.api) {
            return None;
        }
        let mut boxes = Vec::new();
        loop {
            // SAFETY: the size check covers the appended field; `boxes` has room for its
            // capacity and the world is borrowed for this call.
            let count = unsafe {
                (self.api.collision_shape)(self.raw, pos, boxes.as_mut_ptr(), boxes.capacity())
            };
            let count = usize::try_from(count).ok()?;
            if count <= boxes.capacity() {
                // SAFETY: the host wrote `count` boxes, within the capacity.
                unsafe { boxes.set_len(count) };
                return Some(boxes);
            }
            boxes.reserve_exact(count);
        }
    }

    /// Vanilla `FluidState.getType()` id, such as `minecraft:water` for a waterlogged block or
    /// `minecraft:empty`. `None` when unloaded or on a server without it.
    pub fn fluid_id(self, pos: Pos) -> Option<String> {
        if !WorldExtension::FluidId.available(self.api) {
            return None;
        }
        let mut out = Vec::new();
        // SAFETY: the size check covers the appended field; the sink fills `out` for this call.
        let found = unsafe { (self.api.fluid_id)(self.raw, pos, sink(&mut out)) };
        found.then(|| String::from_utf8(out).ok()).flatten()
    }

    /// Any block entity's full saved NBT, as `restore_block_entity` takes it. `None` when there is
    /// none, the position is unloaded or the server lacks `WorldExtension::BlockEntities`.
    pub fn block_entity(self, pos: Pos) -> Option<Vec<u8>> {
        if !WorldExtension::BlockEntities.available(self.api) {
            return None;
        }
        let mut out = Vec::new();
        // SAFETY: the size check covers the appended field; the sink fills `out` for this call.
        unsafe { (self.api.read_block_entity)(self.raw, pos, sink(&mut out)) }.then_some(out)
    }

    /// Replaces the block entity at `pos` with NBT from [`World::block_entity`], after the
    /// destination block is placed. False when the saved type doesn't match the destination.
    pub fn restore_block_entity(self, pos: Pos, nbt: &[u8]) -> bool {
        // SAFETY: the size check covers the appended field; the NBT is borrowed for this call.
        WorldExtension::BlockEntities.available(self.api)
            && unsafe { (self.api.restore_block_entity)(self.raw, pos, abi::Str::new(nbt)) }
    }

    /// Vanilla `Block.getDrops` with `tool` (`None` for an empty hand), as saved stacks, without
    /// breaking the block or spawning them. `None` when unloaded or on a server without it.
    pub fn block_drops(self, pos: Pos, tool: Option<&[u8]>) -> Option<Vec<Vec<u8>>> {
        if !WorldExtension::BlockDrops.available(self.api) {
            return None;
        }
        let mut stacks: Vec<Vec<u8>> = Vec::new();
        let sink = abi::Sink {
            ctx: (&raw mut stacks).cast(),
            put: each,
        };
        let tool = abi::Str::new(tool.unwrap_or_default());
        // SAFETY: the size check covers the appended field; the sink fills `stacks` for the call.
        unsafe { (self.api.block_drops)(self.raw, pos, tool, sink) }.then_some(stacks)
    }

    /// The overworld clock's ticks and whether this dimension has a fixed time. See
    /// [`abi::DayTime`]. `None` on a server without `WorldExtension::Survival`.
    pub fn day_time(self) -> Option<abi::DayTime> {
        if !WorldExtension::Survival.available(self.api) {
            return None;
        }
        let mut out = abi::DayTime::default();
        // SAFETY: the size check covers the appended field; `out` is live for the call.
        unsafe { (self.api.day_time)(self.raw, &raw mut out) }.then_some(out)
    }

    /// Vanilla `getDestroySpeed`: the block's hardness, negative when unbreakable. `None` when
    /// unloaded.
    pub fn destroy_speed(self, pos: Pos) -> Option<f32> {
        if !WorldExtension::Survival.available(self.api) {
            return None;
        }
        let mut out = 0.0;
        // SAFETY: the size check covers the appended field; `out` is live for the call.
        unsafe { (self.api.destroy_speed)(self.raw, pos, &raw mut out) }.then_some(out)
    }

    /// Vanilla `canSurvive` for `state` placed at `pos`, without placing it. `None` for an unknown
    /// state or an unloaded position.
    pub fn can_survive(self, pos: Pos, state: u32) -> Option<bool> {
        if !WorldExtension::Survival.available(self.api) {
            return None;
        }
        // SAFETY: the size check covers the appended field.
        match unsafe { (self.api.can_survive)(self.raw, pos, state) } {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        }
    }

    /// Tells loaded worlds apart. Stable while the world stays loaded.
    pub fn id(self) -> usize {
        self.raw as usize
    }
}
