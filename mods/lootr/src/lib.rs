//! Lootr per-player inventory storage.
mod lifecycle;
mod scheduler;
mod shards;
pub mod state;
mod variants;

use moddedmc_api::{
    InterfaceUnavailable, Mod, Player, RegistrationError, Registry,
    commands::{self, Argument, Node},
    container_conversion::{self, Candidate, Conversion, Decision},
    container_loot::{self, ContainerLoot},
    content::{self, Content, EntitySnapshot, World, WorldExtension},
    entities::{self, EntitiesExtension},
    interaction::{self, BlockUseResult, Interaction, InteractionExtension, UseBlock},
    inventory_menu::{self, Access, Change, MenuKind, Validated, Validity, ValiditySlots},
    mobs::{self, Mobs, MobsExtension},
    saved_data::{self, Key, SavedData},
    stacks::{self, ItemStacks},
};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{LazyLock, Mutex, OnceLock};

#[derive(Clone, Copy)]
struct Interfaces {
    conversion: Conversion,
    content: Content,
    interaction: Interaction,
    menus: Validated,
    mobs: Mobs,
    saved: SavedData,
    loot: ContainerLoot,
    stacks: ItemStacks,
}
#[derive(Clone, Copy)]
struct Runtime {
    api: Interfaces,
    legacy: Key,
    manifest: Key,
    keys: [Key; shards::COUNT],
    menu: MenuKind,
    randomise_seed: bool,
}
static API: OnceLock<Interfaces> = OnceLock::new();
static RUNTIME: OnceLock<Runtime> = OnceLock::new();
static LOCK: Mutex<()> = Mutex::new(());
static POLICY: OnceLock<lifecycle::Config> = OnceLock::new();
static QUEUES: LazyLock<Mutex<HashMap<String, scheduler::Queue>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
const SIZE: usize = 27;
fn placement_facing(yaw: f32) -> Option<usize> {
    // Minecraft yaw zero looks south; a chest faces the opposite direction.
    yaw.is_finite()
        .then(|| ((yaw / 90.0 + 0.5).floor() as i32).rem_euclid(4) as usize)
}

// Temporary bounded diagnostics for the protocol-client smoke tests.
static DIAGNOSTICS: AtomicUsize = AtomicUsize::new(0);
fn trace(message: &str) {
    if DIAGNOSTICS.fetch_add(1, Ordering::Relaxed) < 128 {
        moddedmc_api::info(&format!("lootr diagnostic: {message}"));
    }
}

fn failed(message: &str) -> BlockUseResult {
    trace(message);
    BlockUseResult::Fail
}

struct Lootr;

impl Mod for Lootr {
    fn start(&self) -> Result<(), InterfaceUnavailable> {
        let content = content::require()?;
        content.require_extension(WorldExtension::Dimension)?;
        content.require_extension(WorldExtension::BlockEntities)?;
        content.require_extension(WorldExtension::FluidId)?;
        content.require_extension(WorldExtension::ChestObstruction)?;
        content.require_extension(WorldExtension::ContentEntityTransaction)?;
        content.require_extension(WorldExtension::GameTime)?;
        content.require_block_material()?;
        content.require_comparator_output()?;
        let interaction = interaction::require()?;
        interaction.require_extension(InteractionExtension::BlockUse)?;
        entities::require()?.require_extension(EntitiesExtension::Player)?;
        let mobs = mobs::require()?;
        mobs.require_extension(MobsExtension::Taming)?;
        let _ = API.set(Interfaces {
            conversion: container_conversion::require()?,
            content,
            interaction,
            menus: inventory_menu::require_validity()?,
            mobs,
            saved: saved_data::require()?,
            loot: container_loot::require()?,
            stacks: stacks::require()?,
        });
        Ok(())
    }

    fn register(&self, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
        let api = *API.get().ok_or(RegistrationError::InvalidDefinition)?;
        let randomise_seed = seed_config(std::fs::read_to_string(
            moddedmc_api::data_dir().join("randomise_seed.txt"),
        ))?;
        let policy = refresh_config(std::fs::read_to_string(
            moddedmc_api::data_dir().join("lifecycle.txt"),
        ))?;
        if policy.replace_when_decayed {
            api.content
                .require_extension(WorldExtension::ContentEntityReplacement)
                .map_err(|_| RegistrationError::InvalidDefinition)?;
        }
        let legacy = api.saved.register(registry, "inventories")?;
        let manifest = api.saved.register(registry, "inventories/v2/manifest")?;
        let mut keys = [legacy; shards::COUNT];
        for (i, key) in keys.iter_mut().enumerate() {
            *key = api
                .saved
                .register(registry, &format!("inventories/v2/{i:02x}"))?;
        }
        let menu = api.menus.register(registry, changed, valid)?;
        variants::register(api.content, registry)?;
        api.content.on_tick(registry, lifecycle_tick)?;
        commands::require()
            .map_err(|_| RegistrationError::InvalidDefinition)?
            .add(
                registry,
                "lootr",
                "Lootr administration",
                2,
                &[
                    Node {
                        parent: None,
                        name: "id",
                        argument: Argument::Literal,
                        tag: Some(1),
                    },
                    Node {
                        parent: None,
                        name: "clear",
                        argument: Argument::Literal,
                        tag: Some(2),
                    },
                    Node {
                        parent: None,
                        name: "refresh",
                        argument: Argument::Literal,
                        tag: Some(3),
                    },
                    Node {
                        parent: None,
                        name: "decay",
                        argument: Argument::Literal,
                        tag: Some(4),
                    },
                ],
            )?;
        api.interaction.on_use_block(registry, used)?;
        api.conversion.register(registry, convert)?;
        POLICY
            .set(policy)
            .map_err(|_| RegistrationError::InvalidDefinition)?;
        let _ = RUNTIME.set(Runtime {
            api,
            legacy,
            manifest,
            keys,
            menu,
            randomise_seed,
        });
        Ok(())
    }

    fn command(&self, tag: u64, command: moddedmc_api::Command<'_>) -> i32 {
        if tag == 2 {
            command.reply("Must provide player name.");
            return 1;
        }
        if !matches!(tag, 1 | 3 | 4) {
            return 0;
        }
        let Some(runtime) = RUNTIME.get().copied() else {
            return 0;
        };
        let Some(player) = &command.player else {
            command.reply("Please stand on a valid Lootr container.");
            return 0;
        };
        let Some(world) = runtime.api.content.command_world(&command) else {
            return 0;
        };
        if tag != 1 && clock(world).is_none() {
            command.reply("Persisted game time unavailable.");
            return 0;
        }
        let point = player.position();
        if ![point.x, point.y, point.z]
            .into_iter()
            .all(|n| n.is_finite() && n >= f64::from(i32::MIN) && n < f64::from(i32::MAX))
        {
            return 0;
        }
        let mut pos = moddedmc_api::Pos {
            x: point.x.floor() as i32,
            y: point.y.floor() as i32,
            z: point.z.floor() as i32,
        };
        if container_kind(runtime.api.content, world, pos).is_none() {
            let Some(y) = pos.y.checked_sub(1) else {
                return 0;
            };
            pos.y = y;
        }
        let Ok(_guard) = LOCK.lock() else { return 0 };
        let Some(dimension) = world.dimension_id() else {
            return 0;
        };
        if site_decaying(dimension, position(pos)) {
            command.reply("Container decay is in progress.");
            return 0;
        }
        if tag == 4
            && !POLICY
                .get()
                .is_some_and(|policy| policy.replace_when_decayed)
        {
            command.reply(
                "Drop-producing decay is unavailable; enable guarded replacement in lifecycle.txt.",
            );
            return 0;
        }
        if let Some((chest, _)) = chest_data(world, pos) {
            if tag == 1 {
                command.reply(&format!("The ID of this inventory is: {}", chest.id));
            } else if let Some(ticks) = begin_manual_timer(runtime, world, pos, &chest, tag == 4) {
                command.reply(&format!(
                    "Container with ID {} has been set to {} with a delay of {ticks}",
                    chest.id,
                    if tag == 4 { "decay" } else { "refresh" }
                ));
            } else {
                command.reply("Could not persist the lifecycle deadline.");
                return 0;
            }
        } else {
            command.reply("Please stand on a valid Lootr container.");
        }
        1
    }
}

fn refresh_config(value: std::io::Result<String>) -> Result<lifecycle::Config, RegistrationError> {
    let config = match value {
        Ok(value) => {
            lifecycle::Config::parse(&value).ok_or(RegistrationError::InvalidDefinition)?
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => lifecycle::Config::default(),
        Err(_) => return Err(RegistrationError::InvalidDefinition),
    };
    // Only the no-drop guarded replacement operation is available.
    // Never silently substitute replacement for default drop-producing decay.
    if config.decay.enabled() && !config.replace_when_decayed {
        return Err(RegistrationError::InvalidDefinition);
    }
    Ok(config)
}

/// The configuration is read once during registration; a missing file uses
/// the upstream default, but malformed or unreadable configuration is rejected.
fn seed_config(value: std::io::Result<String>) -> Result<bool, RegistrationError> {
    match value {
        Ok(value) => match value.trim() {
            "true" => Ok(true),
            "false" => Ok(false),
            _ => Err(RegistrationError::InvalidDefinition),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(_) => Err(RegistrationError::InvalidDefinition),
    }
}

struct ShardStore<'a> {
    runtime: Runtime,
    world: World<'a>,
}

impl shards::Store for ShardStore<'_> {
    fn legacy(&self) -> Option<Vec<u8>> {
        self.runtime
            .api
            .saved
            .read(&self.world, self.runtime.legacy)
    }

    fn manifest(&self) -> Option<Vec<u8>> {
        self.runtime
            .api
            .saved
            .read(&self.world, self.runtime.manifest)
    }

    fn write_manifest(&mut self, bytes: &[u8]) -> bool {
        self.runtime
            .api
            .saved
            .write(&self.world, self.runtime.manifest, bytes)
    }

    fn read(&self, index: usize) -> Option<Vec<u8>> {
        self.runtime
            .api
            .saved
            .read(&self.world, *self.runtime.keys.get(index)?)
    }

    fn write(&mut self, index: usize, bytes: &[u8]) -> bool {
        self.runtime
            .keys
            .get(index)
            .is_some_and(|key| self.runtime.api.saved.write(&self.world, *key, bytes))
    }

    fn mark_migrated(&mut self, bytes: &[u8]) -> bool {
        self.runtime
            .api
            .saved
            .write(&self.world, self.runtime.legacy, bytes)
    }
}

/// A live menu can only refer to an already initialized shard; a missing
/// shard must never be recreated by a click or a menu-load callback.
fn archive(runtime: Runtime, world: World<'_>, index: usize) -> Option<shards::Shard> {
    shards::read_initialized(&ShardStore { runtime, world }, index)
}

fn with_queue<T>(dimension: &str, f: impl FnOnce(&mut scheduler::Queue) -> T) -> Option<T> {
    let mut queues = QUEUES.lock().ok()?;
    if let Some(queue) = queues.get_mut(dimension) {
        return Some(f(queue));
    }
    Some(f(queues.entry(dimension.into()).or_default()))
}

fn position(pos: moddedmc_api::Pos) -> scheduler::Position {
    [pos.x, pos.y, pos.z]
}

fn clock(world: World<'_>) -> Option<i64> {
    world.game_time().filter(|now| *now >= 0)
}

fn site_decaying(dimension: &str, pos: scheduler::Position) -> bool {
    // Fail closed on poisoned queue state.
    with_queue(dimension, |q| q.decaying(pos)) != Some(false)
}

/// A reservation spans the unlocked host swap call. All archive operations
/// touching the target check this under LOCK before changing claims.
struct DecayReservation<'a> {
    dimension: &'a str,
    pos: scheduler::Position,
    committed: bool,
}

impl<'a> DecayReservation<'a> {
    fn reserve(dimension: &'a str, pos: scheduler::Position) -> Option<Self> {
        (with_queue(dimension, |q| q.reserve_decay(pos)) == Some(true)).then(|| Self {
            dimension,
            pos,
            committed: false,
        })
    }
}

impl Drop for DecayReservation<'_> {
    fn drop(&mut self) {
        let _ = with_queue(self.dimension, |q| q.finish_decay(self.pos, self.committed));
    }
}

/// No drop-producing fallback. The persistent old binding, timer and claims
/// are retained so a pre-swap world restored after a crash remains due.
fn execute_decay(
    runtime: Runtime,
    world: World<'_>,
    pos: moddedmc_api::Pos,
    snapshot: &EntitySnapshot,
    mut reservation: DecayReservation<'_>,
) -> bool {
    let Some((destination, replacement)) =
        variants::decay_replacement(runtime.api.content, snapshot.state, pos)
    else {
        return false;
    };
    reservation.committed =
        world.compare_replace_entity(pos, snapshot, destination, replacement.as_deref());
    reservation.committed
}

fn next_refresh(archive: &state::Archive, id: &str, config: &lifecycle::Config) -> Option<i64> {
    let schedule = archive.schedule(id)?;
    if !archive.has_claims(id) {
        return None;
    }
    let refresh = config
        .refresh
        .perform_while_ticking
        .then_some(schedule.refresh_at)
        .flatten();
    let decay = config
        .decay
        .perform_while_ticking
        .then_some(schedule.decay_at)
        .flatten();
    if config.replace_when_decayed {
        return refresh.into_iter().chain(decay).min();
    }
    // Without guarded replacement enabled, do not refresh an already-due
    // decay identity. The open path denies it instead.
    refresh.filter(|at| decay.is_none_or(|decay| decay > *at))
}

fn next_probe(
    archive: &state::Archive,
    id: &str,
    config: &lifecycle::Config,
    now: i64,
) -> Option<i64> {
    if let Some(deadline) = next_refresh(archive, id, config) {
        return Some(deadline);
    }
    // A block-entity table may be changed without a block state event.
    // Only the opt-in start-while-ticking policy needs occasional probes
    // of already-opened, currently unscheduled containers.
    if ((config.refresh.start_while_ticking && config.refresh.enabled())
        || (config.replace_when_decayed
            && config.decay.start_while_ticking
            && config.decay.enabled()))
        && archive.has_claims(id)
        && archive
            .schedule(id)
            .is_none_or(|s| s.refresh_at.is_none() && s.decay_at.is_none())
    {
        return now.checked_add(200);
    }
    None
}

/// Source /lootr refresh resets the deadline even if a timer already exists.
/// The caller holds LOCK; a new timer becomes visible only after the shard write.
fn begin_manual_timer(
    runtime: Runtime,
    world: World<'_>,
    pos: moddedmc_api::Pos,
    chest: &ChestData,
    decay: bool,
) -> Option<i64> {
    let now = clock(world)?;
    let config = POLICY.get()?;
    let dimension = world.dimension_id()?;
    let site = state::Site {
        dimension: dimension.into(),
        position: position(pos),
    };
    let index = shards::shard_for(&chest.id)?;
    let mut store = ShardStore { runtime, world };
    let mut shard = shards::load_or_init(&mut store, index)?;
    if !shard.archive.bind_site(&chest.id, &site) {
        return None;
    }
    let mut schedule = shard
        .archive
        .schedule(&chest.id)
        .cloned()
        .unwrap_or_default();
    let ticks = if decay {
        if !config.replace_when_decayed {
            return None;
        }
        schedule.decay_at = Some(now.checked_add(config.decay.ticks)?);
        config.decay.ticks
    } else {
        schedule.refresh_at = Some(now.checked_add(config.refresh.ticks)?);
        config.refresh.ticks
    };
    if !shard.archive.set_schedule(&chest.id, schedule) {
        return None;
    }
    let bytes = shard.encode()?;
    if !runtime.api.saved.write(&world, runtime.keys[index], &bytes) {
        return None;
    }
    with_queue(dimension, |q| {
        q.schedule(
            site.position,
            next_refresh(&shard.archive, &chest.id, config),
        )
    })?;
    Some(ticks)
}

/// Loaded/state events are cheap, deduplicated queue updates. Only a bounded
/// number of sites can deserialize an archive or write a shard each tick.
/// A persisted timer is rehydrated on load without scanning the world's blocks.
fn lifecycle_tick(world: World<'_>, _: u64, events: &[content::ContentEvent]) {
    let Some(runtime) = RUNTIME.get().copied() else {
        return;
    };
    let Some(config) = POLICY.get() else { return };
    let Some(dimension) = world.dimension_id() else {
        return;
    };
    let now = clock(world);
    let Some(ready) = with_queue(dimension, |q| {
        for event in events {
            let pos = position(event.position);
            if event.reason == content::UNLOADED {
                q.unload(pos);
            } else if matches!(
                event.reason,
                content::LOADED | content::STATE_CHANGED | content::MOVED_STATE_CHANGED
            ) {
                if variants::kind(runtime.api.content, event.state).is_some() {
                    // A state transition need not invalidate the existing
                    // incarnation; re-probe metadata instead.
                    // Zero is just the queue's immediate-probe priority
                    // if the clock is unavailable, never a lifecycle time.
                    q.schedule(pos, Some(now.unwrap_or(0)));
                } else {
                    q.unload(pos);
                }
            }
        }
        now.map_or_else(Vec::new, |now| q.ready(now))
    }) else {
        return;
    };
    let Some(now) = now else { return };
    for pos in ready {
        refresh_loaded(runtime, world, config, dimension, pos, now);
    }
}

enum LoadedProbe {
    Deadline(i64),
    Decay(EntitySnapshot),
}

fn refresh_loaded(
    runtime: Runtime,
    world: World<'_>,
    config: &lifecycle::Config,
    dimension: &str,
    position: scheduler::Position,
    now: i64,
) {
    let Ok(guard) = LOCK.lock() else { return };
    if site_decaying(dimension, position) {
        let _ = with_queue(dimension, |q| q.schedule(position, now.checked_add(20)));
        return;
    }
    match probe_loaded(runtime, world, config, dimension, position, now) {
        Some(LoadedProbe::Decay(snapshot)) => {
            let Some(reservation) = DecayReservation::reserve(dimension, position) else {
                return;
            };
            // Install bounded retry before releasing LOCK. A later successful
            // open cannot have its new timer overwritten by this old probe.
            let _ = with_queue(dimension, |q| q.schedule(position, now.checked_add(20)));
            drop(guard);
            // Host may close screens synchronously; never hold LOCK across CAS.
            let [x, y, z] = position;
            let pos = moddedmc_api::Pos { x, y, z };
            let _ = execute_decay(runtime, world, pos, &snapshot, reservation);
        }
        result => {
            let deadline = match result {
                Some(LoadedProbe::Deadline(at)) => Some(at),
                _ => None,
            };
            // Publish the result while serialized with the archive operation.
            let _ = with_queue(dimension, |q| q.schedule(position, deadline));
        }
    }
}

fn probe_loaded(
    runtime: Runtime,
    world: World<'_>,
    config: &lifecycle::Config,
    dimension: &str,
    position: scheduler::Position,
    now: i64,
) -> Option<LoadedProbe> {
    let [x, y, z] = position;
    let pos = moddedmc_api::Pos { x, y, z };
    // Do not assign IDs, initialize shards, or roll loot merely by loading a block.
    let snapshot = world.entity_snapshot(pos)?;
    variants::kind(runtime.api.content, snapshot.state)?;
    let chest: ChestData = fastnbt::from_bytes(&snapshot.nbt).ok()?;
    let index = shards::shard_for(&chest.id)?;
    let mut shard = archive(runtime, world, index)?;
    let site = state::Site {
        dimension: dimension.into(),
        position,
    };
    if !shard.archive.site_available(&chest.id, &site) || !shard.archive.has_claims(&chest.id) {
        return None;
    }
    let previous = shard.archive.schedule(&chest.id).cloned();
    // Nothing to do for an ordinary container with no running deadline
    // and no enabled start-on-tick filter.
    if previous.is_none()
        && !(config.refresh.start_while_ticking
            && config.refresh.matches(chest.table.as_deref(), dimension))
        && !(config.replace_when_decayed
            && config.decay.start_while_ticking
            && config.decay.matches(chest.table.as_deref(), dimension))
    {
        return next_probe(&shard.archive, &chest.id, config, now).map(LoadedProbe::Deadline);
    }
    if !shard.archive.bind_site(&chest.id, &site) {
        return None;
    }
    let action =
        shard
            .archive
            .advance_lifecycle(&chest.id, config, now, chest.table.as_deref(), true)?;
    if action == lifecycle::Action::Decay {
        if !config.replace_when_decayed {
            return None;
        }
        return Some(LoadedProbe::Decay(snapshot));
    }
    if action == lifecycle::Action::Refresh
        || previous.as_ref() != shard.archive.schedule(&chest.id)
    {
        // This only mutates the old identity's saved data, never the block.
        // Recheck the snapshot before the write to avoid a stale event.
        if !chest_matches(world, &site, &chest.id, Some(snapshot.incarnation)) {
            return None;
        }
        let bytes = shard.encode()?;
        if !runtime.api.saved.write(&world, runtime.keys[index], &bytes) {
            return now.checked_add(20).map(LoadedProbe::Deadline);
        }
        if action == lifecycle::Action::Refresh {
            let _ = with_queue(dimension, |q| q.invalidate(position));
        }
    }
    next_probe(&shard.archive, &chest.id, config, now).map(LoadedProbe::Deadline)
}

#[derive(Default, serde::Deserialize)]
struct ChestData {
    #[serde(rename = "LootrId", default)]
    id: String,
    #[serde(rename = "LootTable", default)]
    table: Option<String>,
    #[serde(rename = "LootTableSeed", default)]
    seed: i64,
    #[serde(rename = "CustomName", default)]
    name: Option<fastnbt::Value>,
    #[serde(rename = "lock", default)]
    lock: Option<fastnbt::Value>,
    #[serde(rename = "Lock", default)]
    legacy_lock: Option<fastnbt::Value>,
    #[serde(rename = "LootrIsCustomInventory", default)]
    reference: i8,
    #[serde(rename = "LootrCustomInventory", default)]
    reference_items: Vec<fastnbt::Value>,
}

impl ChestData {
    fn locked(&self) -> bool {
        // Vanilla 26.3's lowercase lock is an ItemPredicate. Until the host
        // exposes predicate evaluation, never grant access to a locked block.
        fn present(lock: &fastnbt::Value) -> bool {
            match lock {
                fastnbt::Value::Compound(fields) => !fields.is_empty(),
                fastnbt::Value::String(value) => !value.is_empty(),
                _ => true,
            }
        }
        self.lock.as_ref().is_some_and(present) || self.legacy_lock.as_ref().is_some_and(present)
    }

    fn title(&self, kind: variants::Variant) -> String {
        let default = if kind.barrel() { "Barrel" } else { "Chest" };
        let text = match self.name.as_ref() {
            Some(fastnbt::Value::String(text)) => Some(text),
            Some(fastnbt::Value::Compound(fields)) => match fields.get("text") {
                Some(fastnbt::Value::String(text)) => Some(text),
                _ => None,
            },
            _ => None,
        };
        // Native menu ABI accepts at most 256 UTF-8 bytes, not characters.
        // Avoid reserving a claim which cannot be shown due to its title.
        text.filter(|text| !text.is_empty() && text.len() <= 256)
            .map_or_else(|| default.into(), |text| text.clone())
    }

    fn reference_slots(&self) -> Option<Vec<Option<Vec<u8>>>> {
        let mut slots = vec![None; SIZE];
        if self.reference == 0 {
            return Some(slots);
        }
        if self.reference != 1 || self.reference_items.len() > SIZE {
            return None;
        }
        for item in &self.reference_items {
            let fastnbt::Value::Compound(mut fields) = item.clone() else {
                return None;
            };
            let index = match fields.remove("Slot") {
                None => 0,
                Some(fastnbt::Value::Byte(n)) => usize::try_from(n).ok()?,
                Some(fastnbt::Value::Int(n)) => usize::try_from(n).ok()?,
                _ => return None,
            };
            let target = slots.get_mut(index)?;
            if target.is_some()
                || !matches!(fields.get("id"), Some(fastnbt::Value::String(id)) if !id.is_empty() && id != "minecraft:air" && id != "air")
                || fields
                    .get("components")
                    .is_some_and(|value| !matches!(value, fastnbt::Value::Compound(_)))
            {
                return None;
            }
            match fields.get("count") {
                None => {
                    fields.insert("count".into(), fastnbt::Value::Int(1));
                }
                Some(fastnbt::Value::Int(n)) if (1..=99).contains(n) => {}
                _ => return None,
            }
            let bytes = fastnbt::to_bytes(&fields).ok()?;
            if bytes.len() > moddedmc_api::abi::MAX_STACK_BYTES {
                return None;
            }
            *target = Some(bytes);
        }
        Some(slots)
    }
}

fn assign_chest_id(bytes: &[u8], id: &str) -> Option<Vec<u8>> {
    // Preserve metadata owned by the container, including fields we do not interpret.
    let mut fields: std::collections::HashMap<String, fastnbt::Value> =
        fastnbt::from_bytes(bytes).ok()?;
    fields.insert("LootrId".into(), fastnbt::Value::String(id.into()));
    fastnbt::to_bytes(&fields).ok()
}

fn convert(candidate: Candidate<'_>) -> Decision {
    let Some(api) = API.get() else {
        return Decision::Defer;
    };
    let Some(state) = variants::converted(api.content, candidate.state) else {
        return Decision::Pass;
    };
    let Ok(metadata) = fastnbt::from_bytes::<ChestData>(candidate.nbt) else {
        return Decision::Pass;
    };
    if metadata.locked() || metadata.reference != 0 {
        return Decision::Pass;
    }
    let Some(nbt) = assign_chest_id(candidate.nbt, &format!("{:032x}", rand::random::<u128>()))
    else {
        return Decision::Defer;
    };
    Decision::Convert { state, nbt }
}

fn container_kind(
    content: Content,
    world: World<'_>,
    pos: moddedmc_api::Pos,
) -> Option<variants::Variant> {
    let mut states = [moddedmc_api::content::UNLOADED_STATE];
    world.read(&[pos], &mut states).then_some(())?;
    variants::kind(content, states[0])
}

fn chest_data(world: World<'_>, pos: moddedmc_api::Pos) -> Option<(ChestData, u64)> {
    let Some(snapshot) = world.entity_snapshot(pos) else {
        trace("world.entity missing");
        return None;
    };
    variants::kind(API.get()?.content, snapshot.state)?;
    let mut data: ChestData = match fastnbt::from_bytes(&snapshot.nbt) {
        Ok(data) => data,
        Err(error) => {
            trace(&format!("chest NBT decoding: {error}"));
            return None;
        }
    };
    if data.id.is_empty() {
        data.id = format!("{:032x}", rand::random::<u128>());
        let bytes = match assign_chest_id(&snapshot.nbt, &data.id) {
            Some(bytes) => bytes,
            None => {
                trace("chest NBT encoding failed");
                return None;
            }
        };
        if !world.compare_entity(pos, &snapshot, &bytes) {
            trace("world.compare_entity failed");
            return None;
        }
    }
    trace("chest_data decoded");
    state::valid_id(&data.id).then_some((data, snapshot.incarnation))
}

fn loot_seed(stored: i64, randomise: bool) -> i64 {
    select_seed(stored, randomise, rand::random)
}

fn select_seed(stored: i64, randomise: bool, generate: impl FnOnce() -> i64) -> i64 {
    if randomise || stored == 0 || stored == -1 {
        generate()
    } else {
        stored
    }
}

fn chest_matches(world: World<'_>, site: &state::Site, id: &str, incarnation: Option<u64>) -> bool {
    if world.dimension_id() != Some(site.dimension.as_str()) {
        return false;
    }
    let [x, y, z] = site.position;
    let pos = moddedmc_api::Pos { x, y, z };
    let Some(snapshot) = world.entity_snapshot(pos) else {
        return false;
    };
    if incarnation.is_some_and(|expected| expected != snapshot.incarnation)
        || !API
            .get()
            .and_then(|api| variants::kind(api.content, snapshot.state))
            .is_some()
    {
        return false;
    }
    fastnbt::from_bytes::<ChestData>(&snapshot.nbt)
        .ok()
        .is_some_and(|chest| chest.id == id)
}

fn site_matches(world: World<'_>, record: &state::Record) -> bool {
    record.site.as_ref().is_some_and(|site| {
        !site_decaying(&site.dimension, site.position)
            && chest_matches(world, site, &record.container, None)
    })
}

#[derive(serde::Deserialize, serde::Serialize)]
struct ViewContext {
    site: state::Site,
    container: String,
    incarnation: i64,
    epoch: i64,
}

fn within_reach(player: &Player<'_>, site: &state::Site) -> bool {
    let [x, y, z] = site.position;
    let pos = player.position();
    let distance = (pos.x - f64::from(x) - 0.5).powi(2)
        + (pos.y - f64::from(y) - 0.5).powi(2)
        + (pos.z - f64::from(z) - 0.5).powi(2);
    player.dimension() == site.dimension && distance.is_finite() && distance <= 64.0
}

fn valid(view: Validity<'_>) -> bool {
    let Ok(context) = fastnbt::from_bytes::<ViewContext>(view.context) else {
        trace("validity context decode failed");
        return false;
    };
    if !within_reach(&view.player, &context.site) {
        trace("validity reach failed");
        return false;
    }
    if clock(view.world).is_none()
        || with_queue(&context.site.dimension, |q| {
            q.valid(context.site.position, context.epoch)
        }) != Some(true)
    {
        return false;
    }
    if !chest_matches(
        view.world,
        &context.site,
        &context.container,
        Some(context.incarnation as u64),
    ) {
        trace("validity chest identity failed");
        return false;
    }
    true
}

/// Sneaking never creates a private inventory or assigns a new identity.
fn mark_unopened(
    runtime: Runtime,
    world: World<'_>,
    pos: moddedmc_api::Pos,
    site: &state::Site,
    owner: [u8; 16],
) {
    let Ok(_guard) = LOCK.lock() else { return };
    if site_decaying(&site.dimension, site.position) {
        return;
    }
    let Some(snapshot) = world.entity_snapshot(pos) else {
        return;
    };
    if variants::kind(runtime.api.content, snapshot.state).is_none() {
        return;
    }
    let Ok(chest) = fastnbt::from_bytes::<ChestData>(&snapshot.nbt) else {
        return;
    };
    let Some(index) = shards::shard_for(&chest.id) else {
        return;
    };
    let mut store = ShardStore { runtime, world };
    let Some(mut shard) = shards::load_or_init(&mut store, index) else {
        return;
    };
    if !shard.archive.site_available(&chest.id, site)
        || !shard.archive.mark_visual(&chest.id, owner, false)
    {
        return;
    }
    if let Some(bytes) = shard.encode() {
        runtime.api.saved.write(&world, runtime.keys[index], &bytes);
    }
}

fn used(context: &mut UseBlock<'_>) -> BlockUseResult {
    // LootrChestBlock overrides useWithoutItem only. Vanilla runs that
    // fallback for the main hand and does not open a menu from off-hand use.
    if context.with_item {
        return BlockUseResult::TryEmptyHand;
    }
    trace("block use hook entered");
    let Some(runtime) = RUNTIME.get().copied() else {
        return failed("runtime missing");
    };
    let Some(entity) = context.player else {
        return failed("use context has no player");
    };
    let Some(player) = entity.as_player() else {
        return failed("use context entity is not a player");
    };
    let Some(owner) = runtime.api.mobs.uuid(entity) else {
        return failed("owner UUID missing");
    };
    let world = context.world;
    let pos = context.position;
    if clock(world).is_none() {
        return failed("persisted game time unavailable");
    }
    let Some(policy) = POLICY.get() else {
        return failed("lifecycle policy unavailable");
    };
    let Some(kind) = container_kind(runtime.api.content, world, pos) else {
        return failed("container block state changed");
    };
    if runtime
        .api
        .mobs
        .game_mode(entity)
        .is_none_or(|mode| mode == 3)
    {
        return BlockUseResult::Success;
    }
    let Some(dimension) = world.dimension_id() else {
        return failed("world dimension missing");
    };
    let site = state::Site {
        dimension: dimension.into(),
        position: [pos.x, pos.y, pos.z],
    };
    if !within_reach(&player, &site) {
        trace(&format!(
            "initial reach failed: player dimension {:?}, site dimension {:?}, position {:?}",
            player.dimension(),
            site.dimension,
            player.position()
        ));
        return BlockUseResult::Fail;
    }
    // The Java chest handles sneak-use before checking overhead obstruction.
    if context.sneaking {
        mark_unopened(runtime, world, pos, &site, owner);
        return BlockUseResult::Success;
    }
    if !kind.barrel() && world.chest_blocked(pos) != Some(false) {
        return BlockUseResult::Success;
    }
    let (index, id, inventory, validity_context, title) = {
        let Ok(guard) = LOCK.lock() else {
            return failed("archive lock poisoned");
        };
        if site_decaying(dimension, site.position) {
            return failed("container decay is in progress");
        }
        let Some(now) = clock(world) else {
            return failed("persisted game time unavailable");
        };
        if container_kind(runtime.api.content, world, pos) != Some(kind) {
            return failed("container state changed before claim");
        }
        let Some((chest, incarnation)) = chest_data(world, pos) else {
            return failed("chest_data failed");
        };
        if chest.locked() {
            return BlockUseResult::Success;
        }
        let title = chest.title(kind);
        let Some(index) = shards::shard_for(&chest.id) else {
            return failed("invalid shard prefix");
        };
        let mut store = ShardStore { runtime, world };
        let Some(mut shard) = shards::load_or_init(&mut store, index) else {
            return failed("shard initialization failed");
        };
        let data = &mut shard.archive;
        if !data.bind_site(&chest.id, &site) {
            return failed("site binding failed");
        }
        let Some(action) =
            data.advance_lifecycle(&chest.id, policy, now, chest.table.as_deref(), false)
        else {
            return failed("lifecycle site missing");
        };
        if action == lifecycle::Action::Decay {
            if !policy.replace_when_decayed {
                return failed("container decay due; guarded destruction unavailable");
            }
            let Some(snapshot) = world.entity_snapshot(pos) else {
                return failed("decay snapshot unavailable");
            };
            if snapshot.incarnation != incarnation
                || variants::kind(runtime.api.content, snapshot.state) != Some(kind)
                || !fastnbt::from_bytes::<ChestData>(&snapshot.nbt)
                    .is_ok_and(|data| data.id == chest.id && !data.locked())
            {
                return failed("decay snapshot identity changed");
            }
            let Some(reservation) = DecayReservation::reserve(dimension, site.position) else {
                return failed("decay already reserved");
            };
            drop(guard);
            return if execute_decay(runtime, world, pos, &snapshot, reservation) {
                BlockUseResult::Success
            } else {
                failed("guarded decay failed")
            };
        }
        if action == lifecycle::Action::Refresh {
            // Persist the deletion even if new loot generation later fails.
            let Some(bytes) = shard.encode() else {
                return failed("refresh encoding failed");
            };
            if !runtime.api.saved.write(&world, runtime.keys[index], &bytes) {
                return failed("refresh save failed");
            }
            if with_queue(dimension, |q| q.invalidate(site.position)).is_none() {
                return failed("view invalidation failed");
            }
        }
        let data = &mut shard.archive;
        let Some(id) = data.open_bounded(&chest.id, owner, SIZE, shards::MAX_ID, || {
            let slots = if chest.reference != 0 {
                chest.reference_slots()
            } else {
                match chest.table.as_deref() {
                    Some(table) => runtime.api.loot.generate(
                        world,
                        &player,
                        table,
                        loot_seed(chest.seed, runtime.randomise_seed),
                        pos,
                        SIZE,
                    ),
                    None => Some(vec![None; SIZE]),
                }
            }?;
            // Validate through the same host stack codec used to initialize
            // the native menu BEFORE reserving or saving a new claim.
            // Otherwise a typo in a custom item id permanently consumes the
            // claim despite the menu failing to open.
            slots
                .iter()
                .flatten()
                .all(|stack| {
                    runtime
                        .api
                        .stacks
                        .info(stack)
                        .is_some_and(|info| info.count > 0 && info.count <= info.max_count)
                })
                .then_some(slots)
        }) else {
            return failed("archive open / loot generation failed");
        };
        let Some(record) = data.get_mut(id, owner) else {
            return failed("new record lookup failed");
        };
        if record.site.as_ref().is_some_and(|old| old != &site) {
            return failed("record site mismatch");
        }
        record.site = Some(site.clone());
        data.mark_visual(&chest.id, owner, true);
        let deadline = next_probe(data, &chest.id, policy, now);
        let Some(Some(epoch)) = with_queue(dimension, |q| q.epoch(site.position)) else {
            return failed("view epoch unavailable");
        };
        let Ok(validity_context) = fastnbt::to_bytes(&ViewContext {
            site: site.clone(),
            container: chest.id,
            incarnation: incarnation as i64,
            epoch,
        }) else {
            return failed("view context encoding failed");
        };
        let Some(inventory) = shards::menu_id(index, id) else {
            return failed("menu id invalid");
        };
        let Some(bytes) = shard.encode() else {
            return failed("shard encoding failed");
        };
        if !runtime.api.saved.write(&world, runtime.keys[index], &bytes) {
            return failed("shard save failed");
        }
        let _ = with_queue(dimension, |q| q.schedule(site.position, deadline));
        (index, id, inventory, validity_context, title)
    };
    if runtime.api.menus.open(
        &player,
        runtime.menu,
        inventory,
        &title,
        ValiditySlots {
            world,
            access: &[Access {
                take: true,
                insert: true,
            }; SIZE],
            context: &validity_context,
        },
        || {
            trace("menu initializer entered");
            let _guard = LOCK.lock().ok()?;
            let shard = archive(runtime, world, index)?;
            let record = shard.archive.get(id, owner)?;
            site_matches(world, record).then(|| record.slots())
        },
    ) {
        trace("menu opened");
        BlockUseResult::Success
    } else {
        failed("validated menu open rejected")
    }
}

fn changed(mut change: Change<'_>) {
    let Some(runtime) = RUNTIME.get().copied() else {
        return;
    };
    let Ok(_guard) = LOCK.lock() else { return };
    if clock(change.world).is_none() {
        return;
    }
    let Some((index, id)) = shards::split_menu(change.inventory) else {
        return;
    };
    let Some(mut shard) = archive(runtime, change.world, index) else {
        return;
    };
    let Some(record) = shard.archive.get_mut(id, change.player_uuid) else {
        return;
    };
    if !site_matches(change.world, record) {
        return;
    }
    // Refuse a transaction that would discard displaced items.
    if change.overflow().len() != 0 {
        return;
    }
    if record.replace(change.slots()).is_none() {
        return;
    }
    if let Some(bytes) = shard.encode() {
        change.commit(runtime.keys[index], &bytes);
    }
}

moddedmc_api::export_mod!(Lootr);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placement_faces_opposite_horizontal_view() {
        for (yaw, facing) in [(0., 0), (90., 1), (180., 2), (270., 3), (-90., 3)] {
            assert_eq!(placement_facing(yaw), Some(facing));
        }
        assert_eq!(placement_facing(44.9), Some(0));
        assert_eq!(placement_facing(45.), Some(1));
        assert_eq!(placement_facing(-45.), Some(0));
        assert_eq!(placement_facing(f32::NAN), None);
    }

    #[test]
    fn reference_inventory_keeps_slot_components_and_lock_is_not_bypassed() {
        use fastnbt::Value;
        use std::collections::HashMap;
        let mut data = ChestData {
            reference: 1,
            reference_items: vec![Value::Compound(HashMap::from([
                ("Slot".into(), Value::Byte(5)),
                ("id".into(), Value::String("minecraft:diamond".into())),
                ("count".into(), Value::Int(7)),
                ("components".into(), Value::Compound(HashMap::new())),
            ]))],
            name: Some(Value::String("Private diamonds".into())),
            ..ChestData::default()
        };
        assert_eq!(data.title(variants::VARIANTS[0]), "Private diamonds");
        assert!(!data.locked());
        let slots = data.reference_slots().unwrap();
        assert_eq!(slots.iter().filter(|slot| slot.is_some()).count(), 1);
        let stack: HashMap<String, Value> =
            fastnbt::from_bytes(slots[5].as_ref().unwrap()).unwrap();
        assert_eq!(stack.get("count"), Some(&Value::Int(7)));
        assert!(!stack.contains_key("Slot"));
        data.reference_items.push(data.reference_items[0].clone());
        assert!(data.reference_slots().is_none());
        data.lock = Some(Value::Compound(HashMap::from([(
            "items".into(),
            Value::String("minecraft:diamond".into()),
        )])));
        assert!(data.locked());
    }

    #[test]
    fn malformed_reference_items_and_long_title_fail_safely() {
        use fastnbt::Value;
        use std::collections::HashMap;
        let item = |id: &str, count: Value, components: Value| {
            Value::Compound(HashMap::from([
                ("id".into(), Value::String(id.into())),
                ("count".into(), count),
                ("components".into(), components),
            ]))
        };
        let compound = || Value::Compound(HashMap::new());
        let mut data = ChestData {
            reference: 1,
            ..ChestData::default()
        };
        for invalid in [
            item("minecraft:air", Value::Int(1), compound()),
            item("minecraft:diamond", Value::Int(0), compound()),
            item("minecraft:diamond", Value::Int(100), compound()),
            item(
                "minecraft:diamond",
                Value::Int(1),
                Value::String("bad".into()),
            ),
        ] {
            data.reference_items = vec![invalid];
            assert!(data.reference_slots().is_none());
        }
        data.reference_items = vec![Value::Compound(HashMap::from([(
            "id".into(),
            Value::String("minecraft:diamond".into()),
        )]))];
        let slots = data.reference_slots().unwrap();
        let stack: HashMap<String, Value> =
            fastnbt::from_bytes(slots[0].as_ref().unwrap()).unwrap();
        assert_eq!(stack.get("count"), Some(&Value::Int(1)));
        data.name = Some(Value::String("é".repeat(129)));
        assert_eq!(data.title(variants::VARIANTS[2]), "Barrel");
        data.name = Some(Value::String("é".repeat(128)));
        assert_eq!(data.title(variants::VARIANTS[2]).len(), 256);
    }

    #[test]
    fn assigning_identity_preserves_container_metadata() {
        use fastnbt::Value;
        use std::collections::HashMap;
        let original = HashMap::from([
            (
                "LootTable".to_owned(),
                Value::String("minecraft:chests/simple_dungeon".into()),
            ),
            ("LootTableSeed".to_owned(), Value::Long(-42)),
            ("CustomName".to_owned(), Value::String("Example".into())),
            (
                "Unrecognised".to_owned(),
                Value::Compound(HashMap::from([("flag".into(), Value::Byte(1))])),
            ),
        ]);
        let bytes = fastnbt::to_bytes(&original).unwrap();
        let id = "a1234567890123456789012345678901";
        let updated = assign_chest_id(&bytes, id).unwrap();
        let mut fields: HashMap<String, Value> = fastnbt::from_bytes(&updated).unwrap();
        assert_eq!(fields.remove("LootrId"), Some(Value::String(id.into())));
        assert_eq!(fields, original);
    }

    #[test]
    fn explicit_seed_is_preserved_when_randomisation_is_off() {
        for seed in [1, 12345, -42, i64::MAX, i64::MIN] {
            assert_eq!(loot_seed(seed, false), seed);
            assert_eq!(
                select_seed(seed, false, || panic!("explicit seed rerolled")),
                seed
            );
            assert_eq!(select_seed(seed, true, || 123), 123);
        }
        for seed in [0, -1] {
            assert_eq!(select_seed(seed, false, || 456), 456);
            assert_eq!(select_seed(seed, true, || 789), 789);
        }
    }

    #[test]
    fn seed_configuration_defaults_only_for_missing_file() {
        use std::io::{Error, ErrorKind};
        assert!(seed_config(Err(Error::from(ErrorKind::NotFound))).unwrap());
        assert!(seed_config(Ok("true\r\n".into())).unwrap());
        assert!(!seed_config(Ok(" false ".into())).unwrap());
        for invalid in ["", "TRUE", "0", "false\ntrue"] {
            assert!(seed_config(Ok(invalid.into())).is_err());
        }
        assert!(seed_config(Err(Error::from(ErrorKind::PermissionDenied))).is_err());
        assert!(seed_config(Err(Error::from(ErrorKind::InvalidData))).is_err());
    }

    #[test]
    fn refresh_configuration_requires_explicit_supported_policy() {
        use std::io::{Error, ErrorKind};
        assert_eq!(
            refresh_config(Err(Error::from(ErrorKind::NotFound))).unwrap(),
            lifecycle::Config::default()
        );
        let config = refresh_config(Ok("refresh_all=true\nrefresh_value=100".into())).unwrap();
        assert!(config.refresh.all);
        assert_eq!(config.refresh.ticks, 100);
        for invalid in [
            "decay_all=true",
            "decay_dimensions=minecraft:overworld",
            "refresh_value=0",
            "unknown=true",
        ] {
            assert!(refresh_config(Ok(invalid.into())).is_err());
        }
        let guarded =
            refresh_config(Ok("decay_all=true\nreplace_when_decayed=true".into())).unwrap();
        assert!(guarded.decay.all && guarded.replace_when_decayed);
        assert!(refresh_config(Err(Error::from(ErrorKind::PermissionDenied))).is_err());
    }

    #[test]
    fn pending_refresh_index_ignores_unclaimed_and_open_only_timers() {
        let id = "00000000000000000000000000000001";
        let mut archive = state::Archive::default();
        let site = state::Site {
            dimension: "minecraft:overworld".into(),
            position: [1, 2, 3],
        };
        assert!(archive.bind_site(id, &site));
        assert!(archive.set_schedule(
            id,
            lifecycle::Schedule {
                refresh_at: Some(20),
                decay_at: None
            }
        ));
        let mut config = lifecycle::Config::default();
        assert_eq!(next_refresh(&archive, id, &config), None);
        archive
            .open(id, [1; 16], SIZE, || Some(vec![None; SIZE]))
            .unwrap();
        assert_eq!(next_refresh(&archive, id, &config), Some(20));
        config.refresh.perform_while_ticking = false;
        assert_eq!(next_refresh(&archive, id, &config), None);
        config.refresh.perform_while_ticking = true;
        assert!(archive.set_schedule(
            id,
            lifecycle::Schedule {
                refresh_at: Some(20),
                decay_at: Some(19)
            }
        ));
        assert_eq!(next_refresh(&archive, id, &config), None);
        config.replace_when_decayed = true;
        assert_eq!(next_refresh(&archive, id, &config), Some(19));
        config.refresh.perform_while_ticking = false;
        assert_eq!(next_refresh(&archive, id, &config), Some(19));
        config.decay.perform_while_ticking = false;
        assert_eq!(next_refresh(&archive, id, &config), None);
        config.refresh.perform_while_ticking = true;
        config.replace_when_decayed = false;
        config.refresh.all = true;
        config.refresh.start_while_ticking = true;
        assert!(archive.set_schedule(id, lifecycle::Schedule::default()));
        assert_eq!(next_probe(&archive, id, &config, 100), Some(300));
    }
}
