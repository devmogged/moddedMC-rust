//! moddedMC-rust content host.
//!
//! Content mods are Pumpkin v0.1 plugin components that also import
//! `moddedmc:mods/registry` and export `moddedmc:mods/content` (see `wit/mods`).
//! The host owns the registry, keeps each placed block's identity and state as
//! custom data on a vanilla stand-in block entity (saved by Pumpkin with the
//! chunk), and delivers due block entities to each mod in one batch per tick.

mod bindings {
    wasmtime::component::bindgen!({
        path: "../../wit/mods",
        world: "content-mod",
    });
}

mod console;
mod stats;

use std::{
    any::Any,
    collections::{HashMap, HashSet},
    sync::{
        Arc, LazyLock, Mutex, MutexGuard, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard,
        Weak,
    },
    time::Instant,
};

use bindings::moddedmc::mods::{
    registry,
    types::{
        self, BlockDef, BlockEntityDef, BlockPos as WitPos, BreakInput, PlaceInput,
        RegistrationError, TickInput, TickUpdate, UseInput, UseResult,
    },
};
use pumpkin_core::{
    plugin::{
        BoxFuture, Cancellable, EventHandler, EventPriority,
        block::{block_break::BlockBreakEvent, block_place::BlockPlaceEvent},
        player::player_interact_event::PlayerInteractEvent,
        server::{server_command::ServerCommandEvent, server_tick_start::ServerTickStartEvent},
    },
    server::Server,
    world::World,
};
use pumpkin_data::{Block, item::Item, item_stack::ItemStack};
use pumpkin_nbt::tag::NbtTag;
use pumpkin_util::{math::position::BlockPos, text::TextComponent};
use pumpkin_wasm_host_common::{
    extension::{self, HostExtension},
    plugin::WasmPlugin,
    state::PluginHostState,
};
use wasmtime::{
    Store,
    component::{ComponentNamedList, Func, HasSelf, Instance, Lift, Linker, Lower},
};

/// Custom-data namespace used on block entities and item stacks.
pub const NAMESPACE: &str = "moddedmc";
/// Item custom-data key holding the block id the item places.
pub const ITEM_BLOCK_KEY: &str = "block";
const KEY_ID: &str = "id";
const KEY_VERSION: &str = "v";
const KEY_STATE: &str = "s";
const CONTENT_EXPORT: &str = "moddedmc:mods/content@0.1.0";
/// Vanilla blocks with a block entity that 0.1 accepts as stand-ins.
const STAND_INS: [&str; 3] = ["barrel", "dropper", "dispenser"];
/// How often (ticks) loaded custom blocks are re-indexed, which also picks up reloaded chunks.
const DISCOVERY_INTERVAL: i32 = 20;
const MAX_STATE_BYTES: usize = 64 * 1024;

struct ContentExports {
    register: Func,
    tick: Func,
    use_block: Func,
    place_block: Func,
    break_block: Func,
}

struct ContentMod {
    plugin: Weak<WasmPlugin>,
    exports: Arc<ContentExports>,
    registered: bool,
}

struct EntityType {
    id: String,
    mod_idx: usize,
    state_version: u32,
    initial_state: Vec<u8>,
    tick_interval: Option<u32>,
}

struct BlockType {
    id: String,
    display_name: String,
    stand_in: &'static Block,
    entity_type: usize,
}

#[derive(Default)]
struct Registry {
    entity_types: Vec<EntityType>,
    blocks: Vec<BlockType>,
}

impl Registry {
    fn block(&self, id: &str) -> Option<usize> {
        self.blocks.iter().position(|b| b.id == id)
    }
}

/// Definitions a mod has added during its `register` call, committed only if it succeeds.
struct Staging {
    mod_idx: usize,
    entity_types: Vec<EntityType>,
    blocks: Vec<BlockType>,
}

/// Loaded custom blocks of one world.
struct WorldIndex {
    world: Weak<World>,
    /// Live block entities: entity type -> one position set per tick phase
    /// (`phase(pos, interval)`), so a tick only visits the entities due on it.
    by_type: HashMap<usize, Vec<HashSet<BlockPos>>>,
}

/// (world, pos, block, player)
type PendingPlace = (Weak<World>, BlockPos, usize, Option<String>);

#[derive(Default)]
struct Host {
    mods: Mutex<Vec<ContentMod>>,
    registry: RwLock<Registry>,
    staging: Mutex<Option<Staging>>,
    /// Placed custom blocks whose block entity appears later this tick: (world, pos, block, player).
    pending_places: Mutex<Vec<PendingPlace>>,
    index: Mutex<Vec<WorldIndex>>,
    /// Console requests waiting for the next tick.
    console: Mutex<Vec<console::Request>>,
    /// Last (tick, position) a player used, so one click fired for both hands acts once.
    last_use: Mutex<HashMap<String, (i32, BlockPos)>>,
}

static HOST: LazyLock<Host> = LazyLock::new(Host::default);

// A panic elsewhere must not take the content host down with it.
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}
fn read<T>(m: &RwLock<T>) -> RwLockReadGuard<'_, T> {
    m.read().unwrap_or_else(PoisonError::into_inner)
}
fn write<T>(m: &RwLock<T>) -> RwLockWriteGuard<'_, T> {
    m.write().unwrap_or_else(PoisonError::into_inner)
}

/// Installs the WIT extension. Call before the plugin runtime is created.
pub fn install() {
    extension::install(HostExtension {
        add_to_linker,
        on_instantiate,
        on_plugin_ready,
    });
}

/// Hooks the host into the server's tick and block events. Call before plugins load.
pub fn attach(server: &Arc<Server>) {
    let pm = &server.plugin_manager;
    pm.register::<ServerTickStartEvent, _>(Arc::new(TickHandler), EventPriority::Normal, true);
    pm.register::<BlockPlaceEvent, _>(Arc::new(PlaceHandler), EventPriority::Normal, true);
    pm.register::<BlockBreakEvent, _>(Arc::new(BreakHandler), EventPriority::Normal, true);
    pm.register::<PlayerInteractEvent, _>(Arc::new(UseHandler), EventPriority::Normal, true);
    pm.register::<ServerCommandEvent, _>(
        Arc::new(console::ConsoleHandler),
        EventPriority::Normal,
        true,
    );
    stats::attach(server);
}

fn add_to_linker(linker: &mut Linker<PluginHostState>) -> wasmtime::Result<()> {
    registry::add_to_linker::<_, HasSelf<_>>(linker, |state: &mut PluginHostState| state)
}

fn on_instantiate(
    instance: &Instance,
    store: &mut Store<PluginHostState>,
) -> Option<Arc<dyn Any + Send + Sync>> {
    let interface = instance.get_export_index(&mut *store, None, CONTENT_EXPORT)?;
    let mut func = |name: &str| {
        let index = instance.get_export_index(&mut *store, Some(&interface), name)?;
        instance.get_func(&mut *store, index)
    };
    let exports = ContentExports {
        register: func("register")?,
        tick: func("tick-block-entities")?,
        use_block: func("use-block")?,
        place_block: func("place-block")?,
        break_block: func("break-block")?,
    };
    Some(Arc::new(exports))
}

fn on_plugin_ready(plugin: &Arc<WasmPlugin>, data: Arc<dyn Any + Send + Sync>) {
    let Ok(exports) = data.downcast::<ContentExports>() else {
        return;
    };
    lock(&HOST.mods).push(ContentMod {
        plugin: Arc::downgrade(plugin),
        exports,
        registered: false,
    });
}

// ---------------------------------------------------------------- registry imports

impl types::Host for PluginHostState {}

impl registry::Host for PluginHostState {
    fn add_block_entity(&mut self, def: BlockEntityDef) -> Result<u32, RegistrationError> {
        let mut staging = lock(&HOST.staging);
        let staging = staging.as_mut().ok_or(RegistrationError::RegistryFrozen)?;
        if !valid_id(&def.id) || def.tick_interval == Some(0) {
            return Err(RegistrationError::InvalidDefinition);
        }
        if def.initial_state.len() > MAX_STATE_BYTES {
            return Err(RegistrationError::InvalidDefinition);
        }
        let registry = read(&HOST.registry);
        if registry
            .entity_types
            .iter()
            .chain(&staging.entity_types)
            .any(|t| t.id == def.id)
        {
            return Err(RegistrationError::DuplicateId);
        }
        let index = registry.entity_types.len() + staging.entity_types.len();
        staging.entity_types.push(EntityType {
            id: def.id,
            mod_idx: staging.mod_idx,
            state_version: def.state_version,
            initial_state: def.initial_state,
            tick_interval: def.tick_interval,
        });
        u32::try_from(index).map_err(|_| RegistrationError::InvalidDefinition)
    }

    fn add_block(&mut self, def: BlockDef) -> Result<(), RegistrationError> {
        let mut staging = lock(&HOST.staging);
        let staging = staging.as_mut().ok_or(RegistrationError::RegistryFrozen)?;
        if !valid_id(&def.id) {
            return Err(RegistrationError::InvalidDefinition);
        }
        let registry = read(&HOST.registry);
        if registry.block(&def.id).is_some() || staging.blocks.iter().any(|b| b.id == def.id) {
            return Err(RegistrationError::DuplicateId);
        }
        let stand_in_name = def
            .stand_in
            .strip_prefix("minecraft:")
            .unwrap_or(&def.stand_in);
        let stand_in = STAND_INS
            .contains(&stand_in_name)
            .then(|| Block::from_name(stand_in_name))
            .flatten()
            .ok_or(RegistrationError::UnsupportedStandIn)?;
        // 0.1 keeps identity on the stand-in's block entity, so every block needs one.
        let entity_id = def
            .block_entity
            .ok_or(RegistrationError::InvalidDefinition)?;
        let entity_type = staging
            .entity_types
            .iter()
            .position(|t| t.id == entity_id)
            .map(|i| i + registry.entity_types.len())
            .ok_or(RegistrationError::UnknownEntityType)?;
        staging.blocks.push(BlockType {
            id: def.id,
            display_name: def.display_name,
            stand_in,
            entity_type,
        });
        Ok(())
    }
}

/// `namespace:path`, lowercase resource-location characters, and not the vanilla namespace.
fn valid_id(id: &str) -> bool {
    let Some((namespace, path)) = id.split_once(':') else {
        return false;
    };
    let ns_ok = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || "_-.".contains(c);
    !namespace.is_empty()
        && !path.is_empty()
        && namespace != "minecraft"
        && namespace.chars().all(ns_ok)
        && path.chars().all(|c| ns_ok(c) || c == '/')
}

// ---------------------------------------------------------------- tick

struct TickHandler;

impl EventHandler<ServerTickStartEvent> for TickHandler {
    fn handle_blocking<'a>(
        &'a self,
        server: &'a Arc<Server>,
        event: &'a mut ServerTickStartEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            register_new_mods().await;
            apply_pending_places().await;
            console::run_queued(server).await;
            if event.tick % DISCOVERY_INTERVAL == 0 {
                let started = Instant::now();
                discover(server);
                stats::TIMING.add_discover(started);
            }
            tick_due(server, event.tick).await;
        })
    }
}

async fn register_new_mods() {
    let pending: Vec<(usize, Arc<WasmPlugin>, Arc<ContentExports>)> = {
        let mods = lock(&HOST.mods);
        mods.iter()
            .enumerate()
            .filter(|(_, m)| !m.registered)
            .filter_map(|(i, m)| Some((i, m.plugin.upgrade()?, m.exports.clone())))
            .collect()
    };
    for (mod_idx, plugin, exports) in pending {
        lock(&HOST.mods)[mod_idx].registered = true;
        *lock(&HOST.staging) = Some(Staging {
            mod_idx,
            entity_types: Vec::new(),
            blocks: Vec::new(),
        });
        let func = exports.register;
        let result = plugin
            .store
            .call_guest(move |mut guest| {
                Box::pin(async move {
                    guest
                        .call_func::<(), (Result<(), RegistrationError>,)>(func, ())
                        .await
                        .map(|(result,)| result)
                })
            })
            .await;
        let staging = lock(&HOST.staging).take();
        match (result, staging) {
            (Ok(Ok(())), Some(staging)) => {
                let mut registry = write(&HOST.registry);
                for t in &staging.entity_types {
                    tracing::info!("moddedmc: registered block entity {}", t.id);
                }
                for b in &staging.blocks {
                    tracing::info!(
                        "moddedmc: registered block {} (stand-in minecraft:{})",
                        b.id,
                        b.stand_in.name
                    );
                }
                registry.entity_types.extend(staging.entity_types);
                registry.blocks.extend(staging.blocks);
            }
            (Ok(Err(error)), _) => {
                tracing::error!(
                    "moddedmc: content mod #{mod_idx} rejected its registration: {error:?}"
                );
            }
            (Err(error), _) => {
                tracing::error!("moddedmc: content mod #{mod_idx} trapped in register: {error}");
            }
            (Ok(Ok(())), None) => {
                tracing::error!("moddedmc: content mod #{mod_idx} registration lost its staging");
            }
        }
    }
}

async fn apply_pending_places() {
    let pending = std::mem::take(&mut *lock(&HOST.pending_places));
    for (world, pos, block_idx, player) in pending {
        let Some(world) = world.upgrade() else {
            continue;
        };
        if adopt_placed(&world, pos, block_idx) {
            notify_placed(&world, pos, block_idx, player).await;
        }
    }
}

/// Tags a freshly placed stand-in with its custom block identity and indexes it.
fn adopt_placed(world: &Arc<World>, pos: BlockPos, block_idx: usize) -> bool {
    let registry = read(&HOST.registry);
    let block = &registry.blocks[block_idx];
    // The stand-in must have been placed and created its block entity.
    if world.get_block(&pos).id != block.stand_in.id || world.get_block_entity(&pos).is_none() {
        return false;
    }
    let entity = &registry.entity_types[block.entity_type];
    write_identity(world, &pos, block, entity);
    index_insert(world, pos, block.entity_type, phase_count(entity));
    true
}

/// Tells the owning mod a block was placed and applies the initial state it returns.
async fn notify_placed(
    world: &Arc<World>,
    pos: BlockPos,
    block_idx: usize,
    player: Option<String>,
) {
    let (entity_type, mod_idx, state) = {
        let registry = read(&HOST.registry);
        let entity_type = registry.blocks[block_idx].entity_type;
        let def = &registry.entity_types[entity_type];
        (entity_type, def.mod_idx, def.initial_state.clone())
    };
    let input = PlaceInput {
        entity_type: u32::try_from(entity_type).unwrap_or(u32::MAX),
        pos: wit_pos(&pos),
        dimension: world.dimension.minecraft_name.to_owned(),
        state,
        player_name: player,
    };
    let result = call_mod::<(PlaceInput,), (Option<Vec<u8>>,)>(
        mod_idx,
        |e| e.place_block,
        (input,),
        "place-block",
    )
    .await;
    if let Some((Some(state),)) = result
        && state.len() <= MAX_STATE_BYTES
    {
        write_state(world, &pos, &state);
    }
}

/// Calls one of a mod's `content` exports. A trap is logged and yields `None`.
async fn call_mod<P, R>(
    mod_idx: usize,
    pick: fn(&ContentExports) -> Func,
    params: P,
    what: &'static str,
) -> Option<R>
where
    P: ComponentNamedList + Lower + Send + 'static,
    R: ComponentNamedList + Lift + Send + 'static,
{
    let (plugin, exports) = mod_handle(mod_idx)?;
    let func = pick(&exports);
    let result = plugin
        .store
        .call_guest(move |mut guest| {
            Box::pin(async move { guest.call_func::<P, R>(func, params).await })
        })
        .await;
    result
        .inspect_err(|error| {
            tracing::error!("moddedmc: content mod #{mod_idx} trapped in {what}: {error}");
        })
        .ok()
}

fn write_identity(world: &World, pos: &BlockPos, block: &BlockType, entity: &EntityType) {
    world.set_block_entity_custom_data(
        pos,
        NAMESPACE,
        KEY_ID,
        NbtTag::String(block.id.as_str().into()),
    );
    world.set_block_entity_custom_data(
        pos,
        NAMESPACE,
        KEY_VERSION,
        NbtTag::Int(entity.state_version.cast_signed()),
    );
    write_state(world, pos, &entity.initial_state);
}

fn write_state(world: &World, pos: &BlockPos, state: &[u8]) {
    let bytes: Box<[i8]> = state.iter().map(|b| b.cast_signed()).collect();
    world.set_block_entity_custom_data(pos, NAMESPACE, KEY_STATE, NbtTag::ByteArray(bytes));
}

fn read_state(world: &World, pos: &BlockPos) -> Option<Vec<u8>> {
    match world.get_block_entity_custom_data(pos, NAMESPACE, KEY_STATE)? {
        NbtTag::ByteArray(bytes) => Some(bytes.iter().map(|b| b.cast_unsigned()).collect()),
        _ => None,
    }
}

fn read_block_id(world: &World, pos: &BlockPos) -> Option<String> {
    // Pumpkin loads a block entity's custom data only when the entity is woken.
    let _ = world.get_block_entity(pos);
    match world.get_block_entity_custom_data(pos, NAMESPACE, KEY_ID)? {
        NbtTag::String(id) => Some(id.into()),
        _ => None,
    }
}

/// Phases per entity type: one per tick of its interval (event-driven types use one).
fn phase_count(def: &EntityType) -> u32 {
    def.tick_interval.unwrap_or(1)
}

/// Stable per-position phase, so equal-interval block entities spread over the interval
/// instead of all ticking on the same tick.
fn phase(pos: &BlockPos, phases: u32) -> usize {
    let mix = (pos.0.x.cast_unsigned().wrapping_mul(73_856_093))
        ^ (pos.0.y.cast_unsigned().wrapping_mul(19_349_663))
        ^ (pos.0.z.cast_unsigned().wrapping_mul(83_492_791));
    (mix % phases.max(1)) as usize
}

fn insert_phased(
    by_type: &mut HashMap<usize, Vec<HashSet<BlockPos>>>,
    entity_type: usize,
    phases: u32,
    pos: BlockPos,
) {
    let buckets = by_type
        .entry(entity_type)
        .or_insert_with(|| vec![HashSet::new(); phases.max(1) as usize]);
    buckets[phase(&pos, phases)].insert(pos);
}

fn index_insert(world: &Arc<World>, pos: BlockPos, entity_type: usize, phases: u32) {
    let mut index = lock(&HOST.index);
    let i = index
        .iter()
        .position(|w| w.world.as_ptr() == Arc::as_ptr(world))
        .unwrap_or_else(|| {
            index.push(WorldIndex {
                world: Arc::downgrade(world),
                by_type: HashMap::new(),
            });
            index.len() - 1
        });
    insert_phased(&mut index[i].by_type, entity_type, phases, pos);
}

fn index_remove(world: &Arc<World>, pos: &BlockPos) {
    let mut index = lock(&HOST.index);
    if let Some(entry) = index
        .iter_mut()
        .find(|w| w.world.as_ptr() == Arc::as_ptr(world))
    {
        for buckets in entry.by_type.values_mut() {
            for positions in buckets {
                positions.remove(pos);
            }
        }
    }
}

/// Rebuilds the index from the custom data of live block entities.
fn discover(server: &Arc<Server>) {
    let registry = read(&HOST.registry);
    if registry.blocks.is_empty() {
        return;
    }
    let mut index = Vec::new();
    for world in server.worlds.load().iter() {
        let mut by_type: HashMap<usize, Vec<HashSet<BlockPos>>> = HashMap::new();
        for entry in &world.custom_block_entity_data {
            let pos = *entry.key();
            let Some(id) = entry
                .value()
                .get_compound(NAMESPACE)
                .and_then(|c| c.get_string(KEY_ID))
            else {
                continue;
            };
            let Some(block_idx) = registry.block(id) else {
                continue;
            };
            // Pumpkin keeps custom data of unloaded chunks around; only tick live block entities.
            let live = world
                .block_entities
                .get(&pos.chunk_position())
                .is_some_and(|chunk| chunk.contains_key(&pos));
            if live {
                let entity_type = registry.blocks[block_idx].entity_type;
                let phases = phase_count(&registry.entity_types[entity_type]);
                insert_phased(&mut by_type, entity_type, phases, pos);
            }
        }
        index.push(WorldIndex {
            world: Arc::downgrade(world),
            by_type,
        });
    }
    *lock(&HOST.index) = index;
}

/// One mod's due block entities in one world.
struct Batch {
    world: Arc<World>,
    mod_idx: usize,
    positions: Vec<BlockPos>,
    entries: Vec<TickInput>,
}

async fn tick_due(_server: &Arc<Server>, tick: i32) {
    let Ok(tick) = u64::try_from(tick) else {
        return;
    };
    let started = Instant::now();
    let batches = collect_batches(tick);
    stats::TIMING.add_collect(started, batches.iter().map(|b| b.entries.len()).sum());
    for batch in batches {
        let Some((plugin, exports)) = mod_handle(batch.mod_idx) else {
            continue;
        };
        let func = exports.tick;
        let dimension = batch.world.dimension.minecraft_name.to_owned();
        let entries = batch.entries;
        let count = entries.len();
        let started = Instant::now();
        let result = plugin
            .store
            .call_guest(move |mut guest| {
                Box::pin(async move {
                    guest
                        .call_func::<(u64, String, Vec<TickInput>), (Vec<TickUpdate>,)>(
                            func,
                            (tick, dimension, entries),
                        )
                        .await
                        .map(|(updates,)| updates)
                })
            })
            .await;
        stats::TIMING.add_call(started);
        let updates = match result {
            Ok(updates) => updates,
            Err(error) => {
                tracing::error!(
                    "moddedmc: content mod #{} trapped in tick; batch discarded: {error}",
                    batch.mod_idx
                );
                continue;
            }
        };
        if let Err(reason) = validate_updates(&updates, count) {
            tracing::error!(
                "moddedmc: content mod #{} returned an invalid tick batch ({reason}); discarded",
                batch.mod_idx
            );
            continue;
        }
        let started = Instant::now();
        for update in updates {
            write_state(
                &batch.world,
                &batch.positions[update.token as usize],
                &update.state,
            );
        }
        stats::TIMING.add_apply(started);
    }
}

fn collect_batches(tick: u64) -> Vec<Batch> {
    let registry = read(&HOST.registry);
    let index = lock(&HOST.index);
    let mut batches: Vec<Batch> = Vec::new();
    for world_index in index.iter() {
        let Some(world) = world_index.world.upgrade() else {
            continue;
        };
        for (&entity_type, buckets) in &world_index.by_type {
            let def = &registry.entity_types[entity_type];
            let Some(interval) = def.tick_interval else {
                continue;
            };
            let Some(positions) = buckets.get((tick % u64::from(interval)) as usize) else {
                continue;
            };
            if positions.is_empty() {
                continue;
            }
            let batch_idx = batches
                .iter()
                .position(|b| b.mod_idx == def.mod_idx && Arc::ptr_eq(&b.world, &world))
                .unwrap_or_else(|| {
                    batches.push(Batch {
                        world: world.clone(),
                        mod_idx: def.mod_idx,
                        positions: Vec::new(),
                        entries: Vec::new(),
                    });
                    batches.len() - 1
                });
            let batch = &mut batches[batch_idx];
            for pos in positions {
                if !state_version_matches(&world, pos, def) {
                    continue;
                }
                let Some(state) = read_state(&world, pos) else {
                    continue;
                };
                let token = u32::try_from(batch.entries.len()).unwrap_or(u32::MAX);
                batch.entries.push(TickInput {
                    token,
                    entity_type: u32::try_from(entity_type).unwrap_or(u32::MAX),
                    pos: wit_pos(pos),
                    state,
                });
                batch.positions.push(*pos);
            }
        }
    }
    batches.retain(|b| !b.entries.is_empty());
    batches
}

/// Fails closed: state saved under another layout version is not handed to the mod.
fn state_version_matches(world: &World, pos: &BlockPos, def: &EntityType) -> bool {
    let saved = world.get_block_entity_custom_data(pos, NAMESPACE, KEY_VERSION);
    let ok = matches!(saved, Some(NbtTag::Int(v)) if v.cast_unsigned() == def.state_version);
    if !ok {
        tracing::warn!(
            "moddedmc: {} at {pos:?} has state version {saved:?}, expected {}; skipped",
            def.id,
            def.state_version
        );
    }
    ok
}

fn validate_updates(updates: &[TickUpdate], count: usize) -> Result<(), &'static str> {
    let mut seen = vec![false; count];
    for update in updates {
        let slot = seen.get_mut(update.token as usize).ok_or("unknown token")?;
        if std::mem::replace(slot, true) {
            return Err("duplicate token");
        }
        if update.state.len() > MAX_STATE_BYTES {
            return Err("state too large");
        }
    }
    Ok(())
}

fn mod_handle(mod_idx: usize) -> Option<(Arc<WasmPlugin>, Arc<ContentExports>)> {
    let mods = lock(&HOST.mods);
    let m = mods.get(mod_idx)?;
    Some((m.plugin.upgrade()?, m.exports.clone()))
}

const fn wit_pos(pos: &BlockPos) -> WitPos {
    WitPos {
        x: pos.0.x,
        y: pos.0.y,
        z: pos.0.z,
    }
}

// ---------------------------------------------------------------- block events

struct PlaceHandler;

impl EventHandler<BlockPlaceEvent> for PlaceHandler {
    fn handle_blocking<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a mut BlockPlaceEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if event.cancelled() {
                return;
            }
            let inventory = event.player.inventory();
            let Some(id) = [inventory.held_item(), inventory.off_hand_item()]
                .iter()
                .find(|stack| is_block_item(stack, event.block_placed))
                .and_then(
                    |stack| match stack.get_custom_data(NAMESPACE, ITEM_BLOCK_KEY)? {
                        NbtTag::String(id) => Some(String::from(id)),
                        _ => None,
                    },
                )
            else {
                return;
            };
            let registry = read(&HOST.registry);
            let Some(block_idx) = registry.block(&id) else {
                tracing::warn!("moddedmc: placed item for unknown block {id}; placed as vanilla");
                return;
            };
            if registry.blocks[block_idx].stand_in.id != event.block_placed.id {
                return;
            }
            let world = Arc::downgrade(&event.player.world());
            let player = Some(event.player.gameprofile.name.clone());
            lock(&HOST.pending_places).push((world, event.block_position, block_idx, player));
        })
    }
}

fn is_block_item(stack: &ItemStack, block: &Block) -> bool {
    Item::from_registry_key(block.name).is_some_and(|item| item.id == stack.item.id)
}

struct BreakHandler;

impl EventHandler<BlockBreakEvent> for BreakHandler {
    fn handle_blocking<'a>(
        &'a self,
        server: &'a Arc<Server>,
        event: &'a mut BlockBreakEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if event.cancelled() {
                return;
            }
            let pos = event.block_position;
            let world = event.player.as_ref().map_or_else(
                || {
                    server
                        .worlds
                        .load()
                        .iter()
                        .find(|w| read_block_id(w, &pos).is_some())
                        .cloned()
                },
                |player| Some(player.world()),
            );
            let Some(world) = world else { return };
            let Some(id) = read_block_id(&world, &pos) else {
                return;
            };
            index_remove(&world, &pos);
            let player = event.player.as_ref().map(|p| p.gameprofile.name.clone());
            notify_broken(&world, pos, &id, player).await;
            if !event.drop {
                return;
            }
            let registry = read(&HOST.registry);
            let Some(block) = registry.block(&id).map(|i| &registry.blocks[i]) else {
                // Unknown to this server (mod removed): let the stand-in drop as vanilla.
                return;
            };
            if let Some(stack) = block_item(block) {
                event.drop = false;
                world.drop_stack(&pos, stack);
                tracing::info!("moddedmc: broke {id} at {pos:?}; dropped its item");
            }
        })
    }
}

/// Tells the owning mod one of its blocks is being broken, with its final state.
async fn notify_broken(world: &World, pos: BlockPos, id: &str, player: Option<String>) {
    let found = {
        let registry = read(&HOST.registry);
        registry.block(id).map(|b| {
            let entity_type = registry.blocks[b].entity_type;
            (entity_type, registry.entity_types[entity_type].mod_idx)
        })
    };
    let Some((entity_type, mod_idx)) = found else {
        return;
    };
    let input = BreakInput {
        entity_type: u32::try_from(entity_type).unwrap_or(u32::MAX),
        pos: wit_pos(&pos),
        dimension: world.dimension.minecraft_name.to_owned(),
        state: read_state(world, &pos).unwrap_or_default(),
        player_name: player,
    };
    call_mod::<(BreakInput,), ()>(mod_idx, |e| e.break_block, (input,), "break-block").await;
}

/// The item that places `block`: its stand-in item tagged with the block id.
fn block_item(block: &BlockType) -> Option<ItemStack> {
    let item = Item::from_registry_key(block.stand_in.name)?;
    let mut stack = ItemStack::new(1, item);
    stack.set_custom_data(
        NAMESPACE,
        ITEM_BLOCK_KEY,
        NbtTag::String(block.id.as_str().into()),
    );
    stack.set_custom_name(block.display_name.clone());
    Some(stack)
}

struct UseHandler;

impl EventHandler<PlayerInteractEvent> for UseHandler {
    fn handle_blocking<'a>(
        &'a self,
        server: &'a Arc<Server>,
        event: &'a mut PlayerInteractEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if event.cancelled() || !event.action.is_right_click() {
                return;
            }
            let Some(pos) = event.clicked_pos else { return };
            let world = event.player.world();
            let Some(id) = read_block_id(&world, &pos) else {
                return;
            };
            let (entity_type, mod_idx) = {
                let registry = read(&HOST.registry);
                let Some(block_idx) = registry.block(&id) else {
                    return;
                };
                let entity_type = registry.blocks[block_idx].entity_type;
                (entity_type, registry.entity_types[entity_type].mod_idx)
            };
            // Custom blocks replace the stand-in's own interaction (e.g. opening the barrel).
            event.set_cancelled(true);

            let tick = server.tick_count.load(std::sync::atomic::Ordering::Relaxed);
            let player_name = event.player.gameprofile.name.clone();
            let repeated =
                lock(&HOST.last_use).insert(player_name.clone(), (tick, pos)) == Some((tick, pos));
            if repeated {
                return;
            }
            let Some(state) = read_state(&world, &pos) else {
                return;
            };
            let input = UseInput {
                entity_type: u32::try_from(entity_type).unwrap_or(u32::MAX),
                pos: wit_pos(&pos),
                state,
                player_name,
                dimension: world.dimension.minecraft_name.to_owned(),
            };
            let Some((result,)) = call_mod::<(UseInput,), (UseResult,)>(
                mod_idx,
                |e| e.use_block,
                (input,),
                "use-block",
            )
            .await
            else {
                return;
            };
            if let Some(state) = result.state.filter(|s| s.len() <= MAX_STATE_BYTES) {
                write_state(&world, &pos, &state);
            }
            if let Some(message) = result.message {
                event
                    .player
                    .send_system_message_raw(&TextComponent::text(message), true);
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids() {
        assert!(valid_id("demo:counter"));
        assert!(valid_id("demo:machines/press"));
        assert!(!valid_id("counter"));
        assert!(!valid_id("minecraft:stone"));
        assert!(!valid_id("Demo:counter"));
        assert!(!valid_id("demo:"));
    }

    #[test]
    fn phases_spread_a_grid() {
        let mut counts = [0u32; 20];
        for x in 0..100 {
            for z in 0..100 {
                counts[phase(
                    &BlockPos(pumpkin_util::math::vector3::Vector3::new(x, 100, z)),
                    20,
                )] += 1;
            }
        }
        // 10,000 blocks over 20 phases: every phase within 2x of the 500 mean.
        assert!(
            counts.iter().all(|&c| (250..=1000).contains(&c)),
            "{counts:?}"
        );
    }

    #[test]
    fn update_validation() {
        let u = |token| TickUpdate {
            token,
            state: vec![],
        };
        assert!(validate_updates(&[u(0), u(2)], 3).is_ok());
        assert_eq!(validate_updates(&[u(3)], 3), Err("unknown token"));
        assert_eq!(validate_updates(&[u(1), u(1)], 3), Err("duplicate token"));
    }
}
