use std::{
    borrow::Cow,
    cell::Cell,
    collections::{BTreeMap, HashMap, HashSet, hash_map::Entry},
    ffi::c_void,
    io,
    path::Path,
    ptr,
    sync::{Arc, LazyLock, Mutex, OnceLock, atomic::Ordering},
};

use moddedmc_api::abi::{self, BlockUpdate, ContentEvent, Pos, Reg, Sink, Str, WorldRef, code};
use pumpkin_config::AdvancedConfiguration;
use pumpkin_core::{
    block::{
        BlockBehaviour, BlockHitResult, CanPlaceAtArgs, CanUpdateAtArgs, EmitsRedstonePowerArgs,
        ExplodeArgs, GetComparatorOutputArgs, GetRedstonePowerArgs, GetStateForNeighborUpdateArgs,
        ItemInputArgs, ItemOutputArgs, NormalUseArgs, OnEntityCollisionArgs, OnNeighborUpdateArgs,
        OnPlaceArgs, PathComputationType, PlayerPlacedArgs, PlayerWillDestroyArgs, RandomTickArgs,
        UseWithItemArgs,
        blocks::{
            iron_bars::IronBarsBlock,
            ladder::LadderBlock,
            redstone::{block_receives_redstone_power, get_redstone_power},
            slabs::SlabBlock,
            stairs::StairBlock,
            walls::WallBlock,
            weathering_copper::change_over_time,
        },
        entities::{
            BlockEntity,
            content::{self as engine, Change, ContentBlockEntity},
        },
        registry::{BlockActionResult, install_content_behaviour},
    },
    command::context::CommandContext,
    entity::{
        Entity, EntityBase,
        decoration::display::ItemDisplayEntity,
        player::Player,
        projectile::arrow::{CustomArrowAmmo, install_custom_arrow_ammo},
    },
    item::items::bucket::{CustomFishBuckets, install_custom_fish_buckets},
    server::Server,
    world::{World, container_conversion, install_entity_added_hook, loot::LootContextParameters},
};
use pumpkin_data::{
    Block, BlockDirection, BlockId, BlockState, BlockStateId, Mirror, Property, Rotation,
    block_properties::SlabProperties,
    content::{self, BlockDefinition, BlockMaterial, ItemDefinition, StateDefinition},
    data_component::DataComponent,
    data_component_impl::{
        ConsumableImpl, ConsumeAnimation, CustomDataImpl, DamageImpl, DataComponentImpl,
        EquipmentSlot, IdOr, ItemModelImpl, MaxDamageImpl, MaxStackSizeImpl,
    },
    dimension::Dimension,
    entity::EntityType,
    fluid::Fluid,
    item::Item,
    item_stack::ItemStack,
    map_color::MapColor,
    sound::Sound,
    tag,
};
use pumpkin_nbt::{Nbt, compound::NbtCompound};
use pumpkin_util::{
    Hand,
    math::{position::BlockPos, vector3::Vector3},
};
use pumpkin_world::world::BlockFlags;
use uuid::Uuid;

use crate::{
    MAX_STATE_BYTES, content_pack, content_panes, content_walls, interaction, load_mods, lock,
    native,
};

const MAX_STATES_PER_BLOCK: usize = 1_024;

pub static WEATHERING_COPPER: abi::WeatheringCopperV1 = abi::WeatheringCopperV1 {
    size: size_of::<abi::WeatheringCopperV1>(),
    register: register_copper_family,
};

pub static CONTENT: abi::ContentV1 = abi::ContentV1 {
    size: size_of::<abi::ContentV1>(),
    add_block,
    state,
    on_tick,
    on_replaced,
    set_comparator_output,
    on_random_tick,
    set_block_material,
};
pub static ITEMS: abi::ItemsV1 = abi::ItemsV1 {
    size: size_of::<abi::ItemsV1>(),
    add_item,
    item_durability,
};
pub static ITEM_DEFAULTS: abi::ItemDefaultsV1 = abi::ItemDefaultsV1 {
    size: size_of::<abi::ItemDefaultsV1>(),
    add_item: add_item_defaults,
};
pub static ARROW_AMMO: abi::ArrowAmmoV1 = abi::ArrowAmmoV1 {
    size: size_of::<abi::ArrowAmmoV1>(),
    bind: bind_arrow_ammo,
};
pub static FISH_BUCKETS: abi::FishBucketsV1 = abi::FishBucketsV1 {
    size: size_of::<abi::FishBucketsV1>(),
    bind: bind_fish_bucket,
};
pub static WORLD: abi::BlockWorldV1 = abi::BlockWorldV1 {
    size: size_of::<abi::BlockWorldV1>(),
    read,
    write,
    command_world,
    read_entity,
    write_entity,
    rotate,
    has_neighbor_signal,
    destroy,
    face_sturdy,
    collision_shape,
    describe_state,
    fluid_id,
    read_block_entity,
    restore_block_entity,
    block_drops,
    day_time,
    destroy_speed,
    can_survive,
    block_item,
    dimension_id,
    is_thundering,
    best_neighbor_signal,
    raw_brightness,
    apply_block_transformer,
    redstone_conductor,
    block_drops_with_player_actor,
    chest_blocked,
    content_entity_snapshot,
    compare_content_entity,
    game_time,
    compare_replace_content_entity,
    write_with_neighbor_shapes: write,
    in_biome,
    surface_height,
    can_see_sky,
    set_weather,
    raycast,
    weather_state,
    set_weather_state,
    signal,
    update_neighbors,
};

unsafe extern "C" fn dimension_id(raw: WorldRef) -> Str {
    // SAFETY: the world handle is borrowed for the synchronous callback.
    unsafe { world(raw) }.map_or(Str::EMPTY, |world| {
        Str::from(world.dimension.minecraft_name)
    })
}

unsafe extern "C" fn is_thundering(raw: WorldRef) -> bool {
    // SAFETY: the world is borrowed for this call.
    unsafe { world(raw) }.is_some_and(World::is_thundering)
}

unsafe extern "C" fn set_weather(raw: WorldRef, mode: u8, ticks: i32) -> bool {
    // SAFETY: the world is borrowed for this call.
    let Some(world) = (unsafe { world(raw) }) else {
        return false;
    };
    if ticks <= 0 || mode > 2 {
        return false;
    }
    let (clear, rain) = if mode == 0 { (ticks, 0) } else { (0, ticks) };
    world
        .weather
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .set_weather_parameters(world, clear, rain, mode != 0, mode == 2);
    true
}

unsafe extern "C" fn weather_state(raw: WorldRef, out: *mut abi::WeatherState) -> bool {
    // SAFETY: the world is borrowed for this call.
    let Some(world) = (unsafe { world(raw) }) else {
        return false;
    };
    if out.is_null() {
        return false;
    }
    let weather = world
        .weather
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let state = abi::WeatherState {
        clear_time: weather.clear_weather_time,
        rain_time: weather.rain_time,
        thunder_time: weather.thunder_time,
        raining: weather.raining,
        thundering: weather.thundering,
        cycle_enabled: world.level_info.load().game_rules.advance_weather,
    };
    // SAFETY: caller supplies a writable output for this call.
    unsafe { out.write(state) };
    true
}

unsafe extern "C" fn set_weather_state(raw: WorldRef, state: abi::WeatherState) -> bool {
    // SAFETY: the world is borrowed for this call.
    let Some(world) = (unsafe { world(raw) }) else {
        return false;
    };
    if state.clear_time < 0 || state.rain_time < 0 || state.thunder_time < 0 {
        return false;
    }
    let mut weather = world
        .weather
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    weather.set_weather_parameters(
        world,
        state.clear_time,
        state.rain_time,
        state.raining,
        state.thundering,
    );
    weather.thunder_time = state.thunder_time;
    true
}

unsafe extern "C" fn raycast(
    raw: WorldRef,
    from: abi::Vec3,
    to: abi::Vec3,
    except: i32,
    block: *mut Pos,
    entity: *mut i32,
) -> i32 {
    use pumpkin_world::clip::{ClipBlock, ClipContext, ClipFluid};
    // SAFETY: the world is borrowed for this call.
    let Some(world) = (unsafe { world(raw) }) else {
        return -1;
    };
    let [x, y, z] = from.0;
    let [a, b, c] = to.0;
    if ![x, y, z, a, b, c]
        .iter()
        .all(|v| v.is_finite() && v.abs() <= 30_000_000.0)
    {
        return -1;
    }
    let from = Vector3::new(x, y, z);
    let to = Vector3::new(a, b, c);
    if (to - from).length() > 256.0 {
        return -1;
    }
    let hit = world.clip(&ClipContext::new(
        from,
        to,
        ClipBlock::Collider,
        ClipFluid::None,
    ));
    let end = if hit.is_miss { to } else { hit.location };
    let id = world
        .ray_trace_entities(from, end)
        .into_iter()
        .find(|(candidate, _, _)| {
            candidate.get_entity().entity_id != except && candidate.can_be_hit_by_projectile()
        })
        .map_or(-1, |(candidate, _, _)| candidate.get_entity().entity_id);
    // SAFETY: non-null output pointers are writable for the call.
    if let Some(out) = unsafe { entity.as_mut() } {
        *out = id;
    }
    if hit.is_miss {
        return 0;
    }
    // SAFETY: non-null output pointers are writable for the call.
    if let Some(out) = unsafe { block.as_mut() } {
        *out = crate::native::pos(hit.block_pos);
    }
    1
}

unsafe extern "C" fn in_biome(raw: WorldRef, pos: Pos, biomes: Str) -> i32 {
    // SAFETY: the world is borrowed for this call.
    let Some(world) = (unsafe { world(raw) }) else {
        return -1;
    };
    // SAFETY: the string is borrowed for this call.
    let biomes = unsafe { biomes.str() };
    let pos = block_pos(pos);
    if !world
        .level
        .loaded_chunks
        .contains_key(&pos.chunk_position())
    {
        return -1;
    }
    i32::from(crate::mobs::in_biomes(biomes, world.get_biome(&pos)))
}

unsafe extern "C" fn surface_height(raw: WorldRef, x: i32, z: i32, heightmap: u8) -> i32 {
    use pumpkin_world::chunk::ChunkHeightmapType;
    let kind = match heightmap {
        abi::heightmap::WORLD_SURFACE => ChunkHeightmapType::WorldSurface,
        abi::heightmap::MOTION_BLOCKING => ChunkHeightmapType::MotionBlocking,
        abi::heightmap::MOTION_BLOCKING_NO_LEAVES => ChunkHeightmapType::MotionBlockingNoLeaves,
        _ => return i32::MIN,
    };
    // SAFETY: the world is borrowed for this call.
    let Some(world) = (unsafe { world(raw) }) else {
        return i32::MIN;
    };
    let chunk = BlockPos::new(x, 0, z).chunk_position();
    if !world.level.loaded_chunks.contains_key(&chunk) {
        return i32::MIN;
    }
    // Pumpkin keeps the top block's y; vanilla answers the free y above it.
    world.get_heightmap_height(kind, x, z) + 1
}

unsafe extern "C" fn can_see_sky(raw: WorldRef, pos: Pos) -> bool {
    // SAFETY: the world is borrowed for this call.
    unsafe { world(raw) }.is_some_and(|world| world.can_see_sky(&block_pos(pos)))
}

unsafe extern "C" fn raw_brightness(raw: WorldRef, position: Pos, sky_darken: u8) -> i32 {
    // SAFETY: the host owns the borrowed world for the duration of the callback.
    let Some(world) = (unsafe { world(raw) }) else {
        return -1;
    };
    let pos = block_pos(position);
    if world.get_block_state_id_if_loaded(&pos).is_none() {
        return -1;
    }
    i32::from(world.get_raw_brightness(&pos, sky_darken))
}

const MAX_BATCH: usize = 4096;

pub struct Registration {
    id: String,
    name: String,
    entity: String,
    default_entity: bool,
    item_model: String,
    states: Vec<RegisteredState>,
    material: Option<BlockMaterial>,
    placement: Option<abi::ContentPlacement>,
    item_input: Option<abi::ItemInputFn>,
    item_output: Option<(u32, abi::ItemOutputFn)>,
    random_tick: Option<abi::ContentRandomTick>,
    physics: Option<content::BlockPhysics>,
    copper_stage: Option<usize>,
    copper_ids: Option<[String; 8]>,
}

pub struct ItemRegistration {
    pub(crate) id: String,
    name: String,
    carrier: &'static Item,
    model: String,
    use_animation: Option<u16>,
    max_damage: Option<u16>,
    components: Vec<(DataComponent, Box<dyn DataComponentImpl>)>,
    bucket_mob: Option<String>,
    arrow: Option<ArrowRegistration>,
}

struct ArrowRegistration {
    velocity_multiplier: f32,
    water_inertia: f64,
    after_hurt: abi::ArrowHurtFn,
}

pub struct RegisteredState {
    pub properties: BTreeMap<String, String>,
    comparator_output: Option<u8>,
    pub physical: &'static BlockState,
    pub(crate) shape: Vec<pumpkin_util::math::boundingbox::BoundingBox>,
    pub(crate) visual: content_pack::StateAssets,
}

struct RuntimeBlock {
    owner: usize,
    slab: Option<&'static Block>,
    stairs: bool,
    wall: bool,
    pane: bool,
    ladder: bool,
    scaffolding: bool,
    trapdoor: bool,
    weathering: bool,
    placement: Option<abi::ContentPlacement>,
    random_tick: Option<abi::ContentRandomTick>,
    item_input: Option<abi::ItemInputFn>,
    item_output: Option<(u32, abi::ItemOutputFn)>,
}

struct Runtime {
    blocks: HashMap<u16, RuntimeBlock>,
    overlays: HashMap<u16, content_pack::OverlayModel>,
    ticks: BTreeMap<usize, abi::ContentTick>,
    replaced: BTreeMap<usize, abi::ContentReplaced>,
    entity_inside: BTreeMap<usize, abi::EntityInside>,
    use_on_block: BTreeMap<usize, abi::UseOnBlockFn>,
    item_use: BTreeMap<usize, abi::ItemUseFn>,
    vanilla_use_on_block: BTreeMap<usize, abi::UseOnBlockFn>,
    use_item: BTreeMap<usize, abi::UseItemFn>,
    player_placed: BTreeMap<usize, abi::PlayerPlacedFn>,
    melee_damage: BTreeMap<usize, abi::MeleeDamageFn>,
    use_block: BTreeMap<usize, abi::UseBlockFn>,
    conversion: BTreeMap<usize, abi::ContainerConversionFn>,
    player_will_destroy: BTreeMap<usize, abi::PlayerWillDestroyFn>,
    redstone: BTreeMap<usize, (abi::RedstoneSignalFn, abi::RedstoneConnectsFn)>,
    /// Owning mod of each content item, block items included.
    item_owners: HashMap<u16, usize>,
}

/// Callbacks one mod stages during registration.
#[derive(Clone, Copy, Default)]
pub struct Hooks {
    pub tick: Option<abi::ContentTick>,
    pub replaced: Option<abi::ContentReplaced>,
    pub entity_inside: Option<abi::EntityInside>,
    pub use_on_block: Option<abi::UseOnBlockFn>,
    pub item_use: Option<abi::ItemUseFn>,
    pub vanilla_use_on_block: Option<abi::UseOnBlockFn>,
    pub use_item: Option<abi::UseItemFn>,
    pub player_placed: Option<abi::PlayerPlacedFn>,
    pub melee_damage: Option<abi::MeleeDamageFn>,
    pub use_block: Option<abi::UseBlockFn>,
    conversion: Option<abi::ContainerConversionFn>,
    pub player_will_destroy: Option<abi::PlayerWillDestroyFn>,
    pub redstone: Option<(abi::RedstoneSignalFn, abi::RedstoneConnectsFn)>,
}

#[derive(Default)]
struct Pending {
    blocks: Vec<(usize, Registration)>,
    items: Vec<(usize, ItemRegistration)>,
    ticks: BTreeMap<usize, abi::ContentTick>,
    replaced: BTreeMap<usize, abi::ContentReplaced>,
    entity_inside: BTreeMap<usize, abi::EntityInside>,
    use_on_block: BTreeMap<usize, abi::UseOnBlockFn>,
    item_use: BTreeMap<usize, abi::ItemUseFn>,
    vanilla_use_on_block: BTreeMap<usize, abi::UseOnBlockFn>,
    use_item: BTreeMap<usize, abi::UseItemFn>,
    player_placed: BTreeMap<usize, abi::PlayerPlacedFn>,
    melee_damage: BTreeMap<usize, abi::MeleeDamageFn>,
    use_block: BTreeMap<usize, abi::UseBlockFn>,
    conversion: BTreeMap<usize, abi::ContainerConversionFn>,
    player_will_destroy: BTreeMap<usize, abi::PlayerWillDestroyFn>,
    redstone: BTreeMap<usize, (abi::RedstoneSignalFn, abi::RedstoneConnectsFn)>,
}
static PENDING: Mutex<Pending> = Mutex::new(Pending {
    blocks: Vec::new(),
    items: Vec::new(),
    ticks: BTreeMap::new(),
    replaced: BTreeMap::new(),
    entity_inside: BTreeMap::new(),
    use_on_block: BTreeMap::new(),
    item_use: BTreeMap::new(),
    vanilla_use_on_block: BTreeMap::new(),
    use_item: BTreeMap::new(),
    player_placed: BTreeMap::new(),
    melee_damage: BTreeMap::new(),
    use_block: BTreeMap::new(),
    conversion: BTreeMap::new(),
    player_will_destroy: BTreeMap::new(),
    redstone: BTreeMap::new(),
});
static RUNTIME: OnceLock<Runtime> = OnceLock::new();
static WORLDS: LazyLock<Mutex<HashMap<Uuid, WorldState>>> = LazyLock::new(Mutex::default);

#[derive(Default)]
struct WorldState {
    events: Vec<(usize, ContentEvent)>,
    blocks: HashMap<BlockPos, BlockStateId>,
    displays: HashMap<BlockPos, Overlay>,
    rotation: HashMap<BlockPos, (u8, f32)>,
    animations: HashMap<BlockPos, String>,
}

struct Overlay {
    entity: Arc<ItemDisplayEntity>,
    phase: f32,
    state: BlockStateId,
    moving: bool,
    clip: String,
}

pub const fn block_pos(pos: Pos) -> BlockPos {
    BlockPos(Vector3::new(pos.x, pos.y, pos.z))
}

const fn abi_pos(pos: BlockPos) -> Pos {
    Pos {
        x: pos.0.x,
        y: pos.0.y,
        z: pos.0.z,
    }
}

fn properties(text: &str) -> Option<BTreeMap<String, String>> {
    let mut result = BTreeMap::new();
    if !text.is_empty() {
        for pair in text.split(',') {
            let (key, value) = pair.split_once('=')?;
            if key.is_empty()
                || value.is_empty()
                || result.insert(key.to_owned(), value.to_owned()).is_some()
            {
                return None;
            }
        }
    }
    Some(result)
}

fn selector(text: &str) -> Option<(&str, BTreeMap<String, String>)> {
    match text.split_once('[') {
        Some((name, props)) => Some((name, properties(props.strip_suffix(']')?)?)),
        None => Some((text, BTreeMap::new())),
    }
}

fn physical(selector_text: &str) -> Option<&'static BlockState> {
    let (name, props) = selector(selector_text)?;
    let block = Block::from_name(name.strip_prefix("minecraft:").unwrap_or(name))?;
    if block.id.as_u16() >= pumpkin_data::BlockId::COUNT {
        return None;
    }
    if props.is_empty() {
        return Some(block.default_state);
    }
    let props = props
        .iter()
        .map(|(key, value)| Property::from_str_pair(key, value))
        .collect::<Option<Vec<_>>>()?;
    Some(block.from_properties(&props).to_state_id(block).to_state())
}

unsafe fn read_shape(
    state: &abi::StateDefinition,
) -> Option<Vec<pumpkin_util::math::boundingbox::BoundingBox>> {
    if state.shape_count > 32
        || (state.shape_count > 0 && state.shape.is_null())
        || (state.orientation.iter().map(|n| n * n).sum::<f32>() - 1.0).abs() > 1e-3
        || state.orientation.iter().any(|n| !n.is_finite())
    {
        return None;
    }
    let mut shape = Vec::with_capacity(state.shape_count);
    if state.shape_count > 0 {
        // SAFETY: the definition owns the shape array for the registration call.
        for bounds in unsafe { std::slice::from_raw_parts(state.shape, state.shape_count) } {
            if (0..3).any(|axis| {
                bounds.min[axis] >= bounds.max[axis]
                    || bounds.min[axis] < -16
                    || bounds.max[axis] > 32
            }) {
                return None;
            }
            let [x1, y1, z1] = bounds.min.map(|n| f64::from(n) / 16.0);
            let [x2, y2, z2] = bounds.max.map(|n| f64::from(n) / 16.0);
            shape.push(pumpkin_util::math::boundingbox::BoundingBox::new(
                Vector3::new(x1, y1, z1),
                Vector3::new(x2, y2, z2),
            ));
        }
    }
    Some(shape)
}

unsafe extern "C" fn add_block(reg: Reg, def: *const abi::ContentBlockDefinition) -> i32 {
    // SAFETY: the host owns staging and the definition is borrowed by the registration call.
    let (staging, Some(def)) = (unsafe { (native::staging(reg), def.as_ref()) }) else {
        return code::INVALID_DEFINITION;
    };
    if RUNTIME.get().is_some()
        || def.states.is_null()
        || !(1..=MAX_STATES_PER_BLOCK).contains(&def.count)
    {
        return code::INVALID_DEFINITION;
    }
    // SAFETY: the definition and its state array are live for the call.
    let (id, name, entity, item_model, states) = unsafe {
        (
            def.id.str(),
            def.display_name.str(),
            def.entity.str(),
            def.item_model.str(),
            std::slice::from_raw_parts(def.states, def.count),
        )
    };
    if !crate::valid_id(id)
        || (!entity.is_empty() && !crate::valid_id(entity))
        || !content_pack::valid_model(item_model)
    {
        return code::INVALID_DEFINITION;
    }
    if staging.blocks.iter().any(|b| b.id == id)
        || staging.registry.block(id).is_some()
        || staging.content.iter().any(|b| b.id == id)
        || staging.content_items.iter().any(|i| i.id == id)
        || lock(&PENDING).blocks.iter().any(|(_, b)| b.id == id)
        || lock(&PENDING).items.iter().any(|(_, i)| i.id == id)
    {
        return code::DUPLICATE_ID;
    }
    let mut registered: Vec<RegisteredState> = Vec::with_capacity(states.len());
    for state in states {
        // SAFETY: the shape is borrowed from this registration entry.
        let Some(shape) = (unsafe { read_shape(state) }) else {
            return code::INVALID_DEFINITION;
        };
        // SAFETY: the string bytes belong to the live definition.
        let (props, phys, model, rotating) = unsafe {
            (
                state.properties.str(),
                state.physical.str(),
                state.model.str(),
                state.rotating_model.str(),
            )
        };
        let (Some(properties), Some(physical)) = (properties(props), physical(phys)) else {
            return code::INVALID_DEFINITION;
        };
        if !content_pack::valid_model(model)
            || !content_pack::valid_model(rotating)
            || (model.is_empty() && rotating.is_empty())
            || ![0, 90, 180, 270].contains(&state.x)
            || ![0, 90, 180, 270].contains(&state.y)
            || registered.iter().any(|s| s.properties == properties)
        {
            return code::INVALID_DEFINITION;
        }
        registered.push(RegisteredState {
            properties,
            comparator_output: None,
            physical,
            shape,
            visual: content_pack::StateAssets {
                model: model.into(),
                rotating_model: rotating.into(),
                x: state.x,
                y: state.y,
                orientation: state.orientation,
                bounded: false,
                uvlock: false,
                straight_stair_display: false,
                block_display_carrier: None,
                scaffolding: None,
                trapdoor: false,
                wall: None,
                pane: false,
            },
        });
    }
    staging.content.push(Registration {
        id: id.into(),
        name: name.into(),
        entity: if entity.is_empty() { id } else { entity }.into(),
        default_entity: entity.is_empty(),
        item_model: item_model.into(),
        states: registered,
        material: None,
        placement: def.on_place,
        random_tick: None,
        item_input: None,
        item_output: None,
        physics: None,
        copper_stage: None,
        copper_ids: None,
    });
    code::OK
}

unsafe extern "C" fn add_item_input(reg: Reg, block: Str, receive: abi::ItemInputFn) -> i32 {
    if RUNTIME.get().is_some() {
        return code::INVALID_DEFINITION;
    }
    // SAFETY: registration and the id are borrowed for this call.
    let (staging, id) = unsafe { (native::staging(reg), block.str()) };
    let Some(block) = staging.content.iter_mut().find(|entry| entry.id == id) else {
        return code::INVALID_DEFINITION;
    };
    if block.item_input.is_some() {
        return code::DUPLICATE_ID;
    }
    block.item_input = Some(receive);
    code::OK
}

unsafe extern "C" fn add_item_output(
    reg: Reg,
    block: Str,
    slots: u32,
    take: abi::ItemOutputFn,
) -> i32 {
    if reg.is_null() || RUNTIME.get().is_some() || !(1..=256).contains(&slots) {
        return code::INVALID_DEFINITION;
    }
    // SAFETY: registration and the id are borrowed for this call.
    let (staging, id) = unsafe { (native::staging(reg), block.str()) };
    let Some(block) = staging.content.iter_mut().find(|entry| entry.id == id) else {
        return code::INVALID_DEFINITION;
    };
    if block.item_output.is_some() {
        return code::DUPLICATE_ID;
    }
    block.item_output = Some((slots, take));
    code::OK
}

pub static ITEM_INPUT: abi::ItemInputV1 = abi::ItemInputV1 {
    size: size_of::<abi::ItemInputV1>(),
    add: add_item_input,
    add_output: add_item_output,
};

unsafe extern "C" fn collect_item_output(ctx: *mut c_void, _key: u64, stack: Str) {
    // SAFETY: the synchronous callback supplies a live output slot and borrowed stack.
    unsafe { *ctx.cast::<Option<ItemStack>>() = crate::stacks::read_stack(stack) };
}

unsafe extern "C" fn register_copper_family(reg: Reg, ids: *const [Str; 8]) -> i32 {
    if reg.is_null() || ids.is_null() || RUNTIME.get().is_some() {
        return code::INVALID_DEFINITION;
    }
    // SAFETY: the caller lends all eight initialized IDs and a registration handle.
    let (ids, staged) = unsafe { (&*ids, &mut native::staging(reg).content) };
    let mut names = [""; 8];
    let mut indices = [0; 8];
    for (index, id) in ids.iter().enumerate() {
        if id.len == 0 || id.len > 256 || id.ptr.is_null() {
            return code::INVALID_DEFINITION;
        }
        // SAFETY: the caller loans the name bytes for this call.
        let name = unsafe { id.str() };
        let Some(found) = staged.iter().position(|block| block.id == name) else {
            return code::INVALID_DEFINITION;
        };
        if indices[..index].contains(&found) || staged[found].copper_stage.is_some() {
            return code::DUPLICATE_ID;
        }
        names[index] = name;
        indices[index] = found;
    }
    let first = &staged[indices[0]];
    if indices.iter().any(|&i| {
        let block = &staged[i];
        !block.default_entity
            || block.random_tick.is_some()
            || block.states.len() != first.states.len()
            || !block.states.iter().all(|state| {
                first
                    .states
                    .iter()
                    .any(|other| state.properties == other.properties)
            })
    }) {
        return code::INVALID_DEFINITION;
    }
    for (stage, staged_index) in indices.into_iter().enumerate() {
        staged[staged_index].copper_stage = Some(stage);
    }
    staged[indices[0]].copper_ids = Some(names.map(str::to_owned));
    code::OK
}

unsafe extern "C" fn on_random_tick(reg: Reg, block: Str, callback: abi::ContentRandomTick) -> i32 {
    if RUNTIME.get().is_some() {
        return code::INVALID_DEFINITION;
    }
    // SAFETY: the staging handle and names are borrowed for registration.
    let (staging, name) = unsafe { (native::staging(reg), block.str()) };
    let Some(block) = staging.content.iter_mut().find(|block| block.id == name) else {
        return code::INVALID_DEFINITION;
    };
    if block.random_tick.is_some() || block.copper_stage.is_some() {
        return code::DUPLICATE_ID;
    }
    block.random_tick = Some(callback);
    code::OK
}

unsafe extern "C" fn set_comparator_output(reg: Reg, id: Str, strength: u8) -> i32 {
    if strength > 15 || RUNTIME.get().is_some() {
        return code::INVALID_DEFINITION;
    }
    // SAFETY: the host owns staging and the selector is borrowed for this call.
    let (staging, id) = unsafe { (native::staging(reg), id.str()) };
    let Some((name, props)) = selector(id) else {
        return code::INVALID_DEFINITION;
    };
    let Some(state) = staging
        .content
        .iter_mut()
        .find(|block| block.id == name)
        .and_then(|block| {
            block
                .states
                .iter_mut()
                .find(|state| state.properties == props)
        })
    else {
        return code::INVALID_DEFINITION;
    };
    if state.comparator_output.is_some() {
        return code::DUPLICATE_ID;
    }
    state.comparator_output = Some(strength);
    code::OK
}

pub static BLOCK_PHYSICS: abi::BlockPhysicsV1 = abi::BlockPhysicsV1 {
    size: size_of::<abi::BlockPhysicsV1>(),
    set: set_block_physics,
};

unsafe extern "C" fn set_block_physics(
    reg: Reg,
    block: Str,
    hardness: f32,
    blast_resistance: f32,
    tool_required: bool,
    sound_from: Str,
) -> i32 {
    if RUNTIME.get().is_some() {
        return code::INVALID_DEFINITION;
    }
    // SAFETY: all parameters are borrowed for registration.
    let (staging, name, sound_from) =
        unsafe { (native::staging(reg), block.str(), sound_from.str()) };
    let Some(sound_state) = physical(sound_from) else {
        return code::INVALID_DEFINITION;
    };
    let properties = content::BlockPhysics {
        hardness,
        blast_resistance,
        tool_required,
        sound_type: sound_state.sound_type,
    };
    if !properties.valid() {
        return code::INVALID_DEFINITION;
    }
    let Some(block) = staging.content.iter_mut().find(|b| b.id == name) else {
        return code::INVALID_DEFINITION;
    };
    if block.physics.is_some() {
        return code::DUPLICATE_ID;
    }
    block.physics = Some(properties);
    code::OK
}

unsafe extern "C" fn set_block_material(
    reg: Reg,
    id: Str,
    material: *const abi::ContentMaterialDefinition,
) -> i32 {
    if RUNTIME.get().is_some() {
        return code::INVALID_DEFINITION;
    }
    // SAFETY: the registration and material are borrowed for this call.
    let (staging, Some(material), id) = (
        unsafe { native::staging(reg) },
        unsafe { material.as_ref() },
        unsafe { id.str() },
    ) else {
        return code::INVALID_DEFINITION;
    };
    // SAFETY: both strings belong to the live material definition.
    let (template, map_color) = unsafe { (material.template.str(), material.map_color.str()) };
    let Some(template) = Block::from_name(template.strip_prefix("minecraft:").unwrap_or(template))
        .filter(|block| block.id.as_u16() < pumpkin_data::BlockId::COUNT)
    else {
        return code::INVALID_DEFINITION;
    };
    let map_color = if map_color.is_empty() {
        None
    } else {
        let Some(color) = MapColor::from_name(map_color) else {
            return code::INVALID_DEFINITION;
        };
        Some(color.id)
    };
    let hardness = material.override_hardness.then_some(material.hardness);
    if hardness.is_some_and(|value| !value.is_finite() || value < 0.0 && value != -1.0) {
        return code::INVALID_DEFINITION;
    }
    let Some(block) = staging.content.iter_mut().find(|block| block.id == id) else {
        return code::INVALID_DEFINITION;
    };
    if block.material.is_some() {
        return code::DUPLICATE_ID;
    }
    block.material = Some(BlockMaterial {
        template,
        hardness,
        map_color,
    });
    code::OK
}

unsafe extern "C" fn add_item(reg: Reg, def: *const abi::ContentItemDefinition) -> i32 {
    // SAFETY: the host owns staging and the definition is borrowed by the registration call.
    let (staging, Some(def)) = (unsafe { (native::staging(reg), def.as_ref()) }) else {
        return code::INVALID_DEFINITION;
    };
    if RUNTIME.get().is_some() {
        return code::INVALID_DEFINITION;
    }
    // SAFETY: the strings belong to the live definition.
    let (id, name, carrier, model) = unsafe {
        (
            def.id.str(),
            def.display_name.str(),
            def.carrier.str(),
            def.model.str(),
        )
    };
    let Some(carrier) = Item::from_registry_key(carrier)
        .filter(|item| item.id < Item::VANILLA_COUNT && **item != Item::AIR)
    else {
        return code::INVALID_DEFINITION;
    };
    if !crate::valid_id(id) || !content_pack::valid_model(model) {
        return code::INVALID_DEFINITION;
    }
    let pending = lock(&PENDING);
    if staging.content.iter().any(|b| b.id == id)
        || staging.content_items.iter().any(|i| i.id == id)
        || staging.registry.block(id).is_some()
        || pending.blocks.iter().any(|(_, b)| b.id == id)
        || pending.items.iter().any(|(_, i)| i.id == id)
    {
        return code::DUPLICATE_ID;
    }
    drop(pending);
    staging.content_items.push(ItemRegistration {
        id: id.into(),
        name: name.into(),
        carrier,
        model: model.into(),
        use_animation: None,
        max_damage: None,
        components: Vec::new(),
        bucket_mob: None,
        arrow: None,
    });
    code::OK
}

unsafe extern "C" fn item_durability(reg: Reg, item: Str, max_damage: u16) -> i32 {
    if RUNTIME.get().is_some() || max_damage == 0 {
        return code::INVALID_DEFINITION;
    }
    // SAFETY: the registrar and item ID are borrowed for this call.
    let (staging, item) = unsafe { (native::staging(reg), item.str()) };
    let Some(definition) = staging
        .content_items
        .iter_mut()
        .find(|definition| definition.id == item)
    else {
        return code::INVALID_DEFINITION;
    };
    if definition.max_damage.is_some()
        || definition.components.iter().any(|(id, _)| {
            matches!(
                id,
                DataComponent::MaxStackSize | DataComponent::MaxDamage | DataComponent::Damage
            )
        })
    {
        return code::DUPLICATE_ID;
    }
    definition.max_damage = Some(max_damage);
    code::OK
}

unsafe extern "C" fn bind_arrow_ammo(
    reg: Reg,
    item: Str,
    velocity_multiplier: f32,
    water_inertia: f64,
    after_hurt: abi::ArrowHurtFn,
) -> i32 {
    if !velocity_multiplier.is_finite()
        || velocity_multiplier <= 0.0
        || !(0.0..=1.0).contains(&water_inertia)
    {
        return code::INVALID_DEFINITION;
    }
    // SAFETY: the host owns staging and the item id is borrowed for this call.
    let (staging, item) = unsafe { (native::staging(reg), item.str()) };
    let Some(entry) = staging
        .content_items
        .iter_mut()
        .find(|entry| entry.id == item && entry.carrier == &Item::ARROW)
    else {
        return code::INVALID_DEFINITION;
    };
    if entry.arrow.is_some() {
        return code::DUPLICATE_ID;
    }
    entry.arrow = Some(ArrowRegistration {
        velocity_multiplier,
        water_inertia,
        after_hurt,
    });
    code::OK
}

fn install_arrow_ammo(bindings: Vec<(String, ArrowRegistration)>) -> io::Result<()> {
    if bindings.is_empty() {
        return Ok(());
    }
    let mut ammo = HashMap::new();
    for (name, definition) in bindings {
        let item = Item::from_registry_key(&name)
            .ok_or_else(|| io::Error::other("arrow ammunition missing after registration"))?;
        ammo.insert(
            item.id,
            CustomArrowAmmo {
                velocity_multiplier: definition.velocity_multiplier,
                water_inertia: definition.water_inertia,
                after_hurt: Box::new(move |target| {
                    // SAFETY: the library stays loaded and the id and target are borrowed for the call.
                    unsafe {
                        (definition.after_hurt)(
                            Str::from(item.registry_key),
                            ptr::from_ref(&target).cast(),
                        );
                    }
                }),
            },
        );
    }
    if !install_custom_arrow_ammo(ammo) {
        return Err(io::Error::other("arrow ammunition hooks already installed"));
    }
    Ok(())
}

unsafe extern "C" fn bind_fish_bucket(reg: Reg, item: Str, mob: Str) -> i32 {
    // SAFETY: the host owns staging and the identifiers are borrowed for this call.
    let (staging, item, mob) = unsafe { (native::staging(reg), item.str(), mob.str()) };
    if !crate::mobs::staged_fish(&staging.mobs, mob) {
        return code::INVALID_DEFINITION;
    }
    if staging
        .content_items
        .iter()
        .any(|entry| entry.bucket_mob.as_deref() == Some(mob))
    {
        return code::DUPLICATE_ID;
    }
    let Some(entry) = staging
        .content_items
        .iter_mut()
        .find(|entry| entry.id == item && entry.carrier == &Item::COD_BUCKET)
    else {
        return code::INVALID_DEFINITION;
    };
    if entry.bucket_mob.is_some() {
        return code::DUPLICATE_ID;
    }
    entry.bucket_mob = Some(mob.to_owned());
    code::OK
}

#[derive(Default)]
struct FishBucketRegistry {
    by_mob: HashMap<String, &'static Item>,
    by_item: HashMap<u16, String>,
}

impl CustomFishBuckets for FishBucketRegistry {
    fn bucket_for(&self, mob: &dyn EntityBase) -> Option<&'static Item> {
        self.by_mob.get(crate::mobs::registered_kind(mob)?).copied()
    }

    fn contains(&self, item: &Item) -> bool {
        self.by_item.contains_key(&item.id)
    }

    fn create(
        &self,
        item: &Item,
        world: &Arc<World>,
        position: Vector3<f64>,
    ) -> Option<Arc<dyn EntityBase>> {
        crate::mobs::create(self.by_item.get(&item.id)?, world, position)
    }
}

fn install_fish_buckets(bindings: Vec<(String, String)>) -> io::Result<()> {
    if bindings.is_empty() {
        return Ok(());
    }
    let mut buckets = FishBucketRegistry::default();
    for (item, mob) in bindings {
        let item = Item::from_registry_key(&item)
            .ok_or_else(|| io::Error::other("fish bucket missing after registration"))?;
        buckets.by_mob.insert(mob.clone(), item);
        buckets.by_item.insert(item.id, mob);
    }
    if !install_custom_fish_buckets(Box::new(buckets)) {
        return Err(io::Error::other("fish bucket hooks already installed"));
    }
    Ok(())
}

unsafe extern "C" fn add_item_defaults(
    reg: Reg,
    def: *const abi::ContentItemDefinition,
    defaults: Str,
) -> i32 {
    // SAFETY: registration and its definition remain live during the call.
    let Some(definition) = (unsafe { def.as_ref() }) else {
        return code::INVALID_DEFINITION;
    };
    let Some(stack) = crate::stacks::read_stack(defaults) else {
        return code::INVALID_DEFINITION;
    };
    // SAFETY: the carrier string belongs to the live definition.
    let carrier = Item::from_registry_key(unsafe { definition.carrier.str() });
    if stack.item_count != 1 || carrier != Some(stack.item) {
        return code::INVALID_DEFINITION;
    }
    let Some(components) = stack
        .patch
        .into_iter()
        .map(|(id, value)| Some((id, value?)))
        .collect::<Option<Vec<_>>>()
    else {
        return code::INVALID_DEFINITION;
    };
    for (index, (id, _)) in components.iter().enumerate() {
        if *id == DataComponent::ItemModel
            || components[..index]
                .iter()
                .any(|(previous, _)| previous == id)
        {
            return code::INVALID_DEFINITION;
        }
    }
    // SAFETY: the same borrowed definition and registration are still live.
    let result = unsafe { add_item(reg, def) };
    if result != code::OK {
        return result;
    }
    // SAFETY: add_item successfully staged the definition on this handle.
    if let Some(entry) = unsafe { native::staging(reg) }.content_items.last_mut() {
        entry.components = components;
    }
    code::OK
}

unsafe extern "C" fn on_tick(reg: Reg, callback: abi::ContentTick) -> i32 {
    // SAFETY: the staging handle is valid only during registration.
    if RUNTIME.get().is_some() {
        return code::INVALID_DEFINITION;
    }
    // SAFETY: registration is live for the call.
    let slot = &mut unsafe { native::staging(reg) }.content_hooks.tick;
    if slot.is_some() {
        return code::DUPLICATE_ID;
    }
    *slot = Some(callback);
    code::OK
}

unsafe extern "C" fn on_replaced(reg: Reg, callback: abi::ContentReplaced) -> i32 {
    if RUNTIME.get().is_some() {
        return code::INVALID_DEFINITION;
    }
    // SAFETY: registration is live for the call.
    let slot = &mut unsafe { native::staging(reg) }.content_hooks.replaced;
    if slot.is_some() {
        return code::DUPLICATE_ID;
    }
    *slot = Some(callback);
    code::OK
}

unsafe extern "C" fn state(text: Str) -> u32 {
    // SAFETY: the selector is borrowed for this call.
    let text = unsafe { text.str() };
    let Some((id, props)) = selector(text) else {
        return abi::UNLOADED_STATE;
    };
    let name = id.strip_prefix("minecraft:").unwrap_or(id);
    let Some(block) = Block::from_name(name) else {
        return abi::UNLOADED_STATE;
    };
    let state = if props.is_empty() {
        Some(block.default_state.id)
    } else if content::block(name).is_some() {
        content::state_from_properties(block, props.iter().map(|(k, v)| (k.as_str(), v.as_str())))
    } else {
        physical(text).map(|state| state.id)
    };
    state.map_or(abi::UNLOADED_STATE, |state| u32::from(state.as_u16()))
}

pub fn register(
    owner: usize,
    definitions: Vec<Registration>,
    items: Vec<ItemRegistration>,
    hooks: Hooks,
) {
    let mut pending = lock(&PENDING);
    pending
        .blocks
        .extend(definitions.into_iter().map(|def| (owner, def)));
    pending
        .items
        .extend(items.into_iter().map(|item| (owner, item)));
    if let Some(tick) = hooks.tick {
        pending.ticks.insert(owner, tick);
    }
    if let Some(callback) = hooks.replaced {
        pending.replaced.insert(owner, callback);
    }
    if let Some(inside) = hooks.entity_inside {
        pending.entity_inside.insert(owner, inside);
    }
    if let Some(callback) = hooks.melee_damage {
        pending.melee_damage.insert(owner, callback);
    }
    if let Some(callback) = hooks.player_placed {
        pending.player_placed.insert(owner, callback);
    }
    if let Some(callback) = hooks.use_item {
        pending.use_item.insert(owner, callback);
    }
    if let Some(callback) = hooks.use_on_block {
        pending.use_on_block.insert(owner, callback);
    }
    if let Some(callback) = hooks.item_use {
        pending.item_use.insert(owner, callback);
    }
    if let Some(callback) = hooks.vanilla_use_on_block {
        pending.vanilla_use_on_block.insert(owner, callback);
    }
    if let Some(callback) = hooks.use_block {
        pending.use_block.insert(owner, callback);
    }
    if let Some(callback) = hooks.conversion {
        pending.conversion.insert(owner, callback);
    }
    if let Some(callback) = hooks.player_will_destroy {
        pending.player_will_destroy.insert(owner, callback);
    }
    if let Some(callbacks) = hooks.redstone {
        pending.redstone.insert(owner, callbacks);
    }
}

fn split_items(
    items: Vec<(usize, ItemRegistration)>,
) -> (Vec<ItemDefinition>, Vec<content_pack::ItemAssets>) {
    items
        .into_iter()
        .map(|(_, item)| {
            (
                ItemDefinition {
                    name: item.id.clone(),
                    display_name: item.name,
                    carrier: item.carrier,
                    components: {
                        let mut components = item.components;
                        if let Some(max_damage) = item.max_damage {
                            components.extend([
                                (
                                    DataComponent::MaxStackSize,
                                    Box::new(MaxStackSizeImpl { size: 1 })
                                        as Box<dyn DataComponentImpl>,
                                ),
                                (
                                    DataComponent::MaxDamage,
                                    Box::new(MaxDamageImpl {
                                        max_damage: i32::from(max_damage),
                                    }),
                                ),
                                (DataComponent::Damage, Box::new(DamageImpl { damage: 0 })),
                            ]);
                        }
                        components
                    },
                    client_components: item.use_animation.map_or_else(Vec::new, |ticks| {
                        vec![(
                            DataComponent::Consumable,
                            Box::new(ConsumableImpl::new(
                                f32::from(ticks) / 20.0,
                                ConsumeAnimation::Eat,
                                IdOr::Id(Sound::ItemBrushBrushingGeneric),
                                false,
                                Cow::Borrowed(&[]),
                            )) as Box<dyn DataComponentImpl>,
                        )]
                    }),
                },
                content_pack::ItemAssets {
                    id: item.id,
                    model: item.model,
                },
            )
        })
        .unzip()
}

fn reserve_variants(
    block: &'static Block,
    property: &'static str,
    values: &'static [&'static str],
    replacement: &'static str,
) -> impl Iterator<Item = (BlockStateId, BlockStateId)> {
    block.states.iter().filter_map(move |state| {
        let mut props = block.properties(state.id)?.to_props();
        let variant = props.iter_mut().find(|prop| prop.key_str() == property)?;
        if !values.contains(&variant.value_str()) {
            return None;
        }
        *variant = Property::from_str_pair(property, replacement)?;
        let replacement = block.from_properties(&props).to_state_id(block);
        Some((state.id, replacement))
    })
}

fn reserve_dry_leaves() -> impl Iterator<Item = (BlockStateId, BlockStateId)> {
    tag::Block::MINECRAFT_LEAVES
        .0
        .iter()
        .filter_map(|name| Block::from_name(name))
        .flat_map(|block| {
            reserve_variants(block, "distance", &["1", "2", "3", "4", "5", "6"], "7").filter(
                move |(state, _)| {
                    block.properties(*state).is_some_and(|properties| {
                        properties.to_props().iter().any(|property| {
                            property.key_str() == "waterlogged" && property.value_str() == "false"
                        })
                    })
                },
            )
        })
}

/// Engine definitions for the staged blocks. States with the same model and rotation share a
/// carrier, since the carrier only decides what a stock client draws.
#[expect(
    clippy::too_many_lines,
    reason = "allocate stock carriers alongside physical states"
)]
fn block_definitions(blocks: &mut [(usize, Registration)]) -> io::Result<Vec<BlockDefinition>> {
    // Keep each note and instrument and each hive's facing and visible honey.
    let notes = reserve_variants(&Block::NOTE_BLOCK, "powered", &["true"], "false");
    let target = Block::TARGET.default_state.id;
    let targets = Block::TARGET
        .states
        .iter()
        .filter_map(|state| (state.id != target).then_some((state.id, target)));
    let hives = [&Block::BEEHIVE, &Block::BEE_NEST]
        .into_iter()
        .flat_map(|block| reserve_variants(block, "honey_level", &["1", "2", "3", "4"], "0"));
    let mut carriers = notes.chain(targets).chain(hives);
    let mut dry_leaves = reserve_dry_leaves();
    let mut pane_carriers = content_panes::CARRIERS.into_iter();
    let mut ladder_carriers = crate::content_ladders::CARRIERS.into_iter();
    let mut scaffolding_carriers = crate::content_ladders::CARRIERS.into_iter();
    let mut trapdoor_carriers = [false, true].into_iter();
    let mut slab_carriers = crate::content_slab::CARRIERS.into_iter();
    let mut straight_carriers = crate::content_stairs::STRAIGHT_CARRIERS.into_iter();
    let mut corner_carriers = crate::content_stairs::corner_carriers();
    let mut assigned = HashMap::new();
    let mut definitions = Vec::with_capacity(blocks.len());
    let mut order: Vec<_> = (0..blocks.len()).collect();
    // Keep the assigned carriers for existing content. The roof cubes can supply
    // their carrier to a double slab with the same model.
    order.sort_by_key(|&index| {
        let block = &blocks[index].1;
        (
            block.copper_stage.is_some(),
            block.copper_stage.is_some() && block.states.len() > 1,
        )
    });
    for index in order {
        let block = &mut blocks[index].1;
        let ladder_carrier = if crate::content_ladders::has_ladder_shape(&block.states) {
            Some(
                ladder_carriers
                    .next()
                    .ok_or_else(|| io::Error::other("custom ladder carrier slots exhausted"))?,
            )
        } else {
            None
        };
        let scaffold_carrier =
            if crate::content_scaffolding::has_scaffolding_shape(&block.states) {
                Some(scaffolding_carriers.next().ok_or_else(|| {
                    io::Error::other("custom scaffolding carrier slots exhausted")
                })?)
            } else {
                None
            };
        let trapdoor_carrier = if crate::content_trapdoors::has_trapdoor_shape(&block.states) {
            Some(
                trapdoor_carriers
                    .next()
                    .ok_or_else(|| io::Error::other("custom trapdoor carrier slots exhausted"))?,
            )
        } else {
            None
        };
        let slab = crate::content_slab::template(&block.states);
        let slab_carrier = slab.and_then(|_| slab_carriers.next());
        let pane = content_panes::has_pane_shape(&block.states);
        let pane_carrier = (pane && block.states[0].visual.rotating_model.is_empty())
            .then(|| pane_carriers.next())
            .flatten();
        let wall_overlay = content_walls::has_wall_overlay(&block.states);
        let stairs = crate::content_stairs::has_stair_shape(&block.states)
            && block
                .states
                .iter()
                .all(|state| state.visual.rotating_model.is_empty());
        let straights = stairs.then(|| straight_carriers.next()).flatten();
        let corners = (stairs && crate::content_stairs::corners_share_models(&block.states))
            .then(|| corner_carriers.next())
            .flatten();
        let mut states = Vec::with_capacity(block.states.len());
        for state in &mut block.states {
            let visual = &mut state.visual;
            // A display-only state draws the empty model, a state with neither draws a note block.
            let look = (
                visual.model.clone(),
                visual.rotating_model.is_empty(),
                visual.x.rem_euclid(360),
                visual.y.rem_euclid(360),
            );
            let (carrier, replacement) = if let Some(carrier) = ladder_carrier {
                crate::content_ladders::allocate(state.physical.id, carrier)
            } else if let Some(carrier) = scaffold_carrier {
                let (carrier, replacement, bottom) =
                    crate::content_scaffolding::allocate(state.physical.id, carrier);
                visual.scaffolding = Some(bottom);
                (carrier, replacement)
            } else if let Some(carrier) = trapdoor_carrier {
                visual.trapdoor = true;
                crate::content_trapdoors::allocate(state.physical.id, carrier)
            } else if pane {
                visual.pane = true;
                content_panes::allocate(state.physical.id, pane_carrier)
            } else if wall_overlay {
                // The stock wall keeps the client's collision and fluid state.
                visual.wall = Some(content_walls::WallSelector::from_state(state.physical.id));
                (state.physical.id, state.physical.id)
            } else if stairs {
                crate::content_stairs::allocate(state, corners, straights)
            } else if slab.is_some() {
                let props = SlabProperties::from_state_id(state.physical.id);
                let shared_cube = (props.r#type
                    == pumpkin_data::block_properties::SlabType::Double
                    && !props.waterlogged)
                    .then(|| assigned.get(&look).copied())
                    .flatten();
                shared_cube.unwrap_or_else(|| {
                    let (carrier, replacement) =
                        slab_carrier.unwrap_or(crate::content_slab::DISPLAY_CARRIER);
                    if slab_carrier.is_none() {
                        visual.rotating_model = std::mem::take(&mut visual.model);
                        visual.bounded = true;
                    }
                    (props.to_state_id(carrier), props.to_state_id(replacement))
                })
            } else {
                match assigned.entry(look) {
                    Entry::Occupied(entry) => *entry.get(),
                    Entry::Vacant(entry) => {
                        // Keep leaf carriers for opaque cubes with no mod-specific block entity.
                        let carrier = if block.default_entity
                            && visual.rotating_model.is_empty()
                            && state.shape.is_empty()
                            && state.physical.is_solid_render()
                        {
                            dry_leaves.next().or_else(|| carriers.next())
                        } else {
                            carriers.next()
                        };
                        *entry.insert(carrier.ok_or_else(|| {
                            io::Error::other("custom block carrier slots exhausted")
                        })?)
                    }
                }
            };
            states.push(StateDefinition {
                properties: state.properties.clone(),
                physical: state.physical,
                random_ticks: block.random_tick.is_some()
                    || matches!(block.copper_stage, Some(0..=2)),
                carrier,
                carrier_replacement: (carrier != replacement).then_some(replacement),
                shape: state.shape.clone(),
            });
        }
        definitions.push(BlockDefinition {
            name: block.id.clone(),
            display_name: block.name.clone(),
            entity: Some(block.entity.clone()),
            item_carrier: &Item::PAPER,
            material: block.material.take(),
            states,
        });
    }
    Ok(definitions)
}

fn open_registry(
    path: &Path,
    blocks: &mut [(usize, Registration)],
    items: &[ItemDefinition],
) -> io::Result<content::Registry> {
    let definitions = block_definitions(blocks)?;
    let mut registry = content::Registry::open_with_properties(
        &path.join("moddedmc/registry.dat"),
        &definitions,
        items,
        |name, state_index| {
            let index = blocks
                .binary_search_by(|(_, block)| block.id.as_str().cmp(name))
                .ok()?;
            blocks[index].1.states[state_index].comparator_output
        },
        |name| {
            let index = blocks
                .binary_search_by(|(_, block)| block.id.as_str().cmp(name))
                .ok()?;
            blocks[index].1.physics
        },
    )?;
    for (_, block) in blocks.iter() {
        if let Some(ids) = &block.copper_ids {
            registry.add_copper_family(ids.each_ref().map(String::as_str))?;
        }
    }
    crate::content_ladders::reserve_carriers(&mut registry, &definitions)?;
    assign_block_display_carriers(blocks, &definitions, &mut registry)?;
    Ok(registry)
}

fn assign_block_display_carriers(
    blocks: &mut [(usize, Registration)],
    definitions: &[BlockDefinition],
    registry: &mut content::Registry,
) -> io::Result<()> {
    let placed: HashSet<_> = definitions
        .iter()
        .flat_map(|block| block.states.iter().map(|state| state.carrier))
        .collect();
    let mut spare =
        crate::content_stairs::corner_aliases().filter(|(carrier, _)| !placed.contains(carrier));
    let mut by_model = HashMap::new();
    for (_, block) in blocks {
        let registered = registry
            .block(&block.id)
            .ok_or_else(|| io::Error::other("custom block missing after registration"))?;
        for (state, definition) in registered.states.iter().zip(&mut block.states) {
            let visual = &mut definition.visual;
            if !visual.bounded {
                continue;
            }
            let look = (
                visual.rotating_model.clone(),
                visual.x,
                visual.y,
                visual.uvlock,
            );
            if let Some((carrier, replacement)) =
                *by_model.entry(look).or_insert_with(|| spare.next())
            {
                registry.set_block_display_carrier(state.id, carrier, replacement)?;
                visual.block_display_carrier = Some(carrier);
            }
        }
    }
    if by_model.values().any(Option::is_none) {
        tracing::warn!("moddedmc: stock block display carriers exhausted");
    }
    Ok(())
}

#[expect(
    clippy::too_many_lines,
    reason = "install the registry, callbacks, and pack together"
)]
pub fn open_world(path: &Path, config: &mut AdvancedConfiguration) -> io::Result<()> {
    load_mods();
    let Pending {
        mut blocks,
        mut items,
        ticks,
        replaced,
        entity_inside,
        use_on_block,
        item_use,
        vanilla_use_on_block,
        use_item,
        player_placed,
        melee_damage,
        use_block,
        conversion,
        player_will_destroy,
        redstone,
    } = std::mem::take(&mut *lock(&PENDING));
    blocks.sort_by(|(_, a), (_, b)| a.id.cmp(&b.id));
    items.sort_by(|(_, a), (_, b)| a.id.cmp(&b.id));
    #[expect(
        clippy::needless_collect,
        reason = "items and blocks are consumed before the registry is installed"
    )]
    let owners: Vec<(String, usize)> = items
        .iter()
        .map(|(owner, item)| (item.id.clone(), *owner))
        .chain(
            blocks
                .iter()
                .map(|(owner, block)| (block.id.clone(), *owner)),
        )
        .collect();

    let fish_buckets: Vec<_> = items
        .iter_mut()
        .filter_map(|(_, item)| item.bucket_mob.take().map(|mob| (item.id.clone(), mob)))
        .collect();
    let arrow_ammo = items
        .iter_mut()
        .filter_map(|(_, item)| item.arrow.take().map(|arrow| (item.id.clone(), arrow)))
        .collect();
    let (item_definitions, item_assets) = split_items(items);
    let registry = open_registry(path, &mut blocks, &item_definitions)?;
    for item in &item_assets {
        tracing::info!("moddedmc: registered content item {}", item.id);
    }
    let mut runtime = Runtime {
        blocks: HashMap::new(),
        overlays: HashMap::new(),
        ticks,
        replaced,
        entity_inside,
        use_on_block,
        item_use,
        vanilla_use_on_block,
        use_item,
        player_placed,
        melee_damage,
        use_block,
        conversion,
        player_will_destroy,
        redstone,
        item_owners: HashMap::new(),
    };
    let mut assets = Vec::with_capacity(blocks.len());
    for (owner, block) in blocks {
        let entry = registry
            .block(&block.id)
            .ok_or_else(|| io::Error::other("custom block missing after registration"))?;
        runtime.blocks.insert(
            entry.id.as_u16(),
            RuntimeBlock {
                owner,
                slab: crate::content_slab::template(&block.states),
                stairs: crate::content_stairs::has_stair_shape(&block.states),
                ladder: crate::content_ladders::has_ladder_shape(&block.states),
                scaffolding: crate::content_scaffolding::has_scaffolding_shape(&block.states),
                trapdoor: crate::content_trapdoors::has_trapdoor_shape(&block.states),
                weathering: matches!(block.copper_stage, Some(0..=2)),
                wall: content_walls::has_wall_shape(&block.states),
                pane: content_panes::has_pane_shape(&block.states),
                placement: block.placement,
                random_tick: block.random_tick,
                item_input: block.item_input,
                item_output: block.item_output,
            },
        );
        tracing::info!(
            "moddedmc: registered content block {} ({} states)",
            block.id,
            entry.states.len()
        );
        assets.push(content_pack::BlockAssets {
            id: block.id,
            item_model: block.item_model,
            states: block.states.into_iter().map(|state| state.visual).collect(),
        });
    }
    registry.install()?;
    install_fish_buckets(fish_buckets)?;
    install_arrow_ammo(arrow_ammo)?;
    runtime.item_owners = owners
        .into_iter()
        .filter_map(|(id, owner)| Some((Item::from_registry_key(&id)?.id, owner)))
        .collect();
    runtime.overlays = content_pack::configure(
        &config.networking.java.address,
        &mut config.resource_pack.java,
        &assets,
        &item_assets,
    )?;
    install_runtime(runtime)
}

fn install_runtime(runtime: Runtime) -> io::Result<()> {
    let has_conversion = !runtime.conversion.is_empty();
    if RUNTIME.set(runtime).is_err()
        || !install_content_behaviour(Arc::new(Behaviour))
        || !crate::interaction::install()
        || !install_entity_added_hook(on_entity_added)
        || !pumpkin_core::world::game_event::install_game_event_observer(
            crate::entities::observe_game_event,
        )
        || !crate::mobs::install_structure_mobs()
        || !crate::mobs::install_natural_spawns()
        || (has_conversion && !container_conversion::install(propose_container_conversion))
        || !engine::observe(observe)
    {
        return Err(io::Error::other("content hooks already installed"));
    }
    Ok(())
}

pub static CONTAINER_CONVERSION: abi::ContainerConversionV1 = abi::ContainerConversionV1 {
    size: size_of::<abi::ContainerConversionV1>(),
    register: register_container_conversion,
};

unsafe extern "C" fn register_container_conversion(
    reg: Reg,
    callback: abi::ContainerConversionFn,
) -> i32 {
    if RUNTIME.get().is_some() {
        return code::INVALID_DEFINITION;
    }
    // SAFETY: the registration handle is borrowed for this call.
    let slot = &mut unsafe { native::staging(reg) }.content_hooks.conversion;
    if slot.is_some() {
        return code::DUPLICATE_ID;
    }
    *slot = Some(callback);
    code::OK
}

fn propose_container_conversion(
    world: &World,
    pos: BlockPos,
    state: BlockStateId,
    source: &NbtCompound,
) -> container_conversion::Proposal {
    use container_conversion::Proposal;

    let Some(runtime) = RUNTIME.get() else {
        return Proposal::Pass;
    };
    let bytes = Nbt::from(source.clone()).write();
    if bytes.len() > abi::MAX_BLOCK_ENTITY_BYTES {
        return Proposal::Pass;
    }
    let candidate = abi::ConversionCandidate {
        world: ptr::from_ref(world).cast(),
        position: abi_pos(pos),
        state: u32::from(state.as_u16()),
        nbt: Str::new(&bytes),
    };
    for (&owner, callback) in &runtime.conversion {
        let mut output: Result<Option<(BlockStateId, NbtCompound)>, ()> = Ok(None);
        let sink = Sink {
            ctx: ptr::from_mut(&mut output).cast(),
            put: collect_container_conversion,
        };
        // SAFETY: the candidate and sink are borrowed for the synchronous callback.
        match unsafe { callback(&raw const candidate, sink) } {
            abi::CONVERSION_PASS => {}
            abi::CONVERSION_REPLACE => {
                if let Ok(Some((state, nbt))) = output
                    && runtime_block(state.to_block().id).is_some_and(|block| block.owner == owner)
                {
                    return Proposal::Replace(state, nbt);
                }
                return Proposal::Defer;
            }
            _ => return Proposal::Defer,
        }
    }
    Proposal::Pass
}

unsafe extern "C" fn collect_container_conversion(ctx: *mut c_void, key: u64, bytes: Str) {
    // SAFETY: the synchronous proposal supplied a live output slot.
    let output = unsafe { &mut *ctx.cast::<Result<Option<(BlockStateId, NbtCompound)>, ()>>() };
    if !matches!(output, Ok(None)) || !(1..=abi::MAX_BLOCK_ENTITY_BYTES).contains(&bytes.len) {
        *output = Err(());
        return;
    }
    let state = u16::try_from(key)
        .ok()
        .and_then(BlockStateId::from_runtime_id);
    // SAFETY: the mod lends the replacement NBT to the sink for this call.
    *output = state
        .zip(read_nbt(unsafe { bytes.bytes() }))
        .map(Some)
        .ok_or(());
}

fn on_entity_added(_world: &World, entity: &Arc<dyn EntityBase>) {
    crate::mob_ai::attach(entity);
    crate::native_entity::attach(entity);
}

struct Behaviour;

fn item_input(block: &Block) -> Option<abi::ItemInputFn> {
    RUNTIME.get()?.blocks.get(&block.id.as_u16())?.item_input
}

fn runtime_block(id: BlockId) -> Option<&'static RuntimeBlock> {
    RUNTIME.get()?.blocks.get(&id.as_u16())
}

struct DefaultBlockBehaviour;

impl BlockBehaviour for DefaultBlockBehaviour {}

impl BlockBehaviour for Behaviour {
    fn is_pathfindable(&self, state: &BlockState, computation: PathComputationType) -> bool {
        match runtime_block(state.id.to_block().id) {
            Some(block) if block.stairs => StairBlock.is_pathfindable(state, computation),
            Some(block) if block.wall => WallBlock.is_pathfindable(state, computation),
            Some(block) if block.pane => IronBarsBlock.is_pathfindable(state, computation),
            Some(block) if block.trapdoor => {
                crate::content_trapdoors::is_pathfindable(state, computation)
            }
            Some(block) if block.slab.is_some() => SlabBlock.is_pathfindable(state, computation),
            _ => DefaultBlockBehaviour.is_pathfindable(state, computation),
        }
    }

    fn rotate(
        &self,
        block: &Block,
        state: BlockStateId,
        rotation: Rotation,
    ) -> &'static BlockState {
        if runtime_block(block.id).is_some_and(|block| block.stairs) {
            StairBlock.rotate(block, state, rotation)
        } else {
            block.rotate(state, rotation)
        }
    }

    fn mirror(&self, block: &Block, state: BlockStateId, mirror: Mirror) -> &'static BlockState {
        if runtime_block(block.id).is_some_and(|block| block.stairs) {
            StairBlock.mirror(block, state, mirror)
        } else {
            block.mirror(state, mirror)
        }
    }

    fn random_tick_sampled(&self, args: RandomTickArgs<'_>, state: BlockStateId) {
        let Some(block) = runtime_block(args.block.id) else {
            return;
        };
        if block.weathering {
            change_over_time(args.world, args.position, args.block);
            return;
        }
        let Some(callback) = block.random_tick else {
            return;
        };
        // SAFETY: the world is borrowed and the library stays loaded during ordered dispatch.
        unsafe {
            callback(
                ptr::from_ref(args.world.as_ref()).cast(),
                abi_pos(*args.position),
                u32::from(state.as_u16()),
            );
        }
    }

    fn has_item_input(&self, block: &Block) -> bool {
        item_input(block).is_some()
    }

    fn insert_item(&self, args: ItemInputArgs<'_>) -> u8 {
        if args.world.get_block_state_id(&args.position) != args.state {
            return 0;
        }
        let (Some(receive), Some(saved)) = (
            item_input(Block::from_state_id(args.state)),
            args.stack.to_nbt(),
        ) else {
            return 0;
        };
        let bytes = Nbt::from(saved).write();
        if bytes.len() > abi::MAX_STACK_BYTES {
            return 0;
        }
        // SAFETY: the callback borrows the world and saved stack for this call.
        let accepted = unsafe {
            receive(
                ptr::from_ref(args.world).cast(),
                abi_pos(args.position),
                u32::from(args.state.as_u16()),
                args.face.to_index(),
                Str::new(&bytes),
                args.simulate,
            )
        };
        accepted.min(u32::from(args.stack.item_count)) as u8
    }

    fn item_output_slots(&self, block: &Block) -> u32 {
        runtime_block(block.id)
            .and_then(|entry| entry.item_output)
            .map_or(0, |(slots, _)| slots)
    }

    fn extract_item(&self, args: ItemOutputArgs<'_>) -> Option<ItemStack> {
        if args.world.get_block_state_id(&args.position) != args.state {
            return None;
        }
        let (slots, take) = runtime_block(Block::from_state_id(args.state).id)?.item_output?;
        if args.slot >= slots {
            return None;
        }
        let expected = match args.expected {
            Some(stack) => Nbt::from(stack.to_nbt()?).write(),
            None => bytes::Bytes::new(),
        };
        if expected.len() > abi::MAX_STACK_BYTES {
            return None;
        }
        let mut output: Option<ItemStack> = None;
        let sink = Sink {
            ctx: ptr::from_mut(&mut output).cast(),
            put: collect_item_output,
        };
        // SAFETY: the callback borrows the world, expected stack and sink synchronously.
        let ok = unsafe {
            take(
                ptr::from_ref(args.world).cast(),
                abi_pos(args.position),
                u32::from(args.state.as_u16()),
                args.face.to_index(),
                args.slot,
                Str::new(&expected),
                sink,
            )
        };
        output.filter(|item| ok && item.item_count == 1 && !item.is_empty())
    }

    fn can_place_at(&self, args: CanPlaceAtArgs<'_>) -> bool {
        if runtime_block(args.block.id).is_some_and(|block| block.ladder) {
            return LadderBlock.can_place_at(args);
        }
        true
    }

    fn extends_scaffolding(&self, block: &Block) -> bool {
        runtime_block(block.id).is_some_and(|runtime| runtime.scaffolding)
    }

    fn can_update_at(&self, args: CanUpdateAtArgs<'_>) -> bool {
        runtime_block(args.block.id)
            .and_then(|block| block.slab)
            .is_some_and(|slab| crate::content_slab::can_update_at(&args, slab))
    }

    fn get_state_for_neighbor_update(
        &self,
        args: GetStateForNeighborUpdateArgs<'_>,
    ) -> BlockStateId {
        let Some(block) = runtime_block(args.block.id) else {
            return args.state_id;
        };
        if block.stairs {
            return StairBlock.get_state_for_neighbor_update(args);
        }
        if block.wall {
            return WallBlock.get_state_for_neighbor_update(args);
        }
        if block.pane {
            return IronBarsBlock.get_state_for_neighbor_update(args);
        }
        if block.ladder {
            return LadderBlock.get_state_for_neighbor_update(args);
        }
        if block.scaffolding {
            return crate::content_scaffolding::neighbor_update(args);
        }
        if block.trapdoor {
            return crate::content_trapdoors::neighbor_shape(&args);
        }
        block.slab.map_or(args.state_id, |slab| {
            crate::content_slab::neighbor_update(&args, slab)
        })
    }

    fn on_place(&self, args: OnPlaceArgs<'_>) -> BlockStateId {
        let default = args.block.default_state.id;
        let Some(block) = RUNTIME
            .get()
            .and_then(|r| r.blocks.get(&args.block.id.as_u16()))
        else {
            return default;
        };
        let Some(callback) = block.placement else {
            if block.scaffolding {
                return crate::content_scaffolding::on_place(args);
            }
            if block.trapdoor {
                return crate::content_trapdoors::on_place(&args);
            }
            if block.stairs {
                return StairBlock.on_place(args);
            }
            if block.wall {
                return WallBlock.on_place(args);
            }
            if block.pane {
                return IronBarsBlock.on_place(args);
            }
            if block.ladder {
                return LadderBlock.on_place(args);
            }
            return block
                .slab
                .map_or(default, |slab| crate::content_slab::on_place(args, slab));
        };
        let entity = args.player.get_entity();
        let placement = abi::Placement {
            world: ptr::from_ref(args.world).cast(),
            state: u32::from(default.as_u16()),
            position: abi_pos(*args.position),
            face: args.direction as u8,
            sneak: entity.is_sneaking(),
            yaw: entity.yaw.load(),
            pitch: entity.pitch.load(),
        };
        // SAFETY: arguments live through the callback and the library stays loaded.
        let selected = unsafe { callback(&raw const placement) };
        u16::try_from(selected)
            .ok()
            .and_then(BlockStateId::from_runtime_id)
            .filter(|id| id.to_block().id == args.block.id)
            .unwrap_or_else(|| {
                if block.ladder {
                    LadderBlock.on_place(args)
                } else {
                    default
                }
            })
    }

    fn player_placed(&self, args: PlayerPlacedArgs<'_>) {
        let Some(runtime) = RUNTIME.get() else { return };
        let Some(callback) = runtime
            .blocks
            .get(&args.block.id.as_u16())
            .and_then(|block| runtime.player_placed.get(&block.owner))
        else {
            return;
        };
        let player: &dyn EntityBase = args.player;
        // SAFETY: the borrowed world and player outlive this synchronous callback.
        unsafe {
            callback(
                ptr::from_ref(args.world.as_ref()).cast(),
                ptr::from_ref(&player).cast(),
                abi_pos(*args.position),
                u32::from(args.state_id.as_u16()),
            );
        }
    }

    fn on_entity_collision(&self, args: OnEntityCollisionArgs<'_>) {
        let Some(runtime) = RUNTIME.get() else { return };
        let Some(callback) = runtime
            .blocks
            .get(&args.block.id.as_u16())
            .and_then(|block| runtime.entity_inside.get(&block.owner))
        else {
            return;
        };
        let entity: &dyn EntityBase = args.entity;
        // SAFETY: the world and the entity reference outlive this synchronous callback.
        unsafe {
            callback(
                ptr::from_ref(args.world.as_ref()).cast(),
                abi_pos(*args.position),
                u32::from(args.state.id.as_u16()),
                ptr::from_ref(&entity).cast(),
            );
        }
    }

    fn emits_redstone_power(&self, args: EmitsRedstonePowerArgs<'_>) -> bool {
        redstone_callbacks(args.block).is_some_and(|(_, connects)| {
            // SAFETY: a pure callback with plain arguments.
            unsafe { connects(u32::from(args.state.id.as_u16()), args.direction.to_index()) }
        })
    }

    fn get_weak_redstone_power(&self, args: GetRedstonePowerArgs<'_>) -> u8 {
        redstone_signal(&args, false)
    }

    fn get_strong_redstone_power(&self, args: GetRedstonePowerArgs<'_>) -> u8 {
        redstone_signal(&args, true)
    }

    fn get_comparator_output(&self, args: GetComparatorOutputArgs<'_>) -> Option<u8> {
        content::state(args.state.id.as_u16())?.comparator_output
    }

    fn player_will_destroy(&self, args: PlayerWillDestroyArgs<'_>) {
        let Some(callback) = RUNTIME.get().and_then(|runtime| {
            let owner = runtime.blocks.get(&args.block.id.as_u16())?.owner;
            runtime.player_will_destroy.get(&owner).copied()
        }) else {
            return;
        };
        let world: &World = args.world;
        let entity: &dyn EntityBase = args.player.as_ref();
        // SAFETY: the world and player outlive this synchronous callback.
        unsafe {
            callback(
                ptr::from_ref(world).cast(),
                ptr::from_ref(&entity).cast(),
                abi_pos(*args.position),
                u32::from(args.state.id.as_u16()),
                args.player.is_creative(),
            );
        }
    }

    fn use_with_item(&self, args: UseWithItemArgs<'_>) -> BlockActionResult {
        let Some(callback) = use_block_callback(args.block) else {
            return BlockActionResult::PassToDefaultBlockAction;
        };
        let context = use_context(
            args.world,
            *args.position,
            args.hit,
            args.player,
            args.equipment_slot == &EquipmentSlot::MAIN_HAND,
            true,
        );
        let hand = if args.equipment_slot == &EquipmentSlot::MAIN_HAND {
            Hand::Right
        } else {
            Hand::Left
        };
        let stack = (args.item_stack, Some(hand));
        match interaction::use_block(callback, args.world, args.player, stack, &context) {
            abi::USE_SUCCESS => BlockActionResult::Success,
            abi::USE_CONSUME => BlockActionResult::Consume,
            abi::USE_FAIL => BlockActionResult::Fail,
            abi::USE_PASS => BlockActionResult::Pass,
            _ => BlockActionResult::PassToDefaultBlockAction,
        }
    }

    fn normal_use(&self, args: NormalUseArgs<'_>) -> BlockActionResult {
        if runtime_block(args.block.id).is_some_and(|block| block.trapdoor) {
            return crate::content_trapdoors::normal_use(&args);
        }
        let Some(callback) = use_block_callback(args.block) else {
            return BlockActionResult::Pass;
        };
        let context = use_context(
            args.world,
            *args.position,
            args.hit,
            args.player,
            true,
            false,
        );
        let mut empty = ItemStack::EMPTY.clone();
        let stack = (&mut empty, None);
        match interaction::use_block(callback, args.world, args.player, stack, &context) {
            abi::USE_SUCCESS => BlockActionResult::Success,
            abi::USE_CONSUME => BlockActionResult::Consume,
            abi::USE_FAIL => BlockActionResult::Fail,
            _ => BlockActionResult::Pass,
        }
    }

    fn explode(&self, args: ExplodeArgs<'_>) {
        if runtime_block(args.block.id).is_some_and(|block| block.trapdoor) {
            crate::content_trapdoors::explode(&args);
        }
    }

    fn on_neighbor_update(&self, args: OnNeighborUpdateArgs<'_>) {
        if runtime_block(args.block.id).is_some_and(|block| block.trapdoor) {
            crate::content_trapdoors::neighbor_update(&args);
        }
        let state = args.world.get_block_state_id(args.position);
        queue(
            args.world,
            *args.position,
            state,
            state,
            abi::NEIGHBOR_CHANGED,
        );
    }
}

fn observe(world: &World, pos: BlockPos, change: Change) {
    match change {
        Change::State(old, state) => queue(world, pos, old, state, abi::STATE_CHANGED),
        Change::Moved(old, state) => queue(world, pos, old, state, abi::MOVED_STATE_CHANGED),
        Change::Loaded(state) => queue(world, pos, BlockStateId::AIR, state, abi::LOADED),
        Change::Unloaded => {
            let mut worlds = lock(&WORLDS);
            if let Some(w) = worlds.get_mut(&world.uuid)
                && let Some(old) = w.blocks.remove(&pos)
            {
                w.rotation.remove(&pos);
                w.animations.remove(&pos);
                if let Some(owner) = owner(old) {
                    w.events
                        .push((owner, event(pos, old, BlockStateId::AIR, abi::UNLOADED)));
                }
            }
        }
    }
}

fn owner(id: BlockStateId) -> Option<usize> {
    let entry = content::state(id.as_u16())?;
    Some(RUNTIME.get()?.blocks.get(&entry.block.id.as_u16())?.owner)
}

fn event(pos: BlockPos, old: BlockStateId, state: BlockStateId, reason: u8) -> ContentEvent {
    ContentEvent {
        position: abi_pos(pos),
        previous: u32::from(old.as_u16()),
        state: u32::from(state.as_u16()),
        reason,
    }
}

fn queue(world: &World, pos: BlockPos, old: BlockStateId, state: BlockStateId, reason: u8) {
    let (before, after) = (owner(old), owner(state));
    if before.is_none() && after.is_none() {
        return;
    }
    let event = event(pos, old, state, reason);
    if let Some(owner) = before
        && matches!(reason, abi::STATE_CHANGED | abi::MOVED_STATE_CHANGED)
        && let Some(callback) = RUNTIME
            .get()
            .and_then(|runtime| runtime.replaced.get(&owner))
        && Block::from_state_id(old) != Block::from_state_id(state)
    {
        // SAFETY: the world and event are borrowed for this synchronous callback.
        unsafe { callback(ptr::from_ref(world).cast(), &raw const event) };
    }
    let mut worlds = lock(&WORLDS);
    let w = worlds.entry(world.uuid).or_default();
    if after.is_some() {
        w.blocks.insert(pos, state);
        if Block::from_state_id(old) != Block::from_state_id(state) {
            w.animations.remove(&pos);
        }
    } else {
        w.blocks.remove(&pos);
        w.rotation.remove(&pos);
        w.animations.remove(&pos);
    }
    if let Some(before) = before.filter(|before| Some(*before) != after) {
        w.events.push((before, event));
    }
    if let Some(after) = after {
        w.events.push((after, event));
    }
}

thread_local! {
    static TICK_WORLD: Cell<WorldRef> = const { Cell::new(ptr::null()) };
}

/// Whether this thread is dispatching the serial content tick for this world.
pub fn in_world_tick(world: WorldRef) -> bool {
    TICK_WORLD.with(|current| current.get() == world && !world.is_null())
}

pub fn tick(server: &Arc<Server>, time: u64) {
    let Some(runtime) = RUNTIME.get() else { return };
    // A mod may only subscribe to world ticks (e.g. save-global data) without
    // registering any custom blocks. It still needs its callback every tick.
    if runtime.blocks.is_empty() && runtime.ticks.is_empty() {
        return;
    }
    for world in server.worlds.load().iter() {
        let mut events: BTreeMap<usize, Vec<ContentEvent>> = BTreeMap::new();
        if let Some(w) = lock(&WORLDS).get_mut(&world.uuid) {
            for (owner, event) in w.events.drain(..) {
                events.entry(owner).or_default().push(event);
            }
        }
        for (owner, callback) in &runtime.ticks {
            let events = events.get(owner).map_or(&[][..], Vec::as_slice);
            let raw = ptr::from_ref(world.as_ref()).cast();
            TICK_WORLD.with(|current| current.set(raw));
            // SAFETY: the world and event array are live for this synchronous callback.
            unsafe {
                callback(raw, time, events.as_ptr(), events.len());
            };
            TICK_WORLD.with(|current| current.set(ptr::null()));
        }
        render(world, runtime, time);
    }
}

fn update_overlay_clip(overlay: &mut Overlay, clip: &str) {
    if overlay.clip == clip {
        return;
    }
    let mut stack = overlay.entity.get_item();
    let mut custom = stack
        .get_data_component::<CustomDataImpl>()
        .map_or_else(NbtCompound::new, |data| data.data.clone());
    if clip.is_empty() {
        custom.child_tags.remove("moddedmc:rig_clip");
    } else {
        custom.put_string("moddedmc:rig_clip", clip.to_owned());
    }
    if custom.is_empty() {
        stack.remove_data_component(DataComponent::CustomData);
    } else {
        stack.set_data_component(CustomDataImpl::new(custom));
    }
    overlay.entity.set_item(stack);
    clip.clone_into(&mut overlay.clip);
}

fn render(world: &Arc<World>, runtime: &Runtime, tick: u64) {
    if world.players.load().is_empty() {
        let displays = lock(&WORLDS)
            .get_mut(&world.uuid)
            .map_or_else(HashMap::new, |state| std::mem::take(&mut state.displays));
        for overlay in displays.into_values() {
            world.remove_entity(overlay.entity.as_ref());
        }
        return;
    }
    let Some((blocks, rotation, animations, mut displays)) = ({
        lock(&WORLDS).get_mut(&world.uuid).map(|w| {
            (
                w.blocks.clone(),
                w.rotation.clone(),
                w.animations.clone(),
                std::mem::take(&mut w.displays),
            )
        })
    }) else {
        return;
    };

    displays.retain(|pos, overlay| {
        let keep = blocks.get(pos) == Some(&overlay.state);
        if !keep {
            world.remove_entity(overlay.entity.as_ref());
        }
        keep
    });
    for (pos, state) in blocks {
        let Some(model) = runtime.overlays.get(&state.as_u16()) else {
            continue;
        };
        let overlay = displays.entry(pos).or_insert_with(|| {
            let y_offset = if model.bounded { 0.0 } else { 0.5 };
            let center = Vector3::new(
                f64::from(pos.0.x) + 0.5,
                f64::from(pos.0.y) + y_offset,
                f64::from(pos.0.z) + 0.5,
            );
            let entity = Entity::new(world.clone(), center, &EntityType::ITEM_DISPLAY);
            entity.set_has_no_gravity(true);
            let display = ItemDisplayEntity::transient(entity);
            display.init_data_tracker();
            let mut stack = ItemStack::new(1, &Item::PAPER);
            stack.set_data_component(ItemModelImpl {
                id: Cow::Owned(model.id.clone()),
            });
            if let Some(wall) = model.wall {
                stack.set_data_component(wall.component());
                // Keep the wall render just outside the matching stock carrier.
                display.display.set_scale(Vector3::new(
                    content_walls::WALL_OVERLAY_SCALE,
                    content_walls::WALL_OVERLAY_SCALE,
                    content_walls::WALL_OVERLAY_SCALE,
                ));
            }
            display.set_item(stack);
            display.display.set_right_rotation(model.orientation);
            if model.bounded {
                // Java anchors its culling box at the entity's lower Y.
                display.display.set_translation(Vector3::new(0.0, 0.5, 0.0));
                display.display.set_display_width(1.0);
                display.display.set_display_height(1.0);
                // New trackers receive the initial non-default metadata when they pair.
                display.get_entity().synched_data.clear_dirty();
            }
            world.spawn_entity_non_save(display.clone());
            Overlay {
                entity: display,
                phase: 0.0,
                state,
                moving: true,
                clip: String::new(),
            }
        });
        update_overlay_clip(overlay, animations.get(&pos).map_or("", String::as_str));
        if model.bounded {
            continue;
        }
        let (axis, rpm) = rotation.get(&pos).copied().unwrap_or((1, 0.0));
        let delta = rpm * std::f32::consts::TAU / 1200.0;
        let interval = (1.5 / delta.abs()).clamp(1.0, 5.0) as u64;
        overlay.phase = (overlay.phase + delta).rem_euclid(std::f32::consts::TAU);
        let moving = rpm != 0.0;
        if moving != overlay.moving || (moving && tick.is_multiple_of(interval)) {
            let (sin, cos) = f32::midpoint(overlay.phase, delta * interval as f32).sin_cos();
            let mut spin = [0.0, 0.0, 0.0, cos];
            spin[usize::from(axis)] = sin;
            overlay
                .entity
                .display
                .set_interpolation_duration(interval as i32);
            overlay.entity.display.set_left_rotation(spin);
            overlay
                .entity
                .display
                .set_interpolation_start_delta_ticks(0);
            overlay.moving = moving;
        }
    }
    if let Some(w) = lock(&WORLDS).get_mut(&world.uuid) {
        w.displays = displays;
    }
}

pub const unsafe fn world<'a>(world: WorldRef) -> Option<&'a World> {
    // SAFETY: the caller keeps the world alive for the operation.
    unsafe { world.cast::<World>().as_ref() }
}

unsafe extern "C" fn read(raw: WorldRef, pos: *const Pos, out: *mut u32, count: usize) -> bool {
    if count > MAX_BATCH || pos.is_null() || out.is_null() {
        return false;
    }
    // SAFETY: world and both arrays are live for the call.
    let Some(world) = (unsafe { world(raw) }) else {
        return false;
    };
    for i in 0..count {
        // SAFETY: i is in bounds of both caller-provided arrays.
        unsafe {
            *out.add(i) = world
                .get_block_state_id_if_loaded(&block_pos(*pos.add(i)))
                .map_or(abi::UNLOADED_STATE, |id| u32::from(id.as_u16()));
        }
    }
    true
}

/// The server's owning handle for a borrowed world, for engine calls that need an `Arc`.
pub fn shared(world: &World) -> Option<Arc<World>> {
    let server = native::SERVER.get()?;
    server
        .worlds
        .load()
        .iter()
        .find(|w| w.uuid == world.uuid)
        .cloned()
}

unsafe extern "C" fn write(raw: WorldRef, updates: *const BlockUpdate, count: usize) -> usize {
    if count > MAX_BATCH || updates.is_null() {
        return 0;
    }
    // SAFETY: world and update list are live for the call.
    let Some(world) = (unsafe { world(raw) }).and_then(shared) else {
        return 0;
    };
    // SAFETY: the caller provides count consecutive updates.
    let updates = unsafe { std::slice::from_raw_parts(updates, count) };
    let Some(validated) = updates
        .iter()
        .map(|update| {
            let pos = block_pos(update.position);
            let state = BlockStateId::from_runtime_id(u16::try_from(update.state).ok()?)?;
            if update.flags
                & !(abi::UPDATE_ALL
                    | abi::UPDATE_MOVED
                    | abi::UPDATE_KNOWN_SHAPE
                    | abi::UPDATE_FROM_NEIGHBOR_SHAPES)
                != 0
            {
                return None;
            }
            let mut flags = BlockFlags::from_bits(u32::from(update.flags & abi::UPDATE_ALL))?;
            if update.flags & abi::UPDATE_MOVED != 0 {
                flags |= BlockFlags::MOVED
                    | BlockFlags::SKIP_DROPS
                    | BlockFlags::SKIP_BLOCK_ENTITY_REPLACED_CALLBACK;
            }
            if update.flags & abi::UPDATE_KNOWN_SHAPE != 0 {
                flags |= BlockFlags::UPDATE_KNOWN_SHAPE;
            }
            world.get_block_state_id_if_loaded(&pos)?;
            let resolve_shape = update.flags & abi::UPDATE_FROM_NEIGHBOR_SHAPES != 0;
            Some((pos, state, flags, resolve_shape))
        })
        .collect::<Option<Vec<_>>>()
    else {
        return 0;
    };
    for (pos, mut state, flags, resolve_shape) in validated {
        if resolve_shape {
            state = world.update_from_neighbor_shapes(state, &pos);
        }
        world.set_block_state(&pos, state, flags);
    }
    count
}

unsafe extern "C" fn command_world(source: *const c_void) -> WorldRef {
    // SAFETY: the command context is live for the full callback.
    unsafe { source.cast::<CommandContext<'_>>().as_ref() }
        .and_then(|source| source.source.world.as_deref())
        .map_or(ptr::null(), |world| ptr::from_ref(world).cast())
}

unsafe extern "C" fn read_entity(raw: WorldRef, pos: Pos, out: Sink) -> bool {
    // SAFETY: world and sink are borrowed for the operation.
    let Some(world) = (unsafe { world(raw) }) else {
        return false;
    };
    let pos = block_pos(pos);
    let Some(entity) = world.get_block_entity(&pos) else {
        return false;
    };
    if !entity.as_any().is::<ContentBlockEntity>() {
        return false;
    }
    let Some(nbt) = world.get_block_entity_nbt(&pos) else {
        return false;
    };
    let bytes = Nbt::from(nbt).write();
    // SAFETY: bytes are valid until the synchronous sink returns.
    unsafe { (out.put)(out.ctx, 0, Str::new(&bytes)) };
    true
}

unsafe extern "C" fn chest_blocked(raw: WorldRef, pos: Pos) -> i32 {
    // SAFETY: the world is borrowed for the synchronous callback.
    let Some(world) = (unsafe { world(raw) }) else {
        return -1;
    };
    let pos = block_pos(pos);
    if world.get_block_state_id_if_loaded(&pos).is_none() {
        return -1;
    }
    i32::from(pumpkin_core::block::blocks::chests::is_chest_blocked(
        world, &pos,
    ))
}

unsafe extern "C" fn content_entity_snapshot(
    raw: WorldRef,
    pos: Pos,
    state: *mut u32,
    incarnation: *mut u64,
    out: Sink,
) -> bool {
    // SAFETY: the callback borrows the world and distinct output slots.
    let (Some(world), Some(state), Some(incarnation)) =
        (unsafe { (world(raw), state.as_mut(), incarnation.as_mut()) })
    else {
        return false;
    };
    let Some((current, identity, nbt)) = world.content_entity_snapshot(&block_pos(pos)) else {
        return false;
    };
    let bytes = Nbt::from(nbt).write();
    if bytes.len() > abi::MAX_BLOCK_ENTITY_BYTES {
        return false;
    }
    *state = u32::from(current.as_u16());
    *incarnation = identity;
    // SAFETY: bytes and the sink are live until the synchronous call returns.
    unsafe { (out.put)(out.ctx, 0, Str::new(&bytes)) };
    true
}

unsafe extern "C" fn compare_content_entity(
    raw: WorldRef,
    pos: Pos,
    state: u32,
    incarnation: u64,
    expected: Str,
    replacement: Str,
) -> bool {
    if expected.len > abi::MAX_BLOCK_ENTITY_BYTES || replacement.len > abi::MAX_BLOCK_ENTITY_BYTES {
        return false;
    }
    // SAFETY: the caller borrows the world and both NBT inputs for this call.
    let (world, expected, replacement) = unsafe {
        (
            world(raw),
            read_nbt(expected.bytes()),
            read_nbt(replacement.bytes()),
        )
    };
    let state = u16::try_from(state)
        .ok()
        .and_then(BlockStateId::from_runtime_id);
    match (world, state, expected, replacement) {
        (Some(world), Some(state), Some(expected), Some(replacement)) => world
            .compare_content_entity(&block_pos(pos), state, incarnation, &expected, &replacement),
        _ => false,
    }
}

unsafe extern "C" fn compare_replace_content_entity(
    raw: WorldRef,
    pos: Pos,
    expected_state: u32,
    incarnation: u64,
    expected: Str,
    destination: u32,
    replacement: Str,
) -> bool {
    if expected.len > abi::MAX_BLOCK_ENTITY_BYTES || replacement.len > abi::MAX_BLOCK_ENTITY_BYTES {
        return false;
    }
    // SAFETY: the caller borrows the world and both NBT inputs for this call.
    let (world, expected) = unsafe { (world(raw), read_nbt(expected.bytes())) };
    let state = u16::try_from(expected_state)
        .ok()
        .and_then(BlockStateId::from_runtime_id);
    let destination = u16::try_from(destination)
        .ok()
        .and_then(BlockStateId::from_runtime_id);
    let (Some(world), Some(state), Some(expected), Some(destination)) =
        (world.and_then(shared), state, expected, destination)
    else {
        return false;
    };
    let replacement = if replacement.len == 0 {
        None
    } else {
        // SAFETY: replacement is a bounded borrowed byte string.
        let Some(nbt) = read_nbt(unsafe { replacement.bytes() }) else {
            return false;
        };
        Some(nbt)
    };
    world.compare_replace_content_entity(
        &block_pos(pos),
        state,
        incarnation,
        &expected,
        destination,
        replacement.as_ref(),
    )
}

unsafe extern "C" fn write_entity(raw: WorldRef, pos: Pos, nbt: Str) -> bool {
    if nbt.len > MAX_STATE_BYTES {
        return false;
    }
    // SAFETY: the world and NBT are live for this call.
    let (Some(world), bytes) = (unsafe { world(raw) }, unsafe { nbt.bytes() }) else {
        return false;
    };
    let (Some(data), pos) = (read_nbt(bytes), block_pos(pos)) else {
        return false;
    };
    let Some(entity) = world.get_block_entity(&pos) else {
        return false;
    };
    let Some(entity) = entity.as_any().downcast_ref::<ContentBlockEntity>() else {
        return false;
    };
    entity.set_data(data);
    let mut saved = pumpkin_nbt::compound::NbtCompound::new();
    entity.write_internal(&mut saved);
    world.add_block_entity_nbt(pos, &saved);
    true
}

/// Named-root Java NBT that fills the whole input.
fn read_nbt(bytes: &[u8]) -> Option<pumpkin_nbt::compound::NbtCompound> {
    let mut reader = std::io::Cursor::new(bytes);
    let nbt = Nbt::read(&mut pumpkin_nbt::deserializer::NbtReadHelperJava::new(
        &mut reader,
    ))
    .ok()?;
    (reader.position() == bytes.len() as u64).then_some(nbt.root_tag)
}

unsafe extern "C" fn read_block_entity(raw: WorldRef, pos: Pos, out: Sink) -> bool {
    // SAFETY: the world is borrowed for this call.
    let Some(world) = (unsafe { world(raw) }) else {
        return false;
    };
    let pos = block_pos(pos);
    if world.get_block_state_id_if_loaded(&pos).is_none() {
        return false;
    }
    let Some(nbt) = world.get_block_entity_nbt(&pos) else {
        return false;
    };
    let bytes = Nbt::from(nbt).write();
    // SAFETY: bytes are valid until the synchronous sink returns.
    unsafe { (out.put)(out.ctx, 0, Str::new(&bytes)) };
    true
}

unsafe extern "C" fn redstone_conductor(raw: WorldRef, pos: Pos) -> i32 {
    // SAFETY: the world is borrowed for this call.
    unsafe { world(raw) }
        .and_then(|world| world.get_block_state_id_if_loaded(&block_pos(pos)))
        .map_or(-1, |state| i32::from(state.to_state().is_solid_block()))
}

unsafe extern "C" fn apply_block_transformer(
    raw: WorldRef,
    origin: Pos,
    direction: u8,
    tool: Str,
) -> bool {
    // SAFETY: the world is borrowed for this call.
    let (Some(world), Some(direction), Some(tool)) = (
        (unsafe { world(raw) }).and_then(shared),
        BlockDirection::all().get(usize::from(direction)).copied(),
        crate::stacks::read_stack(tool),
    ) else {
        return false;
    };
    let origin = block_pos(origin);
    let delta = direction.to_offset();
    let (Some(x), Some(y), Some(z)) = (
        origin.0.x.checked_add(delta.x),
        origin.0.y.checked_add(delta.y),
        origin.0.z.checked_add(delta.z),
    ) else {
        return false;
    };
    let end = BlockPos(Vector3::new(x, y, z));
    if world.get_block_state_id_if_loaded(&origin).is_none()
        || world.get_block_state_id_if_loaded(&end).is_none()
    {
        return false;
    }
    let Some((pos, face, _)) =
        world.ray_trace_block(origin.to_centered_f64(), end.to_centered_f64(), false)
    else {
        return false;
    };
    pumpkin_core::item::items::hoe::apply_block_transformer(
        &world,
        &tool,
        pos,
        face,
        world.get_block(&pos),
    )
    .is_some()
}

unsafe extern "C" fn block_drops(raw: WorldRef, pos: Pos, tool: Str, out: Sink) -> bool {
    // SAFETY: the world is borrowed for this call.
    unsafe { world(raw) }
        .and_then(shared)
        .is_some_and(|world| collect_block_drops(&world, block_pos(pos), tool, &out, None))
}

unsafe extern "C" fn block_drops_with_player_actor(
    raw: WorldRef,
    pos: Pos,
    tool: Str,
    out: Sink,
) -> bool {
    // SAFETY: the world is borrowed for this call.
    let Some(world) = (unsafe { world(raw) }).and_then(shared) else {
        return false;
    };
    let pos = block_pos(pos);
    if world.get_block_state_id_if_loaded(&pos).is_none() {
        return false;
    }
    let actor = Entity::new(world.clone(), pos.to_centered_f64(), &EntityType::PLAYER);
    collect_block_drops(&world, pos, tool, &out, Some(&actor))
}

fn collect_block_drops(
    world: &Arc<World>,
    pos: BlockPos,
    tool: Str,
    out: &Sink,
    actor: Option<&dyn EntityBase>,
) -> bool {
    let Some(id) = world.get_block_state_id_if_loaded(&pos) else {
        return false;
    };
    let tool = if tool.len == 0 {
        None
    } else {
        let Some(tool) = crate::stacks::read_stack(tool) else {
            return false;
        };
        Some(tool)
    };
    let params = LootContextParameters {
        world: Some(world.as_ref()),
        tool,
        block_state: Some(BlockState::from_id(id)),
        position: Some(pos.to_f64()),
        this_entity: actor,
        ..Default::default()
    };
    let drops = pumpkin_core::block::collect_loot(world, Block::from_state_id(id), &pos, &params);
    for (index, stack) in (0u64..).zip(drops) {
        if let Some(saved) = stack.to_nbt() {
            let bytes = Nbt::from(saved).write();
            // SAFETY: bytes are valid until the synchronous sink returns.
            unsafe { (out.put)(out.ctx, index, Str::new(&bytes)) };
        }
    }
    true
}

unsafe extern "C" fn restore_block_entity(raw: WorldRef, pos: Pos, nbt: Str) -> bool {
    if nbt.len > abi::MAX_BLOCK_ENTITY_BYTES {
        return false;
    }
    // SAFETY: the world and NBT are live for this call.
    let (Some(world), bytes) = (unsafe { world(raw) }, unsafe { nbt.bytes() }) else {
        return false;
    };
    let pos = block_pos(pos);
    let (Some(_), Some(mut saved)) = (world.get_block_state_id_if_loaded(&pos), read_nbt(bytes))
    else {
        return false;
    };
    // The placed destination block already made its own entity, so a matching id is vanilla's
    // `BlockEntityType.isValid(state)`.
    let Some(current) = world.get_block_entity(&pos) else {
        return false;
    };
    if saved.get_string("id") != Some(current.resource_location()) {
        return false;
    }
    saved.put_int("x", pos.0.x);
    saved.put_int("y", pos.0.y);
    saved.put_int("z", pos.0.z);
    let Some(entity) = pumpkin_core::block::entities::block_entity_from_nbt(&saved) else {
        return false;
    };
    world.add_block_entity(entity);
    true
}

unsafe extern "C" fn rotate(raw: WorldRef, pos: Pos, axis: u8, rpm: f32) -> bool {
    if axis > 2 || !rpm.is_finite() || rpm.abs() > 1024.0 {
        return false;
    }
    // SAFETY: the world is borrowed for this call.
    let Some(world) = (unsafe { world(raw) }) else {
        return false;
    };
    let pos = block_pos(pos);
    let mut worlds = lock(&WORLDS);
    let Some(w) = worlds.get_mut(&world.uuid) else {
        return false;
    };
    if !w.blocks.get(&pos).is_some_and(|state| {
        RUNTIME
            .get()
            .and_then(|runtime| runtime.overlays.get(&state.as_u16()))
            .is_some_and(|model| !model.bounded)
    }) {
        return false;
    }
    w.rotation.insert(pos, (axis, rpm));
    true
}

pub unsafe extern "C" fn block_animation(raw: WorldRef, pos: Pos, clip: Str) -> bool {
    // SAFETY: the world and clip are borrowed for this call.
    let (Some(world), clip) = (unsafe { world(raw) }, unsafe { clip.str() }) else {
        return false;
    };
    if clip.len() > 256 || clip.chars().any(char::is_control) {
        return false;
    }
    let pos = block_pos(pos);
    let mut worlds = lock(&WORLDS);
    let Some(w) = worlds.get_mut(&world.uuid) else {
        return false;
    };
    if !w.blocks.get(&pos).is_some_and(|state| {
        RUNTIME
            .get()
            .is_some_and(|runtime| runtime.overlays.contains_key(&state.as_u16()))
    }) {
        return false;
    }
    if clip.is_empty() {
        w.animations.remove(&pos);
    } else if w.animations.get(&pos).is_none_or(|old| old != clip) {
        w.animations.insert(pos, clip.to_owned());
    }
    true
}

unsafe extern "C" fn has_neighbor_signal(raw: WorldRef, pos: Pos) -> bool {
    // SAFETY: the world is borrowed for this call.
    let Some(world) = (unsafe { world(raw) }) else {
        return false;
    };
    let pos = block_pos(pos);
    world.get_block_state_id_if_loaded(&pos).is_some() && block_receives_redstone_power(world, &pos)
}

unsafe extern "C" fn best_neighbor_signal(raw: WorldRef, pos: Pos) -> u8 {
    // SAFETY: the world is borrowed for this call.
    let Some(world) = (unsafe { world(raw) }) else {
        return 0;
    };
    let pos = block_pos(pos);
    if world.get_block_state_id_if_loaded(&pos).is_none() {
        return 0;
    }
    let mut best = 0;
    for direction in BlockDirection::all() {
        let neighbor = pos.offset(direction.to_offset());
        let (block, state) = world.get_block_and_state(&neighbor);
        best = best.max(get_redstone_power(
            block, state, world, &neighbor, direction,
        ));
        if best >= 15 {
            return 15;
        }
    }
    best
}

unsafe extern "C" fn signal(raw: WorldRef, pos: Pos, direction: u8) -> u8 {
    // SAFETY: the world is borrowed for this call.
    let (Some(world), Some(direction)) = (
        unsafe { world(raw) },
        BlockDirection::all().get(usize::from(direction)).copied(),
    ) else {
        return 0;
    };
    let pos = block_pos(pos);
    if world.get_block_state_id_if_loaded(&pos).is_none() {
        return 0;
    }
    let (block, state) = world.get_block_and_state(&pos);
    get_redstone_power(block, state, world, &pos, direction)
}

unsafe extern "C" fn update_neighbors(raw: WorldRef, pos: Pos) -> bool {
    // SAFETY: the world is borrowed for this call.
    let Some(world) = (unsafe { world(raw) }).and_then(shared) else {
        return false;
    };
    let pos = block_pos(pos);
    if world.get_block_state_id_if_loaded(&pos).is_none() {
        return false;
    }
    world.update_neighbors(&pos, None);
    true
}

unsafe extern "C" fn destroy(raw: WorldRef, pos: Pos, drop: bool) -> bool {
    // SAFETY: the world is borrowed for this call.
    let Some(world) = (unsafe { world(raw) }).and_then(shared) else {
        return false;
    };
    let pos = block_pos(pos);
    if world.get_block_state_id_if_loaded(&pos).is_none() {
        return false;
    }
    let mut flags = BlockFlags::NOTIFY_ALL;
    if !drop {
        flags.insert(BlockFlags::SKIP_DROPS);
    }
    world.break_block_naturally(&pos, None, true, flags)
}

unsafe extern "C" fn face_sturdy(raw: WorldRef, pos: Pos, face: u8) -> bool {
    // SAFETY: the world is borrowed for this call.
    let (Some(world), Some(face)) = (
        unsafe { world(raw) },
        BlockDirection::all().get(usize::from(face)).copied(),
    ) else {
        return false;
    };
    world
        .get_block_state_id_if_loaded(&block_pos(pos))
        .is_some_and(|id| BlockState::from_id(id).is_side_solid(face))
}

unsafe extern "C" fn collision_shape(
    raw: WorldRef,
    pos: Pos,
    out: *mut abi::CollisionBox,
    capacity: usize,
) -> i32 {
    // SAFETY: the world is borrowed for this call.
    let Some(world) = (unsafe { world(raw) }) else {
        return -1;
    };
    let pos = block_pos(pos);
    let Some(id) = world.get_block_state_id_if_loaded(&pos) else {
        return -1;
    };
    let mut count = 0;
    for shape in BlockState::from_id(id).get_block_collision_shapes_at(&pos) {
        if count < capacity && !out.is_null() {
            let (min, max) = (shape.min, shape.max);
            // SAFETY: `out` has room for `capacity` boxes and `count` is below it.
            unsafe {
                out.add(count).write(abi::CollisionBox {
                    min: [min.x, min.y, min.z],
                    max: [max.x, max.y, max.z],
                });
            }
        }
        count += 1;
    }
    i32::try_from(count).unwrap_or(i32::MAX)
}

/// Writes the selector `state` parses back: content properties in their stored order, vanilla
/// ones in the block's definition order.
unsafe extern "C" fn describe_state(state: u32, out: Sink) -> bool {
    let Some(id) = u16::try_from(state)
        .ok()
        .and_then(BlockStateId::from_runtime_id)
    else {
        return false;
    };
    let (name, properties): (&str, Vec<(&str, &str)>) = content::state(id.as_u16()).map_or_else(
        || {
            let block = Block::from_state_id(id);
            let properties = block.properties(id).map_or_else(Vec::new, |props| {
                props
                    .to_props()
                    .into_iter()
                    .map(|p| (p.key().to_str(), p.value_str()))
                    .collect()
            });
            (block.name, properties)
        },
        |entry| {
            let properties = entry.properties.iter();
            (
                entry.block.name,
                properties.map(|(k, v)| (k.as_str(), v.as_str())).collect(),
            )
        },
    );
    let mut selector = if name.contains(':') {
        name.to_owned()
    } else {
        format!("minecraft:{name}")
    };
    if !properties.is_empty() {
        let pairs: Vec<String> = properties.iter().map(|(k, v)| format!("{k}={v}")).collect();
        selector = format!("{selector}[{}]", pairs.join(","));
    }
    // SAFETY: the selector is live until the synchronous sink returns.
    unsafe { (out.put)(out.ctx, 0, Str::new(selector.as_bytes())) };
    true
}

unsafe extern "C" fn day_time(raw: WorldRef, out: *mut abi::DayTime) -> bool {
    // SAFETY: the world and `out` are live for this call.
    let (Some(world), Some(out)) = (unsafe { world(raw) }, unsafe { out.as_mut() }) else {
        return false;
    };
    // Level.getOverworldClockTime: every dimension reads the overworld's live clock.
    let overworld = world
        .server
        .upgrade()
        .map(|server| server.get_world_from_dimension(&Dimension::OVERWORLD));
    let clock = overworld.as_deref().unwrap_or(world);
    let ticks = lock(&clock.level_time).time_of_day;
    *out = abi::DayTime {
        ticks,
        fixed_time: world.dimension.has_fixed_time,
    };
    true
}

unsafe extern "C" fn game_time(raw: WorldRef, out: *mut i64) -> bool {
    // SAFETY: the caller borrows the world and a writable output slot.
    let (Some(world), Some(out)) = (unsafe { world(raw) }, unsafe { out.as_mut() }) else {
        return false;
    };
    let overworld = world
        .server
        .upgrade()
        .map(|server| server.get_world_from_dimension(&Dimension::OVERWORLD));
    *out = overworld.as_deref().unwrap_or(world).get_world_age();
    true
}

unsafe extern "C" fn destroy_speed(raw: WorldRef, pos: Pos, out: *mut f32) -> bool {
    // SAFETY: the world and `out` are live for this call.
    let (Some(world), Some(out)) = (unsafe { world(raw) }, unsafe { out.as_mut() }) else {
        return false;
    };
    let Some(id) = world.get_block_state_id_if_loaded(&block_pos(pos)) else {
        return false;
    };
    *out = BlockState::from_id(id).hardness;
    true
}

unsafe extern "C" fn can_survive(raw: WorldRef, pos: Pos, state: u32) -> i32 {
    // SAFETY: the world is borrowed for this call.
    let Some(world) = (unsafe { world(raw) }) else {
        return -1;
    };
    let Some(id) = u16::try_from(state)
        .ok()
        .and_then(BlockStateId::from_runtime_id)
    else {
        return -1;
    };
    let pos = block_pos(pos);
    if world.get_block_state_id_if_loaded(&pos).is_none() {
        return -1;
    }
    // Pumpkin's `can_place_at` with a world is vanilla `canSurvive`; neighbour updates call it.
    let survives = world.block_registry.can_place_at(
        None,
        Some(world),
        world,
        None,
        Block::from_state_id(id),
        BlockState::from_id(id),
        &pos,
        None,
        None,
    );
    i32::from(survives)
}

unsafe extern "C" fn block_item(state: u32, out: Sink) -> bool {
    let Some(id) = u16::try_from(state)
        .ok()
        .and_then(BlockStateId::from_runtime_id)
    else {
        return false;
    };
    let block = Block::from_state_id(id);
    let name = Item::from_id(block.item_id)
        .filter(|_| block.item_id != 0)
        .map_or("air", |item| item.registry_key);
    let id = if name.contains(':') {
        name.to_owned()
    } else {
        format!("minecraft:{name}")
    };
    // SAFETY: the id is live until the synchronous sink returns.
    unsafe { (out.put)(out.ctx, 0, Str::new(id.as_bytes())) };
    true
}

unsafe extern "C" fn fluid_id(raw: WorldRef, pos: Pos, out: Sink) -> bool {
    // SAFETY: the world is borrowed for this call.
    let Some(world) = (unsafe { world(raw) }) else {
        return false;
    };
    let pos = block_pos(pos);
    if world.get_block_state_id_if_loaded(&pos).is_none() {
        return false;
    }
    // Pumpkin reports fluid families by their flowing member; vanilla's type is the source one
    // for source states, waterlogged blocks included.
    let (fluid, state) = world.get_fluid_and_fluid_state(&pos);
    let id = if fluid.matches_type(&Fluid::WATER) {
        if state.is_source {
            "minecraft:water"
        } else {
            "minecraft:flowing_water"
        }
    } else if fluid.matches_type(&Fluid::LAVA) {
        if state.is_source {
            "minecraft:lava"
        } else {
            "minecraft:flowing_lava"
        }
    } else {
        "minecraft:empty"
    };
    // SAFETY: the id is live until the synchronous sink returns.
    unsafe { (out.put)(out.ctx, 0, Str::new(id.as_bytes())) };
    true
}

pub static ENTITIES: abi::EntitiesV1 = abi::EntitiesV1 {
    size: size_of::<abi::EntitiesV1>(),
    on_entity_inside,
    view: entity_view,
    type_id: entity_type_id,
    change: entity_change,
    id: crate::entities::id,
    with_entity: crate::entities::with_entity,
    flags: crate::entities::flags,
    move_by: crate::entities::move_by,
    set_position: crate::entities::set_position,
    body: crate::entities::body,
    reset_fall_distance: crate::entities::reset_fall_distance,
    add_effect: crate::entities::add_effect,
    step_height: crate::entities::step_height,
    set_step_height: crate::entities::set_step_height,
    entities_in: crate::entities::entities_in,
    has_indirect_passenger: crate::entities::has_indirect_passenger,
    data: crate::entities::data,
    set_data: crate::entities::set_data,
    may_build: crate::entities::may_build,
    spawn_carrier: crate::entities::spawn_carrier,
    discard: crate::entities::discard,
    start_riding: crate::entities::start_riding,
    stop_riding: crate::entities::stop_riding,
    vehicle: crate::entities::vehicle,
    entities_matching: crate::entities::entities_matching,
    as_player: crate::entities::as_player,
    with_player_context: crate::entities::with_player_context,
    hurt: crate::entities::hurt,
    has_scoreboard_tag: crate::entities::has_scoreboard_tag,
    players: crate::entities::players,
    age: crate::entities::age,
    hurt_by: crate::entities::hurt_by,
    remove_effect: crate::entities::remove_effect,
    attribute_value: crate::entities::attribute_value,
    ticks_frozen: crate::entities::ticks_frozen,
    set_ticks_frozen: crate::entities::set_ticks_frozen,
    spawn_projectile: crate::entities::spawn_projectile,
    projectile_owner: crate::entities::projectile_owner,
    can_freeze: crate::entities::can_freeze,
    shulker_bullet_target: crate::entities::shulker_bullet_target,
    player_inventory_item: crate::entities::player_inventory_item,
    player_inventory_stack: crate::entities::player_inventory_stack,
    player_fall_flying: crate::entities::player_fall_flying,
    set_player_fall_flying: crate::entities::set_player_fall_flying,
    player_flight_boost: crate::entities::player_flight_boost,
    player_selected_slot: crate::entities::player_selected_slot,
    can_move_by: crate::entities::can_move_by,
    player_item_cooldown: crate::entities::player_item_cooldown,
    player_damage_held_item: crate::entities::player_damage_held_item,
    player_reset_attack: crate::entities::player_reset_attack,
    is_attackable: crate::entities::is_attackable,
    broadcast_event: crate::entities::broadcast_event,
    remove_all_effects: crate::entities::remove_all_effects,
    set_custom_name: crate::entities::set_custom_name,
    player_fall_state: crate::entities::player_fall_state,
    game_events: crate::entities::game_events,
    on_item_destroyed: crate::entities::on_item_destroyed,
    mob_geometry: crate::mobs::mob_geometry,
    mob_spawn_clear: crate::mobs::mob_spawn_clear,
    eye_height: entity_eye_height,
    with_entity_uuid: crate::entities::with_entity_uuid,
};

unsafe extern "C" fn on_entity_inside(reg: Reg, callback: abi::EntityInside) -> i32 {
    if RUNTIME.get().is_some() {
        return code::INVALID_DEFINITION;
    }
    // SAFETY: registration is live for the call.
    let slot = &mut unsafe { native::staging(reg) }.content_hooks.entity_inside;
    if slot.is_some() {
        return code::DUPLICATE_ID;
    }
    *slot = Some(callback);
    code::OK
}

/// # Safety
/// `raw` points at the `&dyn EntityBase` a callback passed, still live.
pub const unsafe fn entity<'a>(raw: abi::EntityRef) -> Option<&'a dyn EntityBase> {
    // SAFETY: the caller passes a pointer to a live `&dyn EntityBase`.
    match unsafe { raw.cast::<&dyn EntityBase>().as_ref() } {
        Some(entity) => Some(*entity),
        None => None,
    }
}

unsafe extern "C" fn entity_view(raw: abi::EntityRef, out: *mut abi::EntityView) -> bool {
    // SAFETY: the entity is live for the callback.
    let (Some(base), false) = (unsafe { entity(raw) }, out.is_null()) else {
        return false;
    };
    let entity = base.get_entity();
    let mut kind = 0;
    if entity.entity_type == &EntityType::PLAYER {
        kind |= abi::ENTITY_PLAYER;
    }
    if base.get_living_entity().is_some() {
        kind |= abi::ENTITY_LIVING;
    }
    if base.get_item_entity().is_some() {
        kind |= abi::ENTITY_ITEM;
    }
    let (pos, motion) = (entity.pos.load(), entity.velocity.load());
    let view = abi::EntityView {
        kind,
        on_ground: entity.on_ground.load(Ordering::Relaxed),
        position: [pos.x, pos.y, pos.z],
        motion: [motion.x, motion.y, motion.z],
        yaw: entity.yaw.load(),
        pitch: entity.pitch.load(),
        head_yaw: entity.head_yaw.load(),
        body_yaw: entity.body_yaw.load(),
    };
    // SAFETY: out is a live, aligned EntityView owned by the caller.
    unsafe { out.write(view) };
    true
}

unsafe extern "C" fn entity_eye_height(raw: abi::EntityRef) -> f64 {
    // SAFETY: the entity is borrowed for the duration of the call.
    unsafe { entity(raw) }.map_or(f64::NAN, |base| base.get_entity().get_eye_height())
}

unsafe extern "C" fn entity_type_id(raw: abi::EntityRef) -> Str {
    // SAFETY: the entity is live for the callback.
    let name =
        unsafe { entity(raw) }.map_or("", |base| base.get_entity().entity_type.resource_name);
    Str::new(name.as_bytes())
}

unsafe extern "C" fn entity_change(raw: abi::EntityRef, change: *const abi::EntityChange) -> bool {
    // SAFETY: the entity is live for the callback and the change is borrowed for this call.
    let (Some(base), Some(change)) = (unsafe { entity(raw) }, unsafe { change.as_ref() }) else {
        return false;
    };
    let entity = base.get_entity();
    if change.mask & abi::SET_MOTION != 0 {
        let [x, y, z] = change.motion;
        if !(x.is_finite() && y.is_finite() && z.is_finite()) {
            return false;
        }
        entity.velocity.store(Vector3::new(x, y, z));
        entity.velocity_dirty.store(true, Ordering::Relaxed);
        // hurtMarked: a player's own client gets the motion too.
        entity.mark_hurt();
    }
    if change.mask & abi::SET_YAW != 0 {
        entity.yaw.store(change.yaw);
    }
    if change.mask & abi::SET_HEAD_YAW != 0 {
        entity.head_yaw.store(change.head_yaw);
    }
    if change.mask & abi::SET_BODY_YAW != 0 {
        entity.body_yaw.store(change.body_yaw);
    }
    if change.mask & abi::SET_ON_GROUND != 0 {
        entity.on_ground.store(change.on_ground, Ordering::Relaxed);
    }
    true
}

fn use_block_callback(block: &Block) -> Option<abi::UseBlockFn> {
    let runtime = RUNTIME.get()?;
    let owner = runtime.blocks.get(&block.id.as_u16())?.owner;
    runtime.use_block.get(&owner).copied()
}

fn use_context(
    world: &World,
    position: BlockPos,
    hit: &BlockHitResult<'_>,
    player: &Player,
    main_hand: bool,
    with_item: bool,
) -> abi::UseBlock {
    abi::UseBlock {
        position: abi_pos(position),
        state: u32::from(world.get_block_state_id(&position).as_u16()),
        face: hit.face.to_index(),
        hit: [hit.cursor_pos.x, hit.cursor_pos.y, hit.cursor_pos.z],
        sneaking: player.get_entity().is_sneaking(),
        creative: player.is_creative(),
        main_hand,
        with_item,
    }
}

fn redstone_callbacks(block: &Block) -> Option<(abi::RedstoneSignalFn, abi::RedstoneConnectsFn)> {
    RUNTIME.get().and_then(|runtime| {
        let owner = runtime.blocks.get(&block.id.as_u16())?.owner;
        runtime.redstone.get(&owner).copied()
    })
}

/// Vanilla `getSignal` runs whatever `canConnectRedstone` says; that only decides wire shapes.
fn redstone_signal(args: &GetRedstonePowerArgs<'_>, direct: bool) -> u8 {
    let Some((signal, _)) = redstone_callbacks(args.block) else {
        return 0;
    };
    let world: &World = args.world;
    // SAFETY: the world outlives this synchronous callback.
    unsafe {
        signal(
            ptr::from_ref(world).cast(),
            abi_pos(*args.position),
            u32::from(args.state.id.as_u16()),
            args.direction.to_index(),
            direct,
        )
    }
    .min(15)
}

pub unsafe extern "C" fn on_redstone(
    reg: Reg,
    signal: abi::RedstoneSignalFn,
    connects: abi::RedstoneConnectsFn,
) -> i32 {
    if RUNTIME.get().is_some() {
        return code::INVALID_DEFINITION;
    }
    // SAFETY: registration is live for the call.
    let slot = &mut unsafe { native::staging(reg) }.content_hooks.redstone;
    if slot.is_some() {
        return code::DUPLICATE_ID;
    }
    *slot = Some((signal, connects));
    code::OK
}

pub unsafe extern "C" fn on_player_will_destroy(
    reg: Reg,
    callback: abi::PlayerWillDestroyFn,
) -> i32 {
    if RUNTIME.get().is_some() {
        return code::INVALID_DEFINITION;
    }
    // SAFETY: registration is live for the call.
    let slot = &mut unsafe { native::staging(reg) }
        .content_hooks
        .player_will_destroy;
    if slot.is_some() {
        return code::DUPLICATE_ID;
    }
    *slot = Some(callback);
    code::OK
}

pub unsafe extern "C" fn on_use_block_content(reg: Reg, callback: abi::UseBlockFn) -> i32 {
    if RUNTIME.get().is_some() {
        return code::INVALID_DEFINITION;
    }
    // SAFETY: registration is live for the call.
    let slot = &mut unsafe { native::staging(reg) }.content_hooks.use_block;
    if slot.is_some() {
        return code::DUPLICATE_ID;
    }
    *slot = Some(callback);
    code::OK
}

/// The owning mod's modifier for a direct melee strike with this item.
pub fn melee_damage_callback(item: u16) -> Option<abi::MeleeDamageFn> {
    let runtime = RUNTIME.get()?;
    runtime
        .melee_damage
        .get(runtime.item_owners.get(&item)?)
        .copied()
}

pub unsafe extern "C" fn on_melee_damage(reg: Reg, callback: abi::MeleeDamageFn) -> i32 {
    if RUNTIME.get().is_some() {
        return code::INVALID_DEFINITION;
    }
    // SAFETY: registration is live for the call.
    let slot = &mut unsafe { native::staging(reg) }.content_hooks.melee_damage;
    if slot.is_some() {
        return code::DUPLICATE_ID;
    }
    *slot = Some(callback);
    code::OK
}

pub fn use_item_callback(item: u16) -> Option<abi::UseItemFn> {
    let runtime = RUNTIME.get()?;
    runtime
        .use_item
        .get(runtime.item_owners.get(&item)?)
        .copied()
}

pub unsafe extern "C" fn on_player_placed(reg: Reg, callback: abi::PlayerPlacedFn) -> i32 {
    if RUNTIME.get().is_some() {
        return code::INVALID_DEFINITION;
    }
    // SAFETY: registration is live for the call.
    let slot = &mut unsafe { native::staging(reg) }.content_hooks.player_placed;
    if slot.is_some() {
        return code::DUPLICATE_ID;
    }
    *slot = Some(callback);
    code::OK
}

pub unsafe extern "C" fn on_use_item(reg: Reg, callback: abi::UseItemFn) -> i32 {
    if RUNTIME.get().is_some() {
        return code::INVALID_DEFINITION;
    }
    // SAFETY: registration is live for the call.
    let slot = &mut unsafe { native::staging(reg) }.content_hooks.use_item;
    if slot.is_some() {
        return code::DUPLICATE_ID;
    }
    *slot = Some(callback);
    code::OK
}

pub fn item_use_callback(item: u16) -> Option<abi::ItemUseFn> {
    let runtime = RUNTIME.get()?;
    runtime
        .item_use
        .get(runtime.item_owners.get(&item)?)
        .copied()
}

pub unsafe extern "C" fn on_item_use(reg: Reg, callback: abi::ItemUseFn) -> i32 {
    if RUNTIME.get().is_some() {
        return code::INVALID_DEFINITION;
    }
    // SAFETY: registration is live for this call.
    let slot = &mut unsafe { native::staging(reg) }.content_hooks.item_use;
    if slot.is_some() {
        return code::DUPLICATE_ID;
    }
    *slot = Some(callback);
    code::OK
}

pub unsafe extern "C" fn item_use_animation(reg: Reg, item: Str, ticks: u16) -> i32 {
    if RUNTIME.get().is_some() || ticks == 0 {
        return code::INVALID_DEFINITION;
    }
    // SAFETY: the registrar and item ID are borrowed for this call.
    let (staging, item) = unsafe { (native::staging(reg), item.str()) };
    let Some(definition) = staging
        .content_items
        .iter_mut()
        .find(|definition| definition.id == item)
    else {
        return code::INVALID_DEFINITION;
    };
    if definition.use_animation.is_some() {
        return code::DUPLICATE_ID;
    }
    definition.use_animation = Some(ticks);
    code::OK
}

/// For a content item, call its owner. For a vanilla item, call the opt-in interceptors.
pub fn use_on_block_callbacks(item: u16) -> impl Iterator<Item = abi::UseOnBlockFn> {
    let runtime = RUNTIME.get();
    let owner =
        runtime.and_then(|runtime| runtime.use_on_block.get(runtime.item_owners.get(&item)?));
    let vanilla = runtime
        .filter(|_| item < Item::VANILLA_COUNT)
        .into_iter()
        .flat_map(|runtime| runtime.vanilla_use_on_block.values());
    owner.into_iter().chain(vanilla).copied()
}

pub unsafe extern "C" fn on_use_on_block(reg: Reg, callback: abi::UseOnBlockFn) -> i32 {
    if RUNTIME.get().is_some() {
        return code::INVALID_DEFINITION;
    }
    // SAFETY: registration is live for the call.
    let slot = &mut unsafe { native::staging(reg) }.content_hooks.use_on_block;
    if slot.is_some() {
        return code::DUPLICATE_ID;
    }
    *slot = Some(callback);
    code::OK
}

pub unsafe extern "C" fn on_vanilla_use_on_block(reg: Reg, callback: abi::UseOnBlockFn) -> i32 {
    if RUNTIME.get().is_some() {
        return code::INVALID_DEFINITION;
    }
    // SAFETY: registration is live for the call.
    let slot = &mut unsafe { native::staging(reg) }
        .content_hooks
        .vanilla_use_on_block;
    if slot.is_some() {
        return code::DUPLICATE_ID;
    }
    *slot = Some(callback);
    code::OK
}

#[cfg(test)]
mod tests {
    use moddedmc_api::abi::{Sink, Str};
    use pumpkin_data::Block;

    use super::{block_item, describe_state, state};

    unsafe extern "C" fn collect(ctx: *mut std::ffi::c_void, _key: u64, value: Str) {
        // SAFETY: ctx is the String the test passed, and value is live until we return.
        unsafe { (*ctx.cast::<String>()).push_str(value.str()) };
    }

    /// Every vanilla state's selector reads back as the same state.
    #[test]
    fn describe_state_round_trips() {
        let mut checked = 0;
        for block in [&Block::OAK_STAIRS, &Block::REDSTONE_WIRE, &Block::STONE] {
            for block_state in block.states {
                let mut text = String::new();
                let out = Sink {
                    ctx: (&raw mut text).cast(),
                    put: collect,
                };
                let id = u32::from(block_state.id.as_u16());
                // SAFETY: the sink writes into `text` for the call.
                assert!(unsafe { describe_state(id, out) });
                // SAFETY: the selector is borrowed for the call.
                assert_eq!(unsafe { state(Str::new(text.as_bytes())) }, id, "{text}");
                checked += 1;
            }
        }
        assert!(checked > 1000);
    }

    /// Crops name their seed or produce item, and blocks without an item give vanilla's air.
    #[test]
    fn block_item_matches_vanilla_as_item() {
        for (block, item) in [
            (&Block::CARROTS, "minecraft:carrot"),
            (&Block::WHEAT, "minecraft:wheat_seeds"),
            (&Block::OAK_STAIRS, "minecraft:oak_stairs"),
            (&Block::WATER, "minecraft:air"),
        ] {
            let mut text = String::new();
            let out = Sink {
                ctx: (&raw mut text).cast(),
                put: collect,
            };
            let id = u32::from(block.default_state.id.as_u16());
            // SAFETY: the sink writes into `text` for the call.
            assert!(unsafe { block_item(id, out) });
            assert_eq!(text, item, "{}", block.name);
        }
    }
}
