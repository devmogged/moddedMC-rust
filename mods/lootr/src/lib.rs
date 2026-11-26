//! Lootr per-player inventory storage.
mod lifecycle;
mod shards;
pub mod state;
mod variants;

use moddedmc_api::{
    InterfaceUnavailable, Mod, Player, RegistrationError, Registry,
    commands::{self, Argument, Node},
    container_conversion::{self, Candidate, Conversion, Decision},
    container_loot::{self, ContainerLoot},
    content::{self, Content, World, WorldExtension},
    entities::{self, EntitiesExtension},
    interaction::{self, BlockUseResult, Interaction, InteractionExtension, UseBlock},
    inventory_menu::{self, Access, Change, MenuKind, Validated, Validity, ValiditySlots},
    mobs::{self, Mobs, MobsExtension},
    saved_data::{self, Key, SavedData},
    stacks::{self, ItemStacks},
};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

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
                ],
            )?;
        api.interaction.on_use_block(registry, used)?;
        api.conversion.register(registry, convert)?;
        let randomise_seed = seed_config(std::fs::read_to_string(
            moddedmc_api::data_dir().join("randomise_seed.txt"),
        ))?;
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
        if tag != 1 {
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
        if let Some((chest, _)) = chest_data(world, pos) {
            command.reply(&format!("The ID of this inventory is: {}", chest.id));
        } else {
            command.reply("Please stand on a valid Lootr container.");
        }
        1
    }
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
    let bytes = runtime.api.saved.read(&world, *runtime.keys.get(index)?)?;
    shards::Shard::decode(index, &bytes)
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
    record
        .site
        .as_ref()
        .is_some_and(|site| chest_matches(world, site, &record.container, None))
}

#[derive(serde::Deserialize, serde::Serialize)]
struct ViewContext {
    site: state::Site,
    container: String,
    incarnation: i64,
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
        let Ok(_guard) = LOCK.lock() else {
            return failed("archive lock poisoned");
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
        let Ok(validity_context) = fastnbt::to_bytes(&ViewContext {
            site,
            container: chest.id,
            incarnation: incarnation as i64,
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
}
