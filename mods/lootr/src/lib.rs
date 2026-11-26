//! Lootr per-player inventory storage.
mod shards;
pub mod state;

use moddedmc_api::{
    InterfaceUnavailable, Mod, Player, RegistrationError, Registry,
    container_loot::{self, ContainerLoot},
    content::{self, Block, Content, Placement, ShapeBox, State, World, WorldExtension},
    entities::{self, EntitiesExtension},
    interaction::{self, BlockUseResult, Interaction, InteractionExtension, UseBlock},
    inventory_menu::{self, Access, Change, MenuKind, Validated, Validity, ValiditySlots},
    mobs::{self, Mobs, MobsExtension},
    saved_data::{self, Key, SavedData},
};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

#[derive(Clone, Copy)]
struct Interfaces {
    content: Content,
    interaction: Interaction,
    menus: Validated,
    mobs: Mobs,
    saved: SavedData,
    loot: ContainerLoot,
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
const FACINGS: [&str; 4] = ["north", "east", "south", "west"];
const CHEST_SHAPE: [ShapeBox; 1] = [ShapeBox {
    min: [1, 0, 1],
    max: [15, 14, 15],
}];
static CHEST_STATES: OnceLock<[u32; 8]> = OnceLock::new();

fn chest_properties(facing: &str, wet: bool) -> String {
    format!("facing={facing},type=single,waterlogged={wet}")
}

fn chest_states(content: Content) -> Option<&'static [u32; 8]> {
    if let Some(states) = CHEST_STATES.get() {
        return Some(states);
    }
    let mut states = [0; 8];
    for (index, facing) in FACINGS.iter().enumerate() {
        for (offset, wet) in [false, true].into_iter().enumerate() {
            states[index * 2 + offset] =
                content.state(&format!("lootr:chest[{}]", chest_properties(facing, wet)))?;
        }
    }
    let _ = CHEST_STATES.set(states);
    CHEST_STATES.get()
}

fn placement_facing(yaw: f32) -> Option<usize> {
    // Minecraft yaw zero looks south; a chest faces the opposite direction.
    yaw.is_finite()
        .then(|| ((yaw / 90.0 + 0.5).floor() as i32).rem_euclid(4) as usize)
}

fn placed(args: Placement<'_>) -> Option<u32> {
    let content = API.get()?.content;
    let facing = placement_facing(args.yaw)?;
    let wet = args.world.fluid_id(args.position)?.as_str() == "minecraft:water";
    Some(chest_states(content)?[facing * 2 + usize::from(wet)])
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
        content.require_extension(WorldExtension::RedstoneConductor)?;
        let interaction = interaction::require()?;
        interaction.require_extension(InteractionExtension::BlockUse)?;
        entities::require()?.require_extension(EntitiesExtension::Player)?;
        let mobs = mobs::require()?;
        mobs.require_extension(MobsExtension::Taming)?;
        let _ = API.set(Interfaces {
            content,
            interaction,
            menus: inventory_menu::require_validity()?,
            mobs,
            saved: saved_data::require()?,
            loot: container_loot::require()?,
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
        let properties: Vec<_> = FACINGS
            .iter()
            .flat_map(|facing| [false, true].map(|wet| chest_properties(facing, wet)))
            .collect();
        let physical: Vec<_> = properties
            .iter()
            .map(|properties| format!("minecraft:chest[{properties}]"))
            .collect();
        let states: Vec<_> = properties
            .iter()
            .zip(&physical)
            .map(|(properties, physical)| State {
                properties,
                physical,
                model: "minecraft:block/chest",
                rotating_model: "",
                x: 0,
                y: 0,
                orientation: [0., 0., 0., 1.],
                shape: &CHEST_SHAPE,
            })
            .collect();
        api.content.add_block(
            registry,
            &Block {
                id: "lootr:chest",
                display_name: "Loot Chest",
                entity: "lootr:chest",
                item_model: "minecraft:item/chest",
                states: &states,
            },
            Some(placed),
        )?;
        api.interaction.on_use_block(registry, used)?;
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
}

fn assign_chest_id(bytes: &[u8], id: &str) -> Option<Vec<u8>> {
    // Preserve metadata owned by the container, including fields we do not interpret.
    let mut fields: std::collections::HashMap<String, fastnbt::Value> =
        fastnbt::from_bytes(bytes).ok()?;
    fields.insert("LootrId".into(), fastnbt::Value::String(id.into()));
    fastnbt::to_bytes(&fields).ok()
}

fn is_chest(content: Content, world: World<'_>, pos: moddedmc_api::Pos) -> bool {
    let Some(expected) = chest_states(content) else {
        return false;
    };
    let mut states = [moddedmc_api::content::UNLOADED_STATE];
    world.read(&[pos], &mut states) && expected.contains(&states[0])
}

fn chest_data(world: World<'_>, pos: moddedmc_api::Pos) -> Option<ChestData> {
    let Some(bytes) = world.entity(pos) else {
        trace("world.entity missing");
        return None;
    };
    let mut data: ChestData = match fastnbt::from_bytes(&bytes) {
        Ok(data) => data,
        Err(error) => {
            trace(&format!("chest NBT decoding: {error}"));
            return None;
        }
    };
    if data.id.is_empty() {
        data.id = format!("{:032x}", rand::random::<u128>());
        let bytes = match assign_chest_id(&bytes, &data.id) {
            Some(bytes) => bytes,
            None => {
                trace("chest NBT encoding failed");
                return None;
            }
        };
        if !world.set_entity(pos, &bytes) {
            trace("world.set_entity failed");
            return None;
        }
    }
    trace("chest_data decoded");
    state::valid_id(&data.id).then_some(data)
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

fn chest_matches(world: World<'_>, site: &state::Site, id: &str) -> bool {
    if world.dimension_id() != Some(site.dimension.as_str()) {
        return false;
    }
    let [x, y, z] = site.position;
    let pos = moddedmc_api::Pos { x, y, z };
    if !API
        .get()
        .is_some_and(|api| is_chest(api.content, world, pos))
    {
        return false;
    }
    world
        .entity(pos)
        .and_then(|bytes| fastnbt::from_bytes::<ChestData>(&bytes).ok())
        .is_some_and(|chest| chest.id == id)
}

fn site_matches(world: World<'_>, record: &state::Record) -> bool {
    record
        .site
        .as_ref()
        .is_some_and(|site| chest_matches(world, site, &record.container))
}

#[derive(serde::Deserialize, serde::Serialize)]
struct ViewContext {
    site: state::Site,
    container: String,
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
    if !chest_matches(view.world, &context.site, &context.container) {
        trace("validity chest identity failed");
        return false;
    }
    true
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
    if !is_chest(runtime.api.content, world, pos) {
        return failed("container block state changed");
    }
    if runtime.api.mobs.game_mode(entity).is_none_or(|mode| mode == 3) {
        return BlockUseResult::Success;
    }
    // The Java chest handles sneak-use before checking overhead obstruction.
    // Visual opener markers are not implemented yet; never claim an inventory here.
    if context.sneaking {
        return BlockUseResult::Success;
    }
    let Some(above_y) = pos.y.checked_add(1) else {
        return BlockUseResult::Success;
    };
    let above = moddedmc_api::Pos { y: above_y, ..pos };
    if world.redstone_conductor(above) != Some(false) {
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
    let (index, id, inventory, validity_context) = {
        let Ok(_guard) = LOCK.lock() else {
            return failed("archive lock poisoned");
        };
        if !is_chest(runtime.api.content, world, pos) {
            return failed("container state changed before claim");
        }
        let Some(chest) = chest_data(world, pos) else {
            return failed("chest_data failed");
        };
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
        let Ok(validity_context) = fastnbt::to_bytes(&ViewContext {
            site,
            container: chest.id,
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
        (index, id, inventory, validity_context)
    };
    if runtime.api.menus.open(
        &player,
        runtime.menu,
        inventory,
        "Loot Chest",
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
