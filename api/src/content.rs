/// Real block registration, ordered events, and batched access to loaded worlds.
use std::{ffi::c_void, marker::PhantomData, sync::OnceLock};

use crate::{
    Command, InterfacePrefix, InterfaceUnavailable, Pos, RegistrationError, Registry, abi, guard, s,
};

pub use abi::{
    BlockUpdate, CollisionBox, ContentEvent, LOADED, MOVED_STATE_CHANGED, NEIGHBOR_CHANGED,
    STATE_CHANGED, ShapeBox, UNLOADED, UNLOADED_STATE, UPDATE_ALL, UPDATE_CLIENTS,
    UPDATE_KNOWN_SHAPE, UPDATE_MOVED, UPDATE_NEIGHBORS,
};

#[derive(Clone, Copy)]
pub struct Content(
    InterfacePrefix<abi::ContentV1>,
    InterfacePrefix<abi::BlockWorldV1>,
);

#[derive(Clone, Copy)]
pub struct World<'a> {
    raw: abi::WorldRef,
    api: InterfacePrefix<abi::BlockWorldV1>,
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
pub type RandomTick = fn(World<'_>, Pos, u32);

pub type Replaced = fn(World<'_>, &ContentEvent);

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
    /// `World::dimension_id`.
    Dimension,
    Weather,
    BestNeighborSignal,
    RawBrightness,
    BlockTransformer,
    RedstoneConductor,
    PlayerActorBlockDrops,
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
            Self::BestNeighborSignal => (
                std::mem::offset_of!(abi::BlockWorldV1, best_neighbor_signal),
                "moddedmc:block_world.best_neighbor_signal",
            ),
            Self::PlayerActorBlockDrops => (
                std::mem::offset_of!(abi::BlockWorldV1, block_drops_with_player_actor),
                "moddedmc:block_world.block_drops_with_player_actor",
            ),
            Self::RedstoneConductor => (
                std::mem::offset_of!(abi::BlockWorldV1, redstone_conductor),
                "moddedmc:block_world.redstone_conductor",
            ),
            Self::BlockTransformer => (
                std::mem::offset_of!(abi::BlockWorldV1, apply_block_transformer),
                "moddedmc:block_world.apply_block_transformer",
            ),
            Self::RawBrightness => (
                std::mem::offset_of!(abi::BlockWorldV1, raw_brightness),
                "moddedmc:block_world.raw_brightness",
            ),
            Self::Weather => (
                std::mem::offset_of!(abi::BlockWorldV1, is_thundering),
                "moddedmc:block_world.is_thundering",
            ),
            Self::Survival => (
                std::mem::offset_of!(abi::BlockWorldV1, block_item),
                "moddedmc:block_world.block_item",
            ),
            Self::Dimension => (
                std::mem::offset_of!(abi::BlockWorldV1, dimension_id),
                "moddedmc:block_world.dimension_id",
            ),
        }
    }

    const fn available(self, api: InterfacePrefix<abi::BlockWorldV1>) -> bool {
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

/// Mining, sound and map settings copied from a vanilla block.
pub struct BlockMaterial<'a> {
    pub template: &'a str,
    pub hardness: Option<f32>,
    pub map_color: Option<&'a str>,
}

static WORLD_TABLE: OnceLock<InterfacePrefix<abi::BlockWorldV1>> = OnceLock::new();
static TICK: OnceLock<Tick> = OnceLock::new();
static RANDOM_TICK: OnceLock<RandomTick> = OnceLock::new();
static REPLACED: OnceLock<Replaced> = OnceLock::new();
static PLACE: OnceLock<Place> = OnceLock::new();

pub fn require() -> Result<Content, InterfaceUnavailable> {
    // SAFETY: each table is read using its published type and version.
    let (content, world) = unsafe {
        (
            InterfacePrefix::require(abi::CONTENT, 1, abi::CONTENT_V1_BASE_SIZE)?,
            InterfacePrefix::require(abi::BLOCK_WORLD, 1, abi::BLOCK_WORLD_V1_BASE_SIZE)?,
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

unsafe extern "C" fn random_tick(raw: abi::WorldRef, pos: Pos, state: u32) {
    guard("content random tick", (), || {
        if let Some((&api, callback)) = WORLD_TABLE.get().zip(RANDOM_TICK.get()) {
            callback(
                World {
                    raw,
                    api,
                    scope: PhantomData,
                },
                pos,
                state,
            );
        }
    });
}

unsafe extern "C" fn replaced(world: abi::WorldRef, event: *const ContentEvent) {
    guard("content replacement", (), || {
        let Some((&api, callback)) = WORLD_TABLE.get().zip(REPLACED.get()) else {
            return;
        };
        // SAFETY: host borrows the world and event for the callback.
        callback(
            World {
                raw: world,
                api,
                scope: PhantomData,
            },
            unsafe { &*event },
        );
    });
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
        RegistrationError::from_code(unsafe {
            ((&raw const (*self.0.ptr).add_block).read())(reg.reg, &def)
        })
    }

    pub fn require_comparator_output(self) -> Result<(), InterfaceUnavailable> {
        if self.0.size < std::mem::offset_of!(abi::ContentV1, on_random_tick) {
            return Err(InterfaceUnavailable {
                name: "moddedmc:content.set_comparator_output",
                version: 1,
            });
        }
        Ok(())
    }

    /// Register a comparator strength (0 to 15) after adding the block with an exact state selector.
    pub fn set_comparator_output(
        self,
        reg: &mut Registry<'_>,
        selector: &str,
        strength: u8,
    ) -> Result<(), RegistrationError> {
        self.require_comparator_output()
            .map_err(|_| RegistrationError::InvalidDefinition)?;
        // SAFETY: the table includes the function and both arguments are live for the call.
        RegistrationError::from_code(unsafe {
            ((&raw const (*self.0.ptr).set_comparator_output).read())(
                reg.reg,
                s(selector),
                strength,
            )
        })
    }

    pub fn on_tick(self, reg: &mut Registry<'_>, callback: Tick) -> Result<(), RegistrationError> {
        TICK.set(callback)
            .map_err(|_| RegistrationError::DuplicateId)?;
        // SAFETY: live registration and a callback defined for the life of the mod.
        RegistrationError::from_code(unsafe {
            ((&raw const (*self.0.ptr).on_tick).read())(reg.reg, tick)
        })
    }

    pub fn require_random_tick(self) -> Result<(), InterfaceUnavailable> {
        if self.0.size < std::mem::offset_of!(abi::ContentV1, on_random_tick) + size_of::<usize>() {
            return Err(InterfaceUnavailable {
                name: "moddedmc:content.on_random_tick",
                version: 1,
            });
        }
        Ok(())
    }

    /// Enable scheduler-selected random ticks for these blocks already staged by this mod.
    /// Random ticks are delivered in order after world workers have joined.
    pub fn on_random_tick(
        self,
        reg: &mut Registry<'_>,
        blocks: &[&str],
        callback: RandomTick,
    ) -> Result<(), RegistrationError> {
        self.require_random_tick()
            .map_err(|_| RegistrationError::InvalidDefinition)?;
        RANDOM_TICK
            .set(callback)
            .map_err(|_| RegistrationError::DuplicateId)?;
        for block in blocks {
            // SAFETY: this field is size-checked and the registration and name are borrowed.
            RegistrationError::from_code(unsafe {
                ((&raw const (*self.0.ptr).on_random_tick).read())(reg.reg, s(block), random_tick)
            })?;
        }
        Ok(())
    }

    pub fn require_block_material(self) -> Result<(), InterfaceUnavailable> {
        self.0.full().map(|_| ()).ok_or(InterfaceUnavailable {
            name: "moddedmc:content.set_block_material",
            version: 1,
        })
    }

    /// Set a staged block's material independently of its physical state shapes.
    pub fn set_block_material(
        self,
        reg: &mut Registry<'_>,
        id: &str,
        material: &BlockMaterial<'_>,
    ) -> Result<(), RegistrationError> {
        let api = self.0.full().ok_or(RegistrationError::InvalidDefinition)?;
        let definition = abi::ContentMaterialDefinition {
            template: s(material.template),
            map_color: s(material.map_color.unwrap_or_default()),
            hardness: material.hardness.unwrap_or_default(),
            override_hardness: material.hardness.is_some(),
        };
        // SAFETY: the registration, definition and borrowed strings live through the call.
        RegistrationError::from_code(unsafe {
            (api.set_block_material)(reg.reg, s(id), &definition)
        })
    }

    pub fn require_replaced(self) -> Result<(), InterfaceUnavailable> {
        if self.0.size < abi::CONTENT_V1_BASE_SIZE + size_of::<abi::ContentReplaced>() {
            return Err(InterfaceUnavailable {
                name: "moddedmc:content.on_replaced",
                version: 1,
            });
        }
        Ok(())
    }

    /// Observe a replacement of one of this mod's blocks before its entity is removed.
    /// Copy any data needed later and defer world writes to the tick callback.
    pub fn on_replaced(
        self,
        reg: &mut Registry<'_>,
        callback: Replaced,
    ) -> Result<(), RegistrationError> {
        self.require_replaced()
            .map_err(|_| RegistrationError::InvalidDefinition)?;
        REPLACED
            .set(callback)
            .map_err(|_| RegistrationError::DuplicateId)?;
        // SAFETY: the size check covers the appended function and registration is live.
        RegistrationError::from_code(unsafe {
            ((&raw const (*self.0.ptr).on_replaced).read())(reg.reg, replaced)
        })
    }

    pub fn state(self, selector: &str) -> Option<u32> {
        // SAFETY: the selector is borrowed only for this call.
        let state = unsafe { ((&raw const (*self.0.ptr).state).read())(s(selector)) };
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
        let found =
            unsafe { ((&raw const (*self.1.ptr).describe_state).read())(state, sink(&mut out)) };
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
        let found =
            unsafe { ((&raw const (*self.1.ptr).block_item).read())(state, sink(&mut out)) };
        found.then(|| String::from_utf8(out).ok()).flatten()
    }

    pub fn command_world<'a>(self, command: &'a Command<'_>) -> Option<World<'a>> {
        // SAFETY: the command source remains live for the returned borrow.
        let raw = unsafe { ((&raw const (*self.1.ptr).command_world).read())(command.source) };
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
            api: *WORLD_TABLE.get()?,
            scope: PhantomData,
        })
    }

    /// The canonical namespaced dimension, including custom dimensions.
    pub fn dimension_id(self) -> Option<&'static str> {
        if !WorldExtension::Dimension.available(self.api) {
            return None;
        }
        // SAFETY: the versioned entry is present, and the host returns a static name.
        let name = unsafe { ((&raw const (*self.api.ptr).dimension_id).read())(self.raw).str() };
        (!name.is_empty()).then_some(name)
    }

    pub fn read(self, positions: &[Pos], states: &mut [u32]) -> bool {
        // SAFETY: both slices are live and disjoint; the output length is checked.
        positions.len() == states.len()
            && unsafe {
                ((&raw const (*self.api.ptr).read).read())(
                    self.raw,
                    positions.as_ptr(),
                    states.as_mut_ptr(),
                    positions.len(),
                )
            }
    }

    pub fn write(self, updates: &[BlockUpdate]) -> usize {
        // SAFETY: the slice and world handle are live.
        unsafe {
            ((&raw const (*self.api.ptr).write).read())(self.raw, updates.as_ptr(), updates.len())
        }
    }

    pub fn entity(self, pos: Pos) -> Option<Vec<u8>> {
        let mut data = Vec::new();
        let sink = abi::Sink {
            ctx: (&raw mut data).cast(),
            put: bytes,
        };
        // SAFETY: the sink collects bytes synchronously into the live Vec.
        unsafe { ((&raw const (*self.api.ptr).read_entity).read())(self.raw, pos, sink) }
            .then_some(data)
    }

    pub fn set_entity(self, pos: Pos, nbt: &[u8]) -> bool {
        // SAFETY: World and the bytes are borrowed for this call.
        unsafe {
            ((&raw const (*self.api.ptr).write_entity).read())(self.raw, pos, abi::Str::new(nbt))
        }
    }

    pub fn rotate(self, pos: Pos, axis: u8, rpm: f32) -> bool {
        // SAFETY: world is borrowed for this call.
        unsafe { ((&raw const (*self.api.ptr).rotate).read())(self.raw, pos, axis, rpm) }
    }

    /// Vanilla `Level.hasNeighborSignal`. False when unloaded or on a server without it.
    pub fn has_neighbor_signal(self, pos: Pos) -> bool {
        // SAFETY: the size check covers the appended field; world is borrowed for this call.
        WorldExtension::NeighborSignal.available(self.api)
            && unsafe { ((&raw const (*self.api.ptr).has_neighbor_signal).read())(self.raw, pos) }
    }

    /// Vanilla `Level.getBestNeighborSignal`, from 0 to 15. Gives 0 when unloaded or unavailable.
    pub fn best_neighbor_signal(self, pos: Pos) -> u8 {
        if !WorldExtension::BestNeighborSignal.available(self.api) {
            return 0;
        }
        // SAFETY: the size check covers the appended field; world is borrowed for this call.
        unsafe { ((&raw const (*self.api.ptr).best_neighbor_signal).read())(self.raw, pos) }
    }

    /// Vanilla `Level.destroyBlock(pos, drop)`. False when nothing broke or the server lacks it.
    pub fn destroy(self, pos: Pos, drop: bool) -> bool {
        // SAFETY: the size check covers the appended field; world is borrowed for this call.
        WorldExtension::Destroy.available(self.api)
            && unsafe { ((&raw const (*self.api.ptr).destroy).read())(self.raw, pos, drop) }
    }

    /// Vanilla `BlockState.isFaceSturdy`, face in down, up, north, south, west, east order.
    /// False when unloaded or on a server without it.
    pub fn face_sturdy(self, pos: Pos, face: u8) -> bool {
        // SAFETY: the size check covers the appended field; world is borrowed for this call.
        WorldExtension::FaceSturdy.available(self.api)
            && unsafe { ((&raw const (*self.api.ptr).face_sturdy).read())(self.raw, pos, face) }
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
                ((&raw const (*self.api.ptr).collision_shape).read())(
                    self.raw,
                    pos,
                    boxes.as_mut_ptr(),
                    boxes.capacity(),
                )
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
        let found = unsafe {
            ((&raw const (*self.api.ptr).fluid_id).read())(self.raw, pos, sink(&mut out))
        };
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
        unsafe {
            ((&raw const (*self.api.ptr).read_block_entity).read())(self.raw, pos, sink(&mut out))
        }
        .then_some(out)
    }

    /// Replaces the block entity at `pos` with NBT from [`World::block_entity`], after the
    /// destination block is placed. False when the saved type doesn't match the destination.
    pub fn restore_block_entity(self, pos: Pos, nbt: &[u8]) -> bool {
        // SAFETY: the size check covers the appended field; the NBT is borrowed for this call.
        WorldExtension::BlockEntities.available(self.api)
            && unsafe {
                ((&raw const (*self.api.ptr).restore_block_entity).read())(
                    self.raw,
                    pos,
                    abi::Str::new(nbt),
                )
            }
    }

    /// Use a saved tool stack on the first outline hit along a one-block ray.
    /// The ray starts at the origin block's center and ignores fluids.
    /// Requires `WorldExtension::BlockTransformer`.
    pub fn apply_block_transformer(self, origin: Pos, direction: u8, tool: &[u8]) -> bool {
        if !WorldExtension::BlockTransformer.available(self.api) {
            return false;
        }
        // SAFETY: the table includes the callback and the world and stack are borrowed.
        unsafe {
            ((&raw const (*self.api.ptr).apply_block_transformer).read())(
                self.raw,
                origin,
                direction,
                abi::Str::new(tool),
            )
        }
    }

    /// Reports the loaded state's redstone conduction, or `None` when unloaded.
    /// Requires `WorldExtension::RedstoneConductor`.
    pub fn redstone_conductor(self, pos: Pos) -> Option<bool> {
        if !WorldExtension::RedstoneConductor.available(self.api) {
            return None;
        }
        // SAFETY: the table includes the callback and the world is borrowed.
        let value =
            unsafe { ((&raw const (*self.api.ptr).redstone_conductor).read())(self.raw, pos) };
        (value >= 0).then_some(value != 0)
    }

    /// Vanilla `Block.getDrops` with `tool` (`None` for an empty hand), as saved stacks, without
    /// breaking the block or spawning them. `None` when unloaded or on a server without it.
    pub fn block_drops(self, pos: Pos, tool: Option<&[u8]>) -> Option<Vec<Vec<u8>>> {
        if !WorldExtension::BlockDrops.available(self.api) {
            return None;
        }
        // SAFETY: the size check covers the appended field.
        self.collect_block_drops(pos, tool, unsafe {
            (&raw const (*self.api.ptr).block_drops).read()
        })
    }

    /// Roll loot with an unspawned player-typed actor at the block.
    /// Requires `WorldExtension::PlayerActorBlockDrops`.
    pub fn block_drops_with_player_actor(self, pos: Pos, tool: &[u8]) -> Option<Vec<Vec<u8>>> {
        if !WorldExtension::PlayerActorBlockDrops.available(self.api) {
            return None;
        }
        // SAFETY: the size check covers the appended field.
        let callback = unsafe { (&raw const (*self.api.ptr).block_drops_with_player_actor).read() };
        self.collect_block_drops(pos, Some(tool), callback)
    }

    fn collect_block_drops(
        self,
        pos: Pos,
        tool: Option<&[u8]>,
        callback: unsafe extern "C" fn(abi::WorldRef, Pos, abi::Str, abi::Sink) -> bool,
    ) -> Option<Vec<Vec<u8>>> {
        let mut stacks: Vec<Vec<u8>> = Vec::new();
        let sink = abi::Sink {
            ctx: (&raw mut stacks).cast(),
            put: each,
        };
        let tool = abi::Str::new(tool.unwrap_or_default());
        // SAFETY: the callback borrows the world and tool, and the sink fills stacks during the call.
        unsafe { callback(self.raw, pos, tool, sink) }.then_some(stacks)
    }

    /// The overworld clock's ticks and whether this dimension has a fixed time. See
    /// [`abi::DayTime`]. `None` on a server without `WorldExtension::Survival`.
    pub fn day_time(self) -> Option<abi::DayTime> {
        if !WorldExtension::Survival.available(self.api) {
            return None;
        }
        let mut out = abi::DayTime::default();
        // SAFETY: the size check covers the appended field; `out` is live for the call.
        unsafe { ((&raw const (*self.api.ptr).day_time).read())(self.raw, &raw mut out) }
            .then_some(out)
    }

    /// Vanilla `getDestroySpeed`: the block's hardness, negative when unbreakable. `None` when
    /// unloaded.
    pub fn destroy_speed(self, pos: Pos) -> Option<f32> {
        if !WorldExtension::Survival.available(self.api) {
            return None;
        }
        let mut out = 0.0;
        // SAFETY: the size check covers the appended field; `out` is live for the call.
        unsafe { ((&raw const (*self.api.ptr).destroy_speed).read())(self.raw, pos, &raw mut out) }
            .then_some(out)
    }

    /// Vanilla `canSurvive` for `state` placed at `pos`, without placing it. `None` for an unknown
    /// state or an unloaded position.
    pub fn can_survive(self, pos: Pos, state: u32) -> Option<bool> {
        if !WorldExtension::Survival.available(self.api) {
            return None;
        }
        // SAFETY: the size check covers the appended field.
        match unsafe { ((&raw const (*self.api.ptr).can_survive).read())(self.raw, pos, state) } {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        }
    }

    /// Returns vanilla raw brightness at a loaded position.
    pub fn raw_brightness(self, pos: Pos, sky_darken: u8) -> Option<u8> {
        if !WorldExtension::RawBrightness.available(self.api) {
            return None;
        }
        // SAFETY: the size check covers the appended field and the world is borrowed.
        let light = unsafe {
            ((&raw const (*self.api.ptr).raw_brightness).read())(self.raw, pos, sky_darken)
        };
        u8::try_from(light).ok()
    }

    pub fn is_thundering(self) -> bool {
        // SAFETY: the optional field is size-checked and the world is borrowed.
        WorldExtension::Weather.available(self.api)
            && unsafe { ((&raw const (*self.api.ptr).is_thundering).read())(self.raw) }
    }

    pub fn id(self) -> usize {
        self.raw as usize
    }
}
