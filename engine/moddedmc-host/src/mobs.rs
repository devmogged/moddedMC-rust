//! `moddedmc:mobs`: mod mob types carried by vanilla mobs. A spawned mob's custom data
//! (`data.moddedmc`) names its type and holds the mod's state, so both are saved with it.

use std::{
    collections::BTreeMap,
    io::Cursor,
    ptr,
    sync::{Arc, Mutex, PoisonError, atomic::Ordering},
};

use moddedmc_api::abi::{self, EntityRef, Reg, Sink, Str, Vec3, WorldRef, code};
use pumpkin_core::{
    entity::mob::spawn::EntitySpawnReason,
    entity::{
        EntityBase,
        r#type::{from_type, load_entity},
    },
    world::{
        World, install_structure_entity_loader,
        natural_spawner::{EmbedderSpawn, EmbedderSpawns, install_embedder_spawns, pool},
    },
};
use pumpkin_data::{
    attributes::Attributes,
    biome::Biome,
    data_component_impl::combat::attribute_from_name,
    entity::{EntityType, MobCategory, SpawnLocation},
    structure_registry::{biome_tag, full_id},
};
use pumpkin_nbt::{Nbt, compound::NbtCompound, deserializer::NbtReadHelperJava, tag::NbtTag};
use pumpkin_util::math::{position::BlockPos, vector3::Vector3};
use uuid::Uuid;

use crate::{
    MAX_STATE_BYTES,
    content::{entity, shared, world},
    native,
};

pub static MOBS: abi::MobsV1 = abi::MobsV1 {
    size: size_of::<abi::MobsV1>(),
    add_mob,
    spawn,
    kind,
    state,
    set_state,
    on_death: crate::mob_ai::on_death,
    uuid,
    by_uuid,
    health,
    heal,
    passengers,
    tame_effect,
    set_no_gravity,
    set_on_fire,
    reset_hurt_cooldown,
    player_input,
    game_mode,
    message,
    make_persistent,
    add_spawn,
    open_trades,
    award_advancement,
    kill_credit,
    player_sleeping,
    award_experience,
    award_stat,
    boolean_gamerule,
    last_hurt_mob,
    last_hurt_by_mob,
    on_merchant_close: crate::mob_ai::on_merchant_close,
    pet_owner,
    team,
    allied,
    set_seat_offset,
    set_dismount_offsets,
};

unsafe extern "C" fn pet_owner(raw: EntityRef, out: *mut [u8; 16]) -> bool {
    // SAFETY: entity and output are borrowed for the call.
    let (Some(base), Some(out)) = (unsafe { entity(raw) }, unsafe { out.as_mut() }) else {
        return false;
    };
    let Some(owner) = base.get_mob().and_then(pumpkin_core::entity::mob::Mob::get_owner_uuid) else {
        return false;
    };
    *out = *owner.as_bytes();
    true
}

unsafe extern "C" fn team(raw: EntityRef, out: Sink) -> bool {
    // SAFETY: the entity and sink are borrowed for this call.
    let Some(name) = (unsafe { entity(raw) }).and_then(EntityBase::get_team_name) else {
        return false;
    };
    // SAFETY: the sink copies the borrowed name synchronously.
    unsafe { (out.put)(out.ctx, 0, Str::new(name.as_bytes())) };
    true
}

unsafe extern "C" fn allied(raw: EntityRef, other: EntityRef) -> bool {
    // SAFETY: both entities are borrowed for the call.
    let (Some(base), Some(other)) = (unsafe { entity(raw) }, unsafe { entity(other) }) else {
        return false;
    };
    base.is_allied_to(other)
}

unsafe extern "C" fn set_seat_offset(raw: EntityRef, index: usize, offset: Vec3) -> bool {
    // SAFETY: the entity is borrowed for the call.
    let Some(base) = (unsafe { entity(raw) }) else {
        return false;
    };
    if registered_kind(base).is_none()
        || index >= 16
        || !offset.0.iter().all(|v| v.is_finite() && v.abs() <= 64.0)
    {
        return false;
    }
    base.get_entity().set_custom_seat(index, offset.0);
    true
}

unsafe extern "C" fn set_dismount_offsets(
    raw: EntityRef,
    offsets: *const Vec3,
    count: usize,
) -> bool {
    // SAFETY: the entity is borrowed for the call.
    let Some(base) = (unsafe { entity(raw) }) else {
        return false;
    };
    if registered_kind(base).is_none() || count > 512 || (count != 0 && offsets.is_null()) {
        return false;
    }
    let offsets = if count == 0 {
        &[]
    } else {
        // SAFETY: a nonempty input is borrowed from the mod for the call.
        unsafe { std::slice::from_raw_parts(offsets, count) }
    };
    if !offsets
        .iter()
        .all(|v| v.0.iter().all(|n| n.is_finite() && n.abs() <= 64.0))
    {
        return false;
    }
    base.get_entity()
        .set_custom_dismount(offsets.iter().map(|v| v.0).collect());
    true
}

unsafe extern "C" fn last_hurt_mob(raw: EntityRef, timestamp: *mut i64) -> i32 {
    // SAFETY: the entity is borrowed for this call.
    let Some((id, game_time)) = (unsafe { entity(raw) })
        .and_then(EntityBase::get_living_entity)
        .and_then(pumpkin_core::entity::living::LivingEntity::get_last_hurt_mob)
    else {
        return -1;
    };
    // SAFETY: a non-null output slot is writable for the call.
    if let Some(out) = unsafe { timestamp.as_mut() } {
        *out = game_time;
    }
    id
}

unsafe extern "C" fn last_hurt_by_mob(raw: EntityRef, timestamp: *mut i64) -> i32 {
    // SAFETY: the entity is borrowed for this call.
    let Some((id, game_time)) = (unsafe { entity(raw) })
        .and_then(EntityBase::get_living_entity)
        .and_then(pumpkin_core::entity::living::LivingEntity::get_last_hurt_by_mob_game_time)
    else {
        return -1;
    };
    // SAFETY: a non-null output slot is writable for the call.
    if let Some(out) = unsafe { timestamp.as_mut() } {
        *out = game_time;
    }
    id
}

#[cfg(test)]
mod combat_history_tests {
    use super::*;

    #[test]
    fn absent_entity_does_not_write_combat_history_timestamp() {
        let mut timestamp = 123;
        // SAFETY: null is the absent entity handle; the timestamp is writable.
        unsafe {
            assert_eq!(last_hurt_mob(ptr::null(), &raw mut timestamp), -1);
            assert_eq!(last_hurt_by_mob(ptr::null(), &raw mut timestamp), -1);
        }
        assert_eq!(timestamp, 123);
    }
}

unsafe extern "C" fn kill_credit(raw: EntityRef) -> i32 {
    // SAFETY: the entity is borrowed for this call.
    unsafe { entity(raw) }
        .and_then(EntityBase::get_living_entity)
        .and_then(pumpkin_core::entity::living::LivingEntity::get_kill_credit)
        .map_or(-1, |credit| credit.get_entity().entity_id)
}

unsafe extern "C" fn player_sleeping(raw: EntityRef) -> i32 {
    // SAFETY: the entity is borrowed for this call.
    unsafe { entity(raw) }
        .and_then(EntityBase::get_player)
        .map_or(-1, |player| i32::from(player.is_sleeping()))
}

unsafe extern "C" fn award_experience(raw: WorldRef, position: Vec3, amount: u32) -> bool {
    if amount == 0 || !position.0.iter().all(|v| v.is_finite()) {
        return false;
    }
    // SAFETY: the world is borrowed for this call.
    let Some(world) = (unsafe { world(raw) }).and_then(shared) else {
        return false;
    };
    let [x, y, z] = position.0;
    pumpkin_core::entity::experience_orb::ExperienceOrbEntity::spawn(
        &world,
        Vector3::new(x, y, z),
        amount,
    );
    true
}

unsafe extern "C" fn award_stat(raw: EntityRef, category: Str, stat: Str, amount: i32) -> bool {
    use pumpkin_data::statistic::StatisticCategory;
    // SAFETY: all inputs are borrowed for this call.
    let (Some(player), Some(category), stat) = (
        unsafe { entity(raw) }.and_then(EntityBase::get_player),
        StatisticCategory::from_registry_key(unsafe { category.str() }),
        unsafe { stat.str() },
    ) else {
        return false;
    };
    if amount <= 0 {
        return false;
    }
    let id = match category {
        StatisticCategory::Custom => {
            static STATS: std::sync::OnceLock<Option<serde_json::Value>> =
                std::sync::OnceLock::new();
            let stats = STATS.get_or_init(|| {
                serde_json::from_str(include_str!("../../pumpkin/assets/stats.json")).ok()
            });
            stats
                .as_ref()
                .and_then(|stats| stats["minecraft:custom"]["entries"][stat]["id"].as_i64())
                .and_then(|id| i32::try_from(id).ok())
        }
        StatisticCategory::Killed | StatisticCategory::KilledBy => {
            EntityType::from_name(stat.strip_prefix("minecraft:").unwrap_or(stat))
                .map(|kind| i32::from(kind.id))
        }
        StatisticCategory::Mined => {
            pumpkin_data::Block::from_registry_key(stat).map(|block| i32::from(block.id.as_u16()))
        }
        _ => pumpkin_data::item::Item::from_registry_key(stat).map(|item| i32::from(item.id)),
    };
    let Some(id) = id else { return false };
    player.increment_stat(category, id, amount);
    true
}

unsafe extern "C" fn boolean_gamerule(raw: WorldRef, rule: Str) -> i32 {
    use pumpkin_data::game_rules::{GameRule, GameRuleValue};
    // SAFETY: both inputs are borrowed for this call.
    let Some(world) = (unsafe { world(raw) }) else {
        return -1;
    };
    // SAFETY: the string is borrowed for this call.
    let name = unsafe { rule.str() };
    let name = name.strip_prefix("minecraft:").unwrap_or(name);
    let Some(rule) = GameRule::all().iter().find(|rule| rule.to_string() == name) else {
        return -1;
    };
    match world.get_game_rule(rule) {
        GameRuleValue::Bool(value) => i32::from(value),
        GameRuleValue::Int(_) => -1,
    }
}

pub static MOB_LOOT: abi::MobLootV1 = abi::MobLootV1 {
    size: size_of::<abi::MobLootV1>(),
    suppress: suppress_loot,
};

unsafe extern "C" fn suppress_loot(raw: EntityRef) -> bool {
    // SAFETY: the entity is borrowed for this callback.
    let Some(base) = (unsafe { entity(raw) }) else {
        return false;
    };
    let Some(mob) = base.get_mob().filter(|_| registered_kind(base).is_some()) else {
        return false;
    };
    *mob.get_mob_entity()
        .death_loot_table
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = Some("minecraft:empty".to_owned());
    true
}

pub static MOB_STORAGE: abi::MobStorageV1 = abi::MobStorageV1 {
    size: size_of::<abi::MobStorageV1>(),
    save: save_mob,
    restore: restore_mob,
};

unsafe extern "C" fn save_mob(raw: EntityRef, out: Sink) -> bool {
    // SAFETY: the entity is borrowed for this call.
    let Some(base) = (unsafe { entity(raw) })
        .filter(|base| registered_kind(*base).is_some() && base.get_entity().is_alive())
    else {
        return false;
    };
    let mut saved = NbtCompound::new();
    base.write_nbt(&mut saved);
    saved.child_tags.remove("Passengers");
    let bytes = Nbt::from(saved).write();
    if bytes.len() > abi::MAX_STACK_BYTES {
        return false;
    }
    // SAFETY: the bytes and sink live through this synchronous call.
    unsafe { (out.put)(out.ctx, 0, Str::new(&bytes)) };
    true
}

unsafe extern "C" fn restore_mob(raw: WorldRef, kind: Str, saved: Str, at: Vec3) -> i32 {
    if saved.len > abi::MAX_STACK_BYTES || !at.0.into_iter().all(f64::is_finite) {
        return -1;
    }
    // SAFETY: world, kind and NBT are borrowed for this call.
    let (Some(world), kind, bytes) =
        (unsafe { (world(raw).and_then(shared), kind.str(), saved.bytes()) })
    else {
        return -1;
    };
    restore_saved(&world, kind, bytes, at.0).unwrap_or(-1)
}

fn restore_saved(world: &Arc<World>, kind: &str, bytes: &[u8], at: [f64; 3]) -> Option<i32> {
    let carrier = { types().get(kind)?.carrier };
    let [x, y, z] = at;
    let cell = BlockPos::new(x.floor() as i32, y.floor() as i32, z.floor() as i32);
    if !world.is_loaded(&cell) {
        return None;
    }
    let mut reader = Cursor::new(bytes);
    let mut saved = Nbt::read(&mut NbtReadHelperJava::new(&mut reader))
        .ok()?
        .root_tag;
    let uuid = saved.get_uuid("UUID")?;
    if reader.position() != bytes.len() as u64
        || vanilla(saved.get_string("id")?) != Some(carrier)
        || world.server.upgrade()?.worlds.load().iter().any(|level| {
            level
                .get_entity_by_uuid(uuid)
                .is_some_and(|entity| !entity.get_entity().removed.load(Ordering::Acquire))
        })
    {
        return None;
    }
    saved.child_tags.remove("Passengers");
    saved.put(
        "Pos",
        NbtTag::List(at.into_iter().map(NbtTag::Double).collect()),
    );
    saved.put("Motion", NbtTag::List(vec![NbtTag::Double(0.0); 3]));
    saved.put_bool("OnGround", false);
    saved.put_double("fall_distance", 0.0);
    let mob = load_entity(carrier, Vector3::new(x, y, z), &saved, world);
    let base = mob.get_entity();
    if registered_kind(mob.as_ref()) != Some(kind)
        || !base.is_alive()
        || !world.no_collision(mob.as_ref(), base.bounding_box.load())
    {
        return None;
    }
    let id = base.entity_id;
    (world.spawn_entity(mob) && world.get_entity_by_id(id).is_some()).then_some(id)
}

pub static MOB_HEALTH: abi::MobHealthV1 = abi::MobHealthV1 {
    size: size_of::<abi::MobHealthV1>(),
    set_health,
    set_attribute_base,
};

unsafe extern "C" fn set_health(raw: EntityRef, health: f32) -> bool {
    // SAFETY: the entity is borrowed for the call.
    let Some(living) = (unsafe { entity(raw) })
        .filter(|base| registered_kind(*base).is_some())
        .and_then(EntityBase::get_living_entity)
    else {
        return false;
    };
    if !health.is_finite()
        || health <= 0.0
        || health > living.get_max_health()
        || living.health.load() <= 0.0
    {
        return false;
    }
    living.set_health(health);
    true
}

unsafe extern "C" fn set_attribute_base(raw: EntityRef, attribute: Str, value: f64) -> bool {
    if !value.is_finite() {
        return false;
    }
    // SAFETY: the identifier and entity are borrowed for the call.
    let (Some(attribute), Some(living)) = (
        attribute_from_name(unsafe { attribute.str() }),
        unsafe { entity(raw) }
            .filter(|base| registered_kind(*base).is_some())
            .and_then(EntityBase::get_living_entity),
    ) else {
        return false;
    };
    living.set_attribute_base(attribute, value);
    true
}

const NAMESPACE: &str = "moddedmc";
const TYPE_KEY: &str = "mob";
const STATE_KEY: &str = "state";

pub struct Registration {
    id: &'static str,
    carrier: &'static EntityType,
    attributes: Vec<(&'static Attributes, f64)>,
    structure_init: Option<abi::StructureMobInitFn>,
    effect_veto: Option<abi::EffectCallback>,
    spawns: Vec<NaturalSpawn>,
}

/// A biome spawn pool entry from `add_spawn`.
struct NaturalSpawn {
    /// A biome id, or `#` and a biome tag.
    biomes: String,
    category: &'static MobCategory,
    weight: i32,
    min_count: i32,
    max_count: i32,
    location: SpawnLocation,
    carrier_rules: bool,
    check: abi::SpawnCheckFn,
}

/// Registered mob types by id. Ids are leaked once at registration so `kind` can return them.
static TYPES: Mutex<BTreeMap<&'static str, Registration>> = Mutex::new(BTreeMap::new());

fn types() -> std::sync::MutexGuard<'static, BTreeMap<&'static str, Registration>> {
    TYPES.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Whether a staged mob uses a vanilla carrier with fish bucket behavior.
pub fn staged_fish(staged: &[Registration], kind: &str) -> bool {
    staged.iter().any(|mob| {
        mob.id == kind
            && [
                &EntityType::COD,
                &EntityType::SALMON,
                &EntityType::TROPICAL_FISH,
                &EntityType::PUFFERFISH,
            ]
            .contains(&mob.carrier)
    })
}

fn vanilla(name: &str) -> Option<&'static EntityType> {
    EntityType::from_name(name.strip_prefix("minecraft:").unwrap_or(name))
}

unsafe extern "C" fn add_mob(reg: Reg, def: *const abi::MobDefinition) -> i32 {
    // SAFETY: the host owns staging and the definition is borrowed by the registration call.
    let (staging, Some(def)) = (unsafe { (native::staging(reg), def.as_ref()) }) else {
        return code::INVALID_DEFINITION;
    };
    // SAFETY: the strings and attribute array belong to the live definition.
    let (id, carrier, attributes) = unsafe {
        (
            def.id.str(),
            def.carrier.str(),
            if def.attribute_count == 0 || def.attributes.is_null() {
                &[][..]
            } else {
                std::slice::from_raw_parts(def.attributes, def.attribute_count)
            },
        )
    };
    // A living carrier other than a player: it has to hold attributes and run goals.
    let Some(carrier) =
        vanilla(carrier).filter(|t| !t.attributes.is_empty() && **t != EntityType::PLAYER)
    else {
        return code::INVALID_DEFINITION;
    };
    let Some(attributes) = attributes
        .iter()
        // SAFETY: the attribute ids belong to the live definition.
        .map(|a| Some((attribute_from_name(unsafe { a.id.str() })?, a.value)))
        .filter(|a| a.is_none_or(|(_, value)| value.is_finite()))
        .collect::<Option<Vec<_>>>()
    else {
        return code::INVALID_DEFINITION;
    };
    if !crate::valid_id(id) {
        return code::INVALID_DEFINITION;
    }
    if types().contains_key(id) || staging.mobs.iter().any(|m| m.id == id) {
        return code::DUPLICATE_ID;
    }
    staging.mobs.push(Registration {
        id: String::leak(id.to_owned()),
        carrier,
        attributes,
        structure_init: None,
        effect_veto: None,
        spawns: Vec::new(),
    });
    code::OK
}

/// Commit a mod's staged mob types after its registration succeeded.
pub fn register(mobs: Vec<Registration>) {
    let mut types = types();
    for mob in mobs {
        tracing::info!(
            "moddedmc: registered mob {} (carrier minecraft:{})",
            mob.id,
            mob.carrier.resource_name
        );
        types.insert(mob.id, mob);
    }
}

unsafe extern "C" fn spawn(raw: WorldRef, kind: Str, position: Vec3) -> i32 {
    // SAFETY: the world and the kind are borrowed for this call.
    let (Some(world), kind) = (unsafe { world(raw) }.and_then(shared), unsafe {
        kind.str()
    }) else {
        return -1;
    };
    let abi::Vec3([x, y, z]) = position;
    if !(x.is_finite() && y.is_finite() && z.is_finite()) {
        return -1;
    }
    let Some(mob) = create(kind, &world, Vector3::new(x, y, z)) else {
        return -1;
    };
    let id = mob.get_entity().entity_id;
    if world.spawn_entity(mob) { id } else { -1 }
}

/// Initialize a mod mob before it is spawned in the world.
pub fn create(
    kind: &str,
    world: &Arc<World>,
    position: Vector3<f64>,
) -> Option<Arc<dyn EntityBase>> {
    let types = types();
    let registered = types.get(kind);
    let carrier = registered
        .map(|mob| mob.carrier)
        .or_else(|| vanilla(kind))?;
    let mob = from_type(carrier, position, world, Uuid::new_v4());
    mob.get_mob()?;
    if let Some(registered) = registered {
        let entity = mob.get_entity();
        entity.set_custom_data(NAMESPACE, TYPE_KEY, NbtTag::String(registered.id.into()));
        if let Some(living) = mob.get_living_entity() {
            for (attribute, value) in &registered.attributes {
                living.set_attribute_base(attribute, *value);
            }
            if let Some((_, health)) = registered
                .attributes
                .iter()
                .find(|(a, _)| **a == Attributes::MAX_HEALTH)
            {
                living.set_health(*health as f32);
            }
        }
    }
    Some(mob)
}

unsafe extern "C" fn add_spawn(
    reg: Reg,
    def: *const abi::SpawnDefinition,
    check: abi::SpawnCheckFn,
) -> i32 {
    // SAFETY: the host owns staging and the definition is borrowed by the registration call.
    let (staging, Some(def)) = (unsafe { (native::staging(reg), def.as_ref()) }) else {
        return code::INVALID_DEFINITION;
    };
    // SAFETY: the strings belong to the live definition.
    let (kind, biomes, category) =
        unsafe { (def.kind.str(), def.biomes.str(), def.category.str()) };
    let category = match category {
        "monster" => &MobCategory::MONSTER,
        "creature" => &MobCategory::CREATURE,
        "ambient" => &MobCategory::AMBIENT,
        "axolotls" => &MobCategory::AXOLOTLS,
        "underground_water_creature" => &MobCategory::UNDERGROUND_WATER_CREATURE,
        "water_creature" => &MobCategory::WATER_CREATURE,
        "water_ambient" => &MobCategory::WATER_AMBIENT,
        _ => return code::INVALID_DEFINITION,
    };
    let location = match def.location {
        abi::spawn_location::UNRESTRICTED => SpawnLocation::Unrestricted,
        abi::spawn_location::ON_GROUND => SpawnLocation::OnGround,
        abi::spawn_location::IN_WATER => SpawnLocation::InWater,
        abi::spawn_location::IN_LAVA => SpawnLocation::InLava,
        _ => return code::INVALID_DEFINITION,
    };
    let Some(mob) = staging.mobs.iter_mut().find(|mob| mob.id == kind) else {
        return code::INVALID_DEFINITION;
    };
    if def.weight < 0 || def.min_count < 1 || def.max_count < def.min_count || biomes.is_empty() {
        return code::INVALID_DEFINITION;
    }
    mob.spawns.push(NaturalSpawn {
        biomes: biomes.to_owned(),
        category,
        weight: def.weight,
        min_count: def.min_count,
        max_count: def.max_count,
        location,
        carrier_rules: def.carrier_rules,
        check,
    });
    code::OK
}

pub fn install_natural_spawns() -> bool {
    install_embedder_spawns(EmbedderSpawns {
        entries: spawn_entries,
        check: spawn_check,
        create: |world, spawn, position| create(spawn.id, world, position),
    })
}

/// Each biome and category's mod entries, built on first use after registration.
static POOLS: Mutex<BTreeMap<(u8, usize), &'static [EmbedderSpawn]>> = Mutex::new(BTreeMap::new());

fn spawn_entries(
    biome: &'static Biome,
    category: &'static MobCategory,
) -> &'static [EmbedderSpawn] {
    let mut pools = POOLS.lock().unwrap_or_else(PoisonError::into_inner);
    pools.entry((biome.id, category.id)).or_insert_with(|| {
        let types = types();
        let entries = types
            .values()
            .flat_map(|mob| mob.spawns.iter().map(move |spawn| (mob, spawn)))
            .filter(|(_, spawn)| spawn.category == category && in_biomes(&spawn.biomes, biome))
            .map(|(mob, spawn)| EmbedderSpawn {
                id: mob.id,
                carrier: mob.carrier,
                category,
                location: spawn.location.clone(),
                weight: spawn.weight,
                min_count: spawn.min_count,
                max_count: spawn.max_count,
                carrier_rules: spawn.carrier_rules,
            })
            .collect::<Vec<_>>();
        Box::leak(entries.into_boxed_slice())
    })
}

pub fn in_biomes(biomes: &str, biome: &Biome) -> bool {
    biomes.strip_prefix('#').map_or_else(
        || full_id(biomes) == full_id(biome.registry_id),
        |tag| biome_tag(tag).is_some_and(|ids| ids.contains(&u16::from(biome.id))),
    )
}

fn spawn_check(
    world: &Arc<World>,
    spawn: &'static EmbedderSpawn,
    pos: &BlockPos,
    reason: EntitySpawnReason,
) -> bool {
    let Some(check) = types()
        .get(spawn.id)
        .and_then(|mob| mob.spawns.first())
        .map(|first| first.check)
    else {
        return false;
    };
    let biome = world.level.get_rough_biome(pos);
    let name = full_id(biome.registry_id);
    let vanilla_weight = pool(biome, spawn.category)
        .iter()
        .filter(|entry| entry.max_count > 0)
        .map(|entry| entry.weight.max(0))
        .sum();
    let check_data = abi::SpawnCheck {
        kind: Str::new(spawn.id.as_bytes()),
        position: abi::Pos {
            x: pos.0.x,
            y: pos.0.y,
            z: pos.0.z,
        },
        biome: Str::new(name.as_bytes()),
        vanilla_weight,
        chunk_generation: matches!(reason, EntitySpawnReason::ChunkGeneration),
    };
    // SAFETY: the world and the check outlive this synchronous callback.
    unsafe { check(ptr::from_ref::<World>(world).cast(), &raw const check_data) }
}

pub static STRUCTURE_MOBS: abi::StructureMobsV1 = abi::StructureMobsV1 {
    size: size_of::<abi::StructureMobsV1>(),
    on_load: on_structure_mob,
};

unsafe extern "C" fn on_structure_mob(
    raw: Reg,
    kind: Str,
    callback: abi::StructureMobInitFn,
) -> i32 {
    // SAFETY: registration and the kind identifier remain live for the call.
    let (staging, kind) = unsafe { (native::staging(raw), kind.str()) };
    let Some(mob) = staging.mobs.iter_mut().find(|mob| mob.id == kind) else {
        return code::INVALID_DEFINITION;
    };
    if mob.structure_init.is_some() {
        return code::DUPLICATE_ID;
    }
    mob.structure_init = Some(callback);
    code::OK
}

pub fn install_structure_mobs() -> bool {
    install_structure_entity_loader(load_structure_mob)
}

fn load_structure_mob(world: &Arc<World>, nbt: &NbtCompound) -> Option<Arc<dyn EntityBase>> {
    let kind = nbt.get_string("id")?;
    let init = types().get(kind)?.structure_init;
    let [x, y, z] = nbt.get_list("Pos")? else {
        return None;
    };
    let at = [
        x.extract_double()?,
        y.extract_double()?,
        z.extract_double()?,
    ];
    if !at.into_iter().all(f64::is_finite) {
        return None;
    }

    let mob = create(kind, world, Vector3::new(at[0], at[1], at[2]))?;
    if let Some([yaw, pitch]) = nbt.get_list("Rotation")
        && let (Some(yaw), Some(pitch)) = (yaw.extract_float(), pitch.extract_float())
        && yaw.is_finite()
        && pitch.is_finite()
    {
        mob.get_entity().set_rotation(yaw, pitch);
    }
    if nbt.get_bool("PersistenceRequired") == Some(true) {
        mob.get_mob()?.get_mob_entity().set_persistence_required();
    }
    if let Some(living) = mob.get_living_entity()
        && let Some(health) = nbt
            .get_float("Health")
            .filter(|health| health.is_finite() && *health > 0.0)
    {
        living.set_health(health.min(living.get_max_health()));
    }
    if let Some(init) = init {
        let bytes = Nbt::from(nbt.clone()).write();
        if bytes.len() <= MAX_STATE_BYTES {
            let entity = mob.as_ref();
            // SAFETY: the borrowed entity and the NBT bytes outlive the synchronous call.
            unsafe { init(ptr::from_ref(&entity).cast(), Str::new(&bytes)) };
        }
    }
    Some(mob)
}

/// Give loaded mobs the default for their mod type, including mobs saved before the override.
pub fn default_loot(mob: &pumpkin_core::entity::mob::MobEntity, kind: &str) {
    if let Some((namespace, path)) = kind.split_once(':') {
        mob.death_loot_table
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get_or_insert_with(|| format!("{namespace}:entities/{path}"));
    }
}

/// The registered type a mob's custom data names, if that type is still registered.
pub fn registered_kind(base: &dyn EntityBase) -> Option<&'static str> {
    let NbtTag::String(name) = base.get_entity().get_custom_data(NAMESPACE, TYPE_KEY)? else {
        return None;
    };
    types().get_key_value(&*name).map(|(id, _)| *id)
}

pub unsafe extern "C" fn on_effect(reg: Reg, kind: Str, callback: abi::EffectCallback) -> i32 {
    // SAFETY: the registration and kind are borrowed for the call.
    let (staging, kind) = unsafe { (native::staging(reg), kind.str()) };
    let Some(mob) = staging.mobs.iter_mut().find(|mob| mob.id == kind) else {
        return code::INVALID_DEFINITION;
    };
    if mob.effect_veto.is_some() {
        return code::DUPLICATE_ID;
    }
    mob.effect_veto = Some(callback);
    code::OK
}

pub fn effect_veto(base: &dyn EntityBase) -> Option<abi::EffectCallback> {
    let NbtTag::String(kind) = base.get_entity().get_custom_data(NAMESPACE, TYPE_KEY)? else {
        return None;
    };
    types().get(&*kind)?.effect_veto
}

/// True when `kind` is a registered mod mob type or one of `staged`.
pub fn known(staged: &[Registration], kind: &str) -> bool {
    types().contains_key(kind) || staged.iter().any(|m| m.id == kind)
}

unsafe extern "C" fn kind(raw: EntityRef) -> Str {
    // SAFETY: the entity is live for the callback.
    let id = unsafe { entity(raw) }
        .and_then(registered_kind)
        .unwrap_or("");
    Str::new(id.as_bytes())
}

unsafe extern "C" fn state(raw: EntityRef, out: Sink) -> bool {
    // SAFETY: the entity is live for the callback.
    let Some(base) = (unsafe { entity(raw) }).filter(|b| registered_kind(*b).is_some()) else {
        return false;
    };
    let bytes: Vec<u8> = match base.get_entity().get_custom_data(NAMESPACE, STATE_KEY) {
        Some(NbtTag::ByteArray(bytes)) => bytes.iter().map(|&b| b as u8).collect(),
        _ => Vec::new(),
    };
    // SAFETY: the sink is valid for this call.
    unsafe { (out.put)(out.ctx, 0, Str::new(&bytes)) };
    true
}

unsafe extern "C" fn set_state(raw: EntityRef, state: Str) -> bool {
    // SAFETY: the entity and bytes are borrowed for this call.
    let (Some(base), bytes) = (unsafe { entity(raw) }, unsafe { state.bytes() }) else {
        return false;
    };
    if bytes.len() > MAX_STATE_BYTES || registered_kind(base).is_none() {
        return false;
    }
    let bytes: Box<[i8]> = bytes.iter().map(|&b| b as i8).collect();
    base.get_entity()
        .set_custom_data(NAMESPACE, STATE_KEY, NbtTag::ByteArray(bytes));
    true
}

unsafe extern "C" fn uuid(raw: EntityRef, out: *mut [u8; 16]) -> bool {
    // SAFETY: the entity is live for the callback and a non-null `out` is writable.
    let (Some(base), Some(out)) = (unsafe { entity(raw) }, unsafe { out.as_mut() }) else {
        return false;
    };
    *out = *base.get_entity().entity_uuid.as_bytes();
    true
}

unsafe extern "C" fn by_uuid(raw: WorldRef, id: *const [u8; 16]) -> i32 {
    // SAFETY: the world and the UUID are borrowed for the call.
    let (Some(world), Some(id)) = (unsafe { world(raw) }, unsafe { id.as_ref() }) else {
        return -1;
    };
    let id = Uuid::from_bytes(*id);
    world
        .get_player_by_uuid(id)
        .map(|player| player.entity_id())
        .or_else(|| {
            world
                .get_entity_by_uuid(id)
                .map(|e| e.get_entity().entity_id)
        })
        .unwrap_or(-1)
}

unsafe extern "C" fn health(raw: EntityRef, max: *mut f32) -> f32 {
    // SAFETY: the entity is live for the callback.
    let Some(living) = (unsafe { entity(raw) }).and_then(EntityBase::get_living_entity) else {
        return -1.0;
    };
    // SAFETY: a non-null `max` is writable for the call.
    if let Some(max) = unsafe { max.as_mut() } {
        *max = living.get_max_health();
    }
    living.health.load()
}

unsafe extern "C" fn heal(raw: EntityRef, amount: f32) -> bool {
    // SAFETY: the entity is live for the callback.
    let Some(base) = (unsafe { entity(raw) }) else {
        return false;
    };
    if !amount.is_finite() || amount <= 0.0 {
        return false;
    }
    match (base.get_player(), base.get_living_entity()) {
        (Some(player), _) => player.heal(amount),
        (None, Some(living)) => living.heal(amount),
        _ => return false,
    }
    true
}

unsafe extern "C" fn passengers(raw: EntityRef, out: *mut i32, capacity: usize) -> usize {
    // SAFETY: the entity is live for the callback.
    let Some(base) = (unsafe { entity(raw) }) else {
        return 0;
    };
    let passengers = base
        .get_entity()
        .passengers
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    if !out.is_null() {
        for (i, passenger) in passengers.iter().take(capacity).enumerate() {
            // SAFETY: `out` has room for `capacity` ids and `i` is below it.
            unsafe { out.add(i).write(passenger.get_entity().entity_id) };
        }
    }
    passengers.len()
}

unsafe extern "C" fn tame_effect(raw_world: WorldRef, raw: EntityRef, tamed: bool) -> bool {
    use pumpkin_data::entity::EntityStatus;
    use pumpkin_protocol::bedrock::server::actor_event::ActorEventID;
    // SAFETY: the world and the entity are live for the callback.
    let (Some(world), Some(base)) = (unsafe { world(raw_world) }, unsafe { entity(raw) }) else {
        return false;
    };
    let (java, bedrock) = if tamed {
        (EntityStatus::TamingSucceeded, ActorEventID::TamingSucceeded)
    } else {
        (EntityStatus::TamingFailed, ActorEventID::TamingFailed)
    };
    world.send_entity_status(base.get_entity(), java, bedrock);
    true
}

unsafe extern "C" fn set_no_gravity(raw: EntityRef, no_gravity: bool) -> bool {
    // SAFETY: the entity is live for the callback.
    let Some(base) = (unsafe { entity(raw) }) else {
        return false;
    };
    base.get_entity().set_has_no_gravity(no_gravity);
    true
}

unsafe extern "C" fn set_on_fire(raw: EntityRef, ticks: u32) -> bool {
    // SAFETY: the entity is live for the callback.
    let Some(base) = (unsafe { entity(raw) }) else {
        return false;
    };
    if ticks == 0 {
        base.get_entity().extinguish();
    } else {
        base.set_on_fire_for_ticks(ticks);
    }
    true
}

unsafe extern "C" fn reset_hurt_cooldown(raw: EntityRef) -> bool {
    // SAFETY: the entity is live for the callback.
    let Some(living) = (unsafe { entity(raw) }).and_then(EntityBase::get_living_entity) else {
        return false;
    };
    living
        .hurt_cooldown
        .store(0, std::sync::atomic::Ordering::Relaxed);
    true
}

unsafe extern "C" fn player_input(raw: EntityRef) -> i32 {
    // SAFETY: the entity is live for the callback.
    (unsafe { entity(raw) })
        .and_then(EntityBase::get_player)
        .map_or(-1, |player| {
            i32::from(
                player
                    .last_input
                    .load(std::sync::atomic::Ordering::Relaxed)
                    .cast_unsigned(),
            )
        })
}

unsafe extern "C" fn game_mode(raw: EntityRef) -> i32 {
    use pumpkin_util::GameMode;
    // SAFETY: the entity is live for the callback.
    (unsafe { entity(raw) })
        .and_then(EntityBase::get_player)
        .map_or(-1, |player| match player.gamemode.load() {
            GameMode::Survival => 0,
            GameMode::Creative => 1,
            GameMode::Adventure => 2,
            GameMode::Spectator => 3,
        })
}

unsafe extern "C" fn message(raw: EntityRef, text: Str, action_bar: bool) -> bool {
    // SAFETY: the entity is live and the text borrowed for the call.
    let (Some(player), text) = (
        unsafe { entity(raw) }.and_then(EntityBase::get_player),
        unsafe { text.str() },
    ) else {
        return false;
    };
    player.send_system_message_raw(
        &pumpkin_util::text::TextComponent::text(text.to_owned()),
        action_bar,
    );
    true
}

unsafe extern "C" fn open_trades(mob: EntityRef, player: EntityRef, offers: Str) -> bool {
    use pumpkin_core::entity::passive::villager::VillagerEntity;
    // SAFETY: both entities are live and the bytes borrowed for the call.
    let (Some(mob), Some(player), offers) = (
        unsafe { entity(mob) }.and_then(EntityBase::get_mob),
        unsafe { entity(player) }.and_then(EntityBase::get_player),
        unsafe { offers.bytes() },
    ) else {
        return false;
    };
    let Some(villager) = mob.cast_any().downcast_ref::<VillagerEntity>() else {
        return false;
    };
    let Some(player) = player.world().get_player_by_uuid(player.gameprofile.id) else {
        return false;
    };
    if !offers.is_empty() {
        let mut reader = Cursor::new(offers);
        let Ok(saved) = Nbt::read(&mut NbtReadHelperJava::new(&mut reader)) else {
            return false;
        };
        villager.set_offers_from_nbt(&saved.root_tag);
    }
    let before = player.screen_handler_sync_id.load(Ordering::Relaxed);
    villager.open_trading_screen(&player);
    let current = player
        .current_screen_handler
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    let (sync_id, window_type) = {
        let handler = current.lock().unwrap_or_else(PoisonError::into_inner);
        (handler.sync_id(), handler.window_type())
    };
    if sync_id == before || window_type != Some(pumpkin_data::screen::WindowType::Merchant) {
        return false;
    }
    if registered_kind(mob)
        .and_then(crate::mob_ai::merchant_close_callback)
        .is_some()
    {
        let mut trades = OPEN_TRADES.lock().unwrap_or_else(PoisonError::into_inner);
        trades.retain(|_, (screen, _)| screen.strong_count() != 0);
        trades.insert(
            player.gameprofile.id,
            (Arc::downgrade(&current), mob.get_entity().entity_uuid),
        );
    }
    true
}

type TradeScreen = std::sync::Weak<Mutex<dyn pumpkin_inventory::screen_handler::ScreenHandler>>;
static OPEN_TRADES: Mutex<BTreeMap<Uuid, (TradeScreen, Uuid)>> = Mutex::new(BTreeMap::new());

pub struct MerchantCloseHandler;

impl
    pumpkin_core::plugin::EventHandler<
        pumpkin_core::plugin::api::events::player::inventory_close::InventoryCloseEvent,
    > for MerchantCloseHandler
{
    fn handle_blocking<'a>(
        &'a self,
        _server: &'a Arc<pumpkin_core::server::Server>,
        event: &'a mut pumpkin_core::plugin::api::events::player::inventory_close::InventoryCloseEvent,
    ) -> pumpkin_core::plugin::BoxFuture<'a, ()> {
        Box::pin(async move {
            if event.window_type != Some(pumpkin_data::screen::WindowType::Merchant) {
                return;
            }
            let player = &event.player;
            let trade = OPEN_TRADES
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(&player.gameprofile.id);
            let Some((screen, merchant)) = trade else {
                return;
            };
            let current = player
                .current_screen_handler
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone();
            if !screen
                .upgrade()
                .is_some_and(|screen| Arc::ptr_eq(&screen, &current))
            {
                return;
            }
            let world = player.world();
            let Some(merchant) = world.get_entity_by_uuid(merchant) else {
                return;
            };
            let Some(callback) =
                registered_kind(merchant.as_ref()).and_then(crate::mob_ai::merchant_close_callback)
            else {
                return;
            };
            let player: &dyn EntityBase = player.as_ref();
            let merchant: &dyn EntityBase = merchant.as_ref();
            // SAFETY: world and both entity handles are live for the synchronous callback.
            unsafe {
                callback(
                    ptr::from_ref(world.as_ref()).cast(),
                    ptr::from_ref(&player).cast(),
                    ptr::from_ref(&merchant).cast(),
                );
            };
        })
    }
}

unsafe extern "C" fn award_advancement(
    player: EntityRef,
    advancement: Str,
    criterion: Str,
) -> bool {
    use pumpkin_core::entity::player::advancement::PlayerAdvancement;
    // SAFETY: the player is live and the strings borrowed for the call.
    let (Some(player), id, criterion) = (
        unsafe { entity(player) }.and_then(EntityBase::get_player),
        unsafe { advancement.str() },
        unsafe { criterion.str() },
    ) else {
        return false;
    };
    let Some(advancement) = pumpkin_data::Advancement::from_minecraft_name(id) else {
        return false;
    };
    if advancement.get_criterion(criterion).is_none() {
        return false;
    }
    let Some(player) = player.world().get_player_by_uuid(player.gameprofile.id) else {
        return false;
    };
    let result = player
        .advancements
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .award(advancement, criterion);
    // Completion events and rewards re-enter the advancement lock, so they run after it.
    PlayerAdvancement::finish_award(&player, advancement, result);
    if result.awarded() {
        player
            .advancements
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .flush_dirty(&player, true);
    }
    result.awarded()
}

unsafe extern "C" fn make_persistent(raw: EntityRef) -> bool {
    // SAFETY: the entity is live for the callback.
    let Some(mob) = (unsafe { entity(raw) }).and_then(EntityBase::get_mob) else {
        return false;
    };
    mob.get_mob_entity().set_persistence_required();
    true
}
