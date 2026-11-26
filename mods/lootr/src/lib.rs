//! Lootr per-player inventory storage.
mod shards;
pub mod state;

use moddedmc_api::{
    InterfaceUnavailable, Mod, Player, RegistrationError, Registry,
    container_loot::{self, ContainerLoot},
    content::{self, Block, Content, State, World, WorldExtension},
    entities::{self, EntitiesExtension},
    interaction::{self, BlockUseResult, Interaction, InteractionExtension, UseBlock},
    inventory_menu::{self, Access, Change, MenuKind, Validated, Validity, ValiditySlots},
    native_entity::{self, Actors},
    saved_data::{self, Key, SavedData},
};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

#[derive(Clone, Copy)]
struct Interfaces {
    content: Content,
    interaction: Interaction,
    menus: Validated,
    actors: Actors,
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

// Temporary bounded diagnostics for the first real stock-client smoke test.
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
        let interaction = interaction::require()?;
        interaction.require_extension(InteractionExtension::BlockUse)?;
        entities::require()?.require_extension(EntitiesExtension::Player)?;
        let _ = API.set(Interfaces {
            content,
            interaction,
            menus: inventory_menu::require_validity()?,
            actors: native_entity::require()?,
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
        api.content.add_block(
            registry,
            &Block {
                id: "lootr:chest",
                display_name: "Loot Chest",
                entity: "lootr:chest",
                item_model: "minecraft:item/chest",
                states: &[State {
                    properties: "",
                    physical: "minecraft:chest[facing=north,type=single,waterlogged=false]",
                    model: "minecraft:block/chest",
                    rotating_model: "",
                    x: 0,
                    y: 0,
                    orientation: [0., 0., 0., 1.],
                    shape: &[],
                }],
            },
            None,
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

fn uuid(api: Interfaces, player: &Player<'_>) -> Option<[u8; 16]> {
    #[derive(serde::Deserialize)]
    struct Profile {
        id: fastnbt::IntArray,
    }
    let Some(bytes) = api.actors.player_profile(player) else {
        trace("player_profile returned no bytes");
        return None;
    };
    let profile: Profile = match fastnbt::from_bytes(&bytes) {
        Ok(profile) => profile,
        Err(error) => {
            trace(&format!("profile NBT decoding: {error}"));
            return None;
        }
    };
    let Ok(words): Result<&[i32; 4], _> = (&profile.id[..]).try_into() else {
        trace("profile id is not four integers");
        return None;
    };
    let mut result = [0; 16];
    for (chunk, word) in result.as_chunks_mut::<4>().0.iter_mut().zip(words) {
        chunk.copy_from_slice(&word.to_be_bytes());
    }
    Some(result)
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

#[derive(Default, serde::Deserialize, serde::Serialize)]
struct ChestData {
    #[serde(rename = "LootrId", default)]
    id: String,
    #[serde(rename = "LootTable", default)]
    table: Option<String>,
    #[serde(rename = "LootTableSeed", default)]
    seed: i64,
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
        let bytes = match fastnbt::to_bytes(&data) {
            Ok(bytes) => bytes,
            Err(error) => {
                trace(&format!("chest NBT encoding: {error}"));
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
    trace("block use hook entered");
    let Some(runtime) = RUNTIME.get().copied() else {
        return failed("runtime missing");
    };
    let Some(player) = context.player.and_then(|p| p.as_player()) else {
        return failed("use context has no player");
    };
    let Some(owner) = uuid(runtime.api, &player) else {
        return failed("owner UUID missing");
    };
    let world = context.world;
    let pos = context.position;
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
