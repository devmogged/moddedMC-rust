use std::{
    borrow::Cow,
    collections::{BTreeMap, HashMap},
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
        BlockBehaviour, BlockHitResult, NormalUseArgs, OnEntityCollisionArgs, OnNeighborUpdateArgs,
        OnPlaceArgs, UseWithItemArgs,
        blocks::redstone::block_receives_redstone_power,
        entities::{
            BlockEntity,
            content::{self as engine, Change, ContentBlockEntity},
        },
        registry::{BlockActionResult, install_content_behaviour},
    },
    command::context::CommandContext,
    entity::{Entity, EntityBase, decoration::display::ItemDisplayEntity, player::Player},
    server::Server,
    world::World,
};
use pumpkin_data::{
    Block, BlockDirection, BlockState, BlockStateId, Property,
    content::{self, BlockDefinition, ItemDefinition, StateDefinition},
    data_component_impl::{EquipmentSlot, ItemModelImpl},
    entity::EntityType,
    item::Item,
    item_stack::ItemStack,
};
use pumpkin_nbt::Nbt;
use pumpkin_util::math::{position::BlockPos, vector3::Vector3};
use pumpkin_world::world::BlockFlags;
use uuid::Uuid;

use crate::{MAX_STATE_BYTES, content_pack, interaction, load_mods, lock, native};

pub static CONTENT: abi::ContentV1 = abi::ContentV1 {
    size: size_of::<abi::ContentV1>(),
    add_block,
    state,
    on_tick,
};
pub static ITEMS: abi::ItemsV1 = abi::ItemsV1 {
    size: size_of::<abi::ItemsV1>(),
    add_item,
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
};

const MAX_BATCH: usize = 4096;

pub struct Registration {
    id: String,
    name: String,
    entity: String,
    item_model: String,
    states: Vec<RegisteredState>,
    placement: Option<abi::ContentPlacement>,
}

pub struct ItemRegistration {
    id: String,
    name: String,
    carrier: &'static Item,
    model: String,
}

struct RegisteredState {
    properties: BTreeMap<String, String>,
    physical: &'static BlockState,
    shape: Vec<pumpkin_util::math::boundingbox::BoundingBox>,
    visual: content_pack::StateAssets,
}

struct RuntimeBlock {
    owner: usize,
    placement: Option<abi::ContentPlacement>,
}

struct Runtime {
    blocks: HashMap<u16, RuntimeBlock>,
    overlays: HashMap<u16, content_pack::OverlayModel>,
    ticks: BTreeMap<usize, abi::ContentTick>,
    entity_inside: BTreeMap<usize, abi::EntityInside>,
    use_on_block: BTreeMap<usize, abi::UseOnBlockFn>,
    use_block: BTreeMap<usize, abi::UseBlockFn>,
    /// Owning mod of each content item, block items included.
    item_owners: HashMap<u16, usize>,
}

/// Callbacks one mod stages during registration.
#[derive(Clone, Copy, Default)]
pub struct Hooks {
    pub tick: Option<abi::ContentTick>,
    pub entity_inside: Option<abi::EntityInside>,
    pub use_on_block: Option<abi::UseOnBlockFn>,
    pub use_block: Option<abi::UseBlockFn>,
}

#[derive(Default)]
struct Pending {
    blocks: Vec<(usize, Registration)>,
    items: Vec<(usize, ItemRegistration)>,
    ticks: BTreeMap<usize, abi::ContentTick>,
    entity_inside: BTreeMap<usize, abi::EntityInside>,
    use_on_block: BTreeMap<usize, abi::UseOnBlockFn>,
    use_block: BTreeMap<usize, abi::UseBlockFn>,
}
static PENDING: Mutex<Pending> = Mutex::new(Pending {
    blocks: Vec::new(),
    items: Vec::new(),
    ticks: BTreeMap::new(),
    entity_inside: BTreeMap::new(),
    use_on_block: BTreeMap::new(),
    use_block: BTreeMap::new(),
});
static RUNTIME: OnceLock<Runtime> = OnceLock::new();
static WORLDS: LazyLock<Mutex<HashMap<Uuid, WorldState>>> = LazyLock::new(Mutex::default);

#[derive(Default)]
struct WorldState {
    events: Vec<(usize, ContentEvent)>,
    blocks: HashMap<BlockPos, BlockStateId>,
    displays: HashMap<BlockPos, Overlay>,
    rotation: HashMap<BlockPos, (u8, f32)>,
}

struct Overlay {
    entity: Arc<ItemDisplayEntity>,
    phase: f32,
    state: BlockStateId,
    moving: bool,
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
    if RUNTIME.get().is_some() || def.states.is_null() || !(1..=256).contains(&def.count) {
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
            physical,
            shape,
            visual: content_pack::StateAssets {
                model: model.into(),
                rotating_model: rotating.into(),
                x: state.x,
                y: state.y,
                orientation: state.orientation,
            },
        });
    }
    staging.content.push(Registration {
        id: id.into(),
        name: name.into(),
        entity: if entity.is_empty() {
            id.into()
        } else {
            entity.into()
        },
        item_model: item_model.into(),
        states: registered,
        placement: def.on_place,
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
    });
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
    if let Some(inside) = hooks.entity_inside {
        pending.entity_inside.insert(owner, inside);
    }
    if let Some(callback) = hooks.use_on_block {
        pending.use_on_block.insert(owner, callback);
    }
    if let Some(callback) = hooks.use_block {
        pending.use_block.insert(owner, callback);
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
                },
                content_pack::ItemAssets {
                    id: item.id,
                    model: item.model,
                },
            )
        })
        .unzip()
}

/// Engine definitions for the staged blocks, each state on its own note block carrier.
fn block_definitions(blocks: &[(usize, Registration)]) -> io::Result<Vec<BlockDefinition>> {
    // Preserve instrument and note for ordinary note blocks by borrowing only powered variants.
    let mut carriers = Block::NOTE_BLOCK.states.iter().filter_map(|state| {
        let mut props = Block::NOTE_BLOCK.properties(state.id)?.to_props();
        let powered = props.iter_mut().find(|p| p.key_str() == "powered")?;
        if powered.value_str() != "true" {
            return None;
        }
        *powered = Property::from_str_pair("powered", "false")?;
        let replacement = Block::NOTE_BLOCK
            .from_properties(&props)
            .to_state_id(&Block::NOTE_BLOCK);
        Some((state.id, replacement))
    });
    let mut definitions = Vec::with_capacity(blocks.len());
    for (_, block) in blocks {
        let mut states = Vec::with_capacity(block.states.len());
        for state in &block.states {
            let (carrier, replacement) = carriers
                .next()
                .ok_or_else(|| io::Error::other("custom block carrier slots exhausted"))?;
            states.push(StateDefinition {
                properties: state.properties.clone(),
                physical: state.physical,
                carrier,
                carrier_replacement: Some(replacement),
                shape: state.shape.clone(),
            });
        }
        definitions.push(BlockDefinition {
            name: block.id.clone(),
            display_name: block.name.clone(),
            entity: Some(block.entity.clone()),
            item_carrier: &Item::PAPER,
            states,
        });
    }
    Ok(definitions)
}

pub fn open_world(path: &Path, config: &mut AdvancedConfiguration) -> io::Result<()> {
    load_mods();
    let Pending {
        mut blocks,
        mut items,
        ticks,
        entity_inside,
        use_on_block,
        use_block,
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

    let definitions = block_definitions(&blocks)?;
    let (item_definitions, item_assets) = split_items(items);
    let registry = content::Registry::open(
        &path.join("moddedmc/registry.dat"),
        &definitions,
        &item_definitions,
    )?;
    for item in &item_assets {
        tracing::info!("moddedmc: registered content item {}", item.id);
    }
    let mut runtime = Runtime {
        blocks: HashMap::new(),
        overlays: HashMap::new(),
        ticks,
        entity_inside,
        use_on_block,
        use_block,
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
                placement: block.placement,
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
    if RUNTIME.set(runtime).is_err()
        || !install_content_behaviour(Arc::new(Behaviour))
        || !crate::interaction::install()
        || !engine::observe(observe)
    {
        return Err(io::Error::other("content hooks already installed"));
    }
    Ok(())
}

struct Behaviour;

impl BlockBehaviour for Behaviour {
    fn on_place(&self, args: OnPlaceArgs<'_>) -> BlockStateId {
        let default = args.block.default_state.id;
        let Some(callback) = RUNTIME
            .get()
            .and_then(|r| r.blocks.get(&args.block.id.as_u16()))
            .and_then(|block| block.placement)
        else {
            return default;
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
            .unwrap_or(default)
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
        match interaction::use_block(callback, args.world, args.player, args.item_stack, &context) {
            abi::USE_SUCCESS => BlockActionResult::Success,
            abi::USE_CONSUME => BlockActionResult::Consume,
            abi::USE_FAIL => BlockActionResult::Fail,
            abi::USE_PASS => BlockActionResult::Pass,
            _ => BlockActionResult::PassToDefaultBlockAction,
        }
    }

    fn normal_use(&self, args: NormalUseArgs<'_>) -> BlockActionResult {
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
        match interaction::use_block(callback, args.world, args.player, &mut empty, &context) {
            abi::USE_SUCCESS => BlockActionResult::Success,
            abi::USE_CONSUME => BlockActionResult::Consume,
            abi::USE_FAIL => BlockActionResult::Fail,
            _ => BlockActionResult::Pass,
        }
    }

    fn on_neighbor_update(&self, args: OnNeighborUpdateArgs<'_>) {
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
        Change::Loaded(state) => queue(world, pos, BlockStateId::AIR, state, abi::LOADED),
        Change::Unloaded => {
            let mut worlds = lock(&WORLDS);
            if let Some(w) = worlds.get_mut(&world.uuid)
                && let Some(old) = w.blocks.remove(&pos)
            {
                w.rotation.remove(&pos);
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
    let mut worlds = lock(&WORLDS);
    let w = worlds.entry(world.uuid).or_default();
    if after.is_some() {
        w.blocks.insert(pos, state);
    } else {
        w.blocks.remove(&pos);
        w.rotation.remove(&pos);
    }
    let event = event(pos, old, state, reason);
    if let Some(before) = before.filter(|before| Some(*before) != after) {
        w.events.push((before, event));
    }
    if let Some(after) = after {
        w.events.push((after, event));
    }
}

pub fn tick(server: &Arc<Server>, time: u64) {
    let Some(runtime) = RUNTIME.get() else { return };
    if runtime.blocks.is_empty() {
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
            // SAFETY: the world and event array are live for this synchronous callback.
            unsafe {
                callback(
                    ptr::from_ref(world.as_ref()).cast(),
                    time,
                    events.as_ptr(),
                    events.len(),
                );
            };
        }
        render(world, runtime, time);
    }
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
    let Some((blocks, rotation, mut displays)) = ({
        lock(&WORLDS).get_mut(&world.uuid).map(|w| {
            (
                w.blocks.clone(),
                w.rotation.clone(),
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
            let center = Vector3::new(
                f64::from(pos.0.x) + 0.5,
                f64::from(pos.0.y) + 0.5,
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
            display.set_item(stack);
            display.display.set_right_rotation(model.orientation);
            world.spawn_entity_non_save(display.clone());
            Overlay {
                entity: display,
                phase: 0.0,
                state,
                moving: true,
            }
        });
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
            let flags = BlockFlags::from_bits(u32::from(update.flags & abi::UPDATE_ALL))?;
            if update.flags & !abi::UPDATE_ALL != 0 {
                return None;
            }
            world.get_block_state_id_if_loaded(&pos)?;
            Some((pos, state, flags))
        })
        .collect::<Option<Vec<_>>>()
    else {
        return 0;
    };
    for (pos, state, flags) in validated {
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

unsafe extern "C" fn write_entity(raw: WorldRef, pos: Pos, nbt: Str) -> bool {
    if nbt.len > MAX_STATE_BYTES {
        return false;
    }
    // SAFETY: the world and NBT are live for this call.
    let (Some(world), bytes) = (unsafe { world(raw) }, unsafe { nbt.bytes() }) else {
        return false;
    };
    let mut reader = std::io::Cursor::new(bytes);
    let (Ok(data), pos) = (
        Nbt::read(&mut pumpkin_nbt::deserializer::NbtReadHelperJava::new(
            &mut reader,
        )),
        block_pos(pos),
    ) else {
        return false;
    };
    if reader.position() != bytes.len() as u64 {
        return false;
    }
    let Some(entity) = world.get_block_entity(&pos) else {
        return false;
    };
    let Some(entity) = entity.as_any().downcast_ref::<ContentBlockEntity>() else {
        return false;
    };
    entity.set_data(data.root_tag);
    let mut saved = pumpkin_nbt::compound::NbtCompound::new();
    entity.write_internal(&mut saved);
    world.add_block_entity_nbt(pos, &saved);
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
            .is_some_and(|r| r.overlays.contains_key(&state.as_u16()))
    }) {
        return false;
    }
    w.rotation.insert(pos, (axis, rpm));
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

/// The mod callback for using a content item on a block.
pub fn use_on_block_callback(item: u16) -> Option<abi::UseOnBlockFn> {
    let runtime = RUNTIME.get()?;
    runtime
        .use_on_block
        .get(runtime.item_owners.get(&item)?)
        .copied()
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
