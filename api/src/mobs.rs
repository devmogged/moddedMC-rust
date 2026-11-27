//! Mob types for mods: register one carried by a vanilla mob, spawn it, and keep the mod's state
//! on each mob across restarts.

use std::{
    collections::BTreeMap,
    sync::{Mutex, OnceLock, PoisonError},
};

use crate::{
    InterfacePrefix, InterfaceUnavailable, RegistrationError, Registry, abi, content::World,
    entities::Entity, guard, s,
};

pub use abi::{
    PLAYER_INPUT_BACKWARD, PLAYER_INPUT_FORWARD, PLAYER_INPUT_JUMP, PLAYER_INPUT_LEFT,
    PLAYER_INPUT_RIGHT, PLAYER_INPUT_SNEAK, PLAYER_INPUT_SPRINT,
};

/// Most bytes a mob's saved state may hold.
pub const MAX_STATE: usize = 64 * 1024;

#[derive(Clone, Copy)]
pub struct MobLoot(&'static abi::MobLootV1);

pub fn require_loot() -> Result<MobLoot, InterfaceUnavailable> {
    // SAFETY: this is the complete published mob_loot v1 table.
    unsafe { crate::require_interface(abi::MOB_LOOT, 1) }.map(MobLoot)
}

impl MobLoot {
    /// Suppress the registered mob's death loot table. Saved with the mob.
    pub fn suppress(self, mob: Entity<'_>) -> bool {
        // SAFETY: the mob is borrowed for this callback.
        unsafe { (self.0.suppress)(mob.raw()) }
    }
}

#[derive(Clone, Copy)]
pub struct StructureMobs(&'static abi::StructureMobsV1);

pub type StructureMobInit = fn(Entity<'_>, &[u8]);

static STRUCTURE_MOB_INIT: OnceLock<StructureMobInit> = OnceLock::new();

pub fn require_structure_mobs() -> Result<StructureMobs, InterfaceUnavailable> {
    // SAFETY: this is the complete published structure_mobs v1 table.
    unsafe { crate::require_interface(abi::STRUCTURE_MOBS, 1) }.map(StructureMobs)
}

unsafe extern "C" fn structure_mob_init(raw: abi::EntityRef, nbt: abi::Str) {
    guard("structure mob initialization", (), || {
        if let (Some(init), Some(mob)) = (STRUCTURE_MOB_INIT.get(), Entity::borrowed(raw)) {
            // SAFETY: the host owns the named-root NBT bytes for this callback.
            init(mob, unsafe { nbt.bytes() });
        }
    });
}

impl StructureMobs {
    /// Register initialization for a staged mob type. The callback receives named-root NBT,
    /// up to 64 KiB, from the structure template. The mob has not yet joined the world.
    pub fn on_load(
        self,
        reg: &mut Registry<'_>,
        kind: &str,
        callback: StructureMobInit,
    ) -> Result<(), RegistrationError> {
        let installed = STRUCTURE_MOB_INIT.get_or_init(|| callback);
        if !std::ptr::fn_addr_eq(*installed, callback) {
            return Err(RegistrationError::InvalidDefinition);
        }
        // SAFETY: registration and kind are borrowed for the call. The trampoline is static.
        RegistrationError::from_code(unsafe {
            (self.0.on_load)(reg.reg, s(kind), structure_mob_init)
        })
    }
}

#[derive(Clone, Copy)]
pub struct MobStorage(&'static abi::MobStorageV1);

pub fn require_storage() -> Result<MobStorage, InterfaceUnavailable> {
    // SAFETY: this is the published table for moddedmc:mob_storage v1.
    unsafe { crate::require_interface(abi::MOB_STORAGE, 1) }.map(MobStorage)
}

impl MobStorage {
    pub fn save(self, mob: Entity<'_>) -> Option<Vec<u8>> {
        let mut bytes = Vec::new();
        let sink = abi::Sink {
            ctx: (&raw mut bytes).cast(),
            put: crate::content::bytes,
        };
        // SAFETY: the borrowed mob and local sink remain live for the call.
        unsafe { (self.0.save)(mob.raw(), sink) }.then_some(bytes)
    }

    pub fn restore(self, world: World<'_>, kind: &str, nbt: &[u8], at: [f64; 3]) -> Option<i32> {
        // SAFETY: the world and input bytes are borrowed for the call.
        let id =
            unsafe { (self.0.restore)(world.raw(), s(kind), abi::Str::new(nbt), abi::Vec3(at)) };
        (id >= 0).then_some(id)
    }
}

#[derive(Clone, Copy)]
pub struct FishBuckets(&'static abi::FishBucketsV1);

pub fn require_fish_buckets() -> Result<FishBuckets, InterfaceUnavailable> {
    // SAFETY: this is the published table type for moddedmc:fish_buckets v1.
    unsafe { crate::require_interface(abi::FISH_BUCKETS, 1) }.map(FishBuckets)
}

impl FishBuckets {
    /// Bind a custom cod-bucket item to a registered fish mob. Both must be staged in
    /// the same registration call before binding. Each item and mob can have one binding.
    pub fn bind(
        self,
        reg: &mut Registry<'_>,
        item: &str,
        mob: &str,
    ) -> Result<(), RegistrationError> {
        // SAFETY: registration and both identifiers are borrowed for this call.
        RegistrationError::from_code(unsafe { (self.0.bind)(reg.reg, s(item), s(mob)) })
    }
}

#[derive(Clone, Copy)]
pub struct MobHealth(InterfacePrefix<abi::MobHealthV1>);

pub fn require_health() -> Result<MobHealth, InterfaceUnavailable> {
    // SAFETY: this is the published table type for moddedmc:mob_health v1.
    unsafe {
        InterfacePrefix::require(
            abi::MOB_HEALTH,
            1,
            std::mem::offset_of!(abi::MobHealthV1, set_attribute_base),
        )
    }
    .map(MobHealth)
}

impl MobHealth {
    pub fn require_attributes(self) -> Result<(), InterfaceUnavailable> {
        if self.0.size < size_of::<abi::MobHealthV1>() {
            return Err(InterfaceUnavailable {
                name: "moddedmc:mob_health.set_attribute_base",
                version: 1,
            });
        }
        Ok(())
    }

    pub fn set_attribute(self, mob: Entity<'_>, name: &str, value: f64) -> bool {
        // SAFETY: the table size is checked and the entity and identifier are borrowed.
        self.require_attributes().is_ok()
            && unsafe {
                ((&raw const (*self.0.ptr).set_attribute_base).read())(mob.raw(), s(name), value)
            }
    }

    /// Set a registered mob's health directly, with no attack or heal event.
    pub fn set(self, mob: Entity<'_>, health: f32) -> bool {
        // SAFETY: the entity is borrowed for the call.
        unsafe { ((&raw const (*self.0.ptr).set_health).read())(mob.raw(), health) }
    }
}

#[derive(Clone, Copy)]
pub struct Mobs(InterfacePrefix<abi::MobsV1>);

/// A mob type to register.
pub struct Mob<'a> {
    pub id: &'a str,
    /// The vanilla mob that carries it, such as `minecraft:ravager`.
    pub carrier: &'a str,
    /// Base attribute values, such as `("minecraft:max_health", 40.0)`.
    pub attributes: &'a [(&'a str, f64)],
}

/// Calls appended to `moddedmc:mobs` v1 after its first release.
#[derive(Clone, Copy)]
pub enum MobsExtension {
    /// Death, UUIDs, health, passengers, gravity, fire, the hurt cooldown and the player calls
    /// taming and riding need.
    Taming,
    /// `add_spawn`: biome spawn pool entries.
    NaturalSpawns,
    /// `Mobs::open_trades`.
    Trading,
    /// `Mobs::award_advancement`.
    Advancements,
    /// Game-time combat history in both directions.
    CombatHistory,
}

#[cfg(test)]
mod combat_history_tests {
    use super::*;

    #[test]
    fn combat_history_requires_both_appended_fields() {
        for size in [224, 232] {
            let mobs = Mobs(InterfacePrefix {
                ptr: std::ptr::null(),
                size,
            });
            assert!(
                mobs.require_extension(MobsExtension::CombatHistory)
                    .is_err()
            );
        }
        let mobs = Mobs(InterfacePrefix {
            ptr: std::ptr::null(),
            size: size_of::<abi::MobsV1>(),
        });
        assert!(mobs.require_extension(MobsExtension::CombatHistory).is_ok());
    }
}

/// Where a spawn entry's mob can appear (vanilla `SpawnPlacementTypes`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpawnLocation {
    Unrestricted,
    OnGround,
    InWater,
    InLava,
}

/// A vanilla `MobCategory`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpawnCategory {
    Monster,
    Creature,
    Ambient,
    Axolotls,
    UndergroundWaterCreature,
    WaterCreature,
    WaterAmbient,
}

impl SpawnCategory {
    const fn name(self) -> &'static str {
        match self {
            Self::Monster => "monster",
            Self::Creature => "creature",
            Self::Ambient => "ambient",
            Self::Axolotls => "axolotls",
            Self::UndergroundWaterCreature => "underground_water_creature",
            Self::WaterCreature => "water_creature",
            Self::WaterAmbient => "water_ambient",
        }
    }
}

/// A biome spawn pool entry for a mob type this mod registered.
pub struct Spawn<'a> {
    pub kind: &'a str,
    /// A biome id, or `#` and a biome tag.
    pub biomes: &'a str,
    pub category: SpawnCategory,
    pub weight: i32,
    pub min_count: i32,
    pub max_count: i32,
    pub location: SpawnLocation,
    /// Also apply the carrier's vanilla spawn rules before the mod's check.
    pub carrier_rules: bool,
}

/// A position the spawner is trying.
pub struct SpawnCheck<'a> {
    pub kind: &'a str,
    pub position: crate::Pos,
    pub biome: &'a str,
    /// Summed weight of the vanilla entries in the same category's pool there.
    pub vanilla_weight: i32,
    pub chunk_generation: bool,
}

/// A mod mob's spawn rule. One per mod: the first one passed serves every spawn.
pub type SpawnRule = fn(world: World<'_>, spawn: &SpawnCheck<'_>) -> bool;

static SPAWN_RULE: OnceLock<SpawnRule> = OnceLock::new();

unsafe extern "C" fn spawn_rule(world: abi::WorldRef, check: *const abi::SpawnCheck) -> bool {
    guard("spawn rule", false, || {
        // SAFETY: the host passes a live check for this call.
        let (Some(rule), Some(world), Some(check)) =
            (SPAWN_RULE.get(), World::borrowed(world), unsafe {
                check.as_ref()
            })
        else {
            return false;
        };
        // SAFETY: the check's strings are live for the call.
        let (kind, biome) = unsafe { (check.kind.str(), check.biome.str()) };
        rule(
            world,
            &SpawnCheck {
                kind,
                position: check.position,
                biome,
                vanilla_weight: check.vanilla_weight,
                chunk_generation: check.chunk_generation,
            },
        )
    })
}

/// Hears a mod mob die, before its loot drops.
pub type DeathHandler = fn(world: World<'_>, mob: Entity<'_>);

// Death callbacks are per *mob kind*, not per dynamic library. A single mod
// can register different death behaviors (e.g. Spiky Bug versus Ambusher).
static DEATH: Mutex<Option<BTreeMap<String, DeathHandler>>> = Mutex::new(None);

fn death_callbacks() -> std::sync::MutexGuard<'static, Option<BTreeMap<String, DeathHandler>>> {
    DEATH.lock().unwrap_or_else(PoisonError::into_inner)
}

fn death_handler(kind: &str) -> Option<DeathHandler> {
    death_callbacks().as_ref()?.get(kind).copied()
}

pub fn require() -> Result<Mobs, InterfaceUnavailable> {
    // SAFETY: this is the published table type for moddedmc:mobs v1; appended fields are read
    // only after require_extension checks the size.
    unsafe { InterfacePrefix::require(abi::MOBS, 1, abi::MOBS_V1_BASE_SIZE) }.map(Mobs)
}

unsafe extern "C" fn death(world: abi::WorldRef, mob: abi::EntityRef) {
    guard("mob death", (), || {
        let (Some(world), Some(mob)) = (World::borrowed(world), Entity::borrowed(mob)) else {
            return;
        };
        let Some(kind) = require().ok().and_then(|mobs| mobs.kind(mob)) else {
            return;
        };
        if let Some(handler) = death_handler(kind) {
            handler(world, mob);
        }
    });
}

impl Mobs {
    pub fn require_extension(self, extension: MobsExtension) -> Result<(), InterfaceUnavailable> {
        let (end, name) = match extension {
            MobsExtension::Taming => (
                std::mem::offset_of!(abi::MobsV1, make_persistent),
                "moddedmc:mobs.taming",
            ),
            MobsExtension::NaturalSpawns => (
                std::mem::offset_of!(abi::MobsV1, add_spawn),
                "moddedmc:mobs.natural_spawns",
            ),
            MobsExtension::Trading => (
                std::mem::offset_of!(abi::MobsV1, open_trades),
                "moddedmc:mobs.trading",
            ),
            MobsExtension::Advancements => (
                std::mem::offset_of!(abi::MobsV1, award_advancement),
                "moddedmc:mobs.advancements",
            ),
            MobsExtension::CombatHistory => (
                std::mem::offset_of!(abi::MobsV1, last_hurt_by_mob),
                "moddedmc:mobs.combat_history",
            ),
        };
        if self.0.size >= end + size_of::<usize>() {
            Ok(())
        } else {
            Err(InterfaceUnavailable { name, version: 1 })
        }
    }

    // `on_death` to `make_persistent` need `MobsExtension::Taming`.

    /// Add `spawn` to the biome spawn pools, with `rule` deciding each position. Needs
    /// `MobsExtension::NaturalSpawns`.
    pub fn add_spawn(
        self,
        reg: &mut Registry<'_>,
        spawn: &Spawn<'_>,
        rule: SpawnRule,
    ) -> Result<(), RegistrationError> {
        if self
            .require_extension(MobsExtension::NaturalSpawns)
            .is_err()
        {
            return Err(RegistrationError::InvalidDefinition);
        }
        let _ = SPAWN_RULE.set(rule);
        let def = abi::SpawnDefinition {
            kind: s(spawn.kind),
            biomes: s(spawn.biomes),
            category: s(spawn.category.name()),
            weight: spawn.weight,
            min_count: spawn.min_count,
            max_count: spawn.max_count,
            location: match spawn.location {
                SpawnLocation::Unrestricted => abi::spawn_location::UNRESTRICTED,
                SpawnLocation::OnGround => abi::spawn_location::ON_GROUND,
                SpawnLocation::InWater => abi::spawn_location::IN_WATER,
                SpawnLocation::InLava => abi::spawn_location::IN_LAVA,
            },
            carrier_rules: spawn.carrier_rules,
        };
        // SAFETY: the size is checked; registration and the definition are live for the call.
        RegistrationError::from_code(unsafe {
            ((&raw const (*self.0.ptr).add_spawn).read())(reg.reg, &raw const def, spawn_rule)
        })
    }

    /// Open a villager carrier's trading screen for `player`. Non-empty `offers` (vanilla's saved
    /// `Offers` compound as NBT) replace its offers first. Needs `MobsExtension::Trading`.
    pub fn open_trades(self, mob: Entity<'_>, player: Entity<'_>, offers: &[u8]) -> bool {
        if self.require_extension(MobsExtension::Trading).is_err() {
            return false;
        }
        // SAFETY: the size is checked; both entities and the bytes are live for the call.
        unsafe {
            ((&raw const (*self.0.ptr).open_trades).read())(
                mob.raw(),
                player.raw(),
                abi::Str::new(offers),
            )
        }
    }

    /// Grant `criterion` of the advancement `advancement` (a full id, such as a datapack's
    /// `mymod:tame_dragon`) to `player`. True when this granted it. Needs
    /// `MobsExtension::Advancements`.
    pub fn award_advancement(self, player: Entity<'_>, advancement: &str, criterion: &str) -> bool {
        if self.require_extension(MobsExtension::Advancements).is_err() {
            return false;
        }
        // SAFETY: the size is checked; the player and strings are live for the call.
        unsafe {
            ((&raw const (*self.0.ptr).award_advancement).read())(
                player.raw(),
                abi::Str::new(advancement.as_bytes()),
                abi::Str::new(criterion.as_bytes()),
            )
        }
    }

    /// Last living target damaged by this entity, as (entity id, game time).
    pub fn last_hurt_mob(self, mob: Entity<'_>) -> Option<(i32, i64)> {
        let api = self
            .0
            .upto(std::mem::offset_of!(abi::MobsV1, last_hurt_mob))?;
        let mut timestamp = 0;
        // SAFETY: the field is present, the entity is borrowed and the output is writable.
        let id =
            unsafe { ((&raw const (*api).last_hurt_mob).read())(mob.raw(), &raw mut timestamp) };
        (id >= 0).then_some((id, timestamp))
    }

    /// Remembered living attacker, as (entity id, game time).
    pub fn last_hurt_by_mob(self, mob: Entity<'_>) -> Option<(i32, i64)> {
        let api = self
            .0
            .upto(std::mem::offset_of!(abi::MobsV1, last_hurt_by_mob))?;
        let mut timestamp = 0;
        // SAFETY: the field is present, the entity is borrowed and the output is writable.
        let id =
            unsafe { ((&raw const (*api).last_hurt_by_mob).read())(mob.raw(), &raw mut timestamp) };
        (id >= 0).then_some((id, timestamp))
    }

    /// Vanilla kill credit, including the remembered attacker during its timeout.
    pub fn kill_credit(self, mob: Entity<'_>) -> Option<i32> {
        let api = self
            .0
            .upto(std::mem::offset_of!(abi::MobsV1, kill_credit))?;
        // SAFETY: the field is present and the entity is borrowed.
        let id = unsafe { ((&raw const (*api).kill_credit).read())(mob.raw()) };
        (id >= 0).then_some(id)
    }

    /// Whether a player is sleeping.
    pub fn player_sleeping(self, player: Entity<'_>) -> Option<bool> {
        let api = self
            .0
            .upto(std::mem::offset_of!(abi::MobsV1, player_sleeping))?;
        // SAFETY: the field is present and the entity is borrowed.
        match unsafe { ((&raw const (*api).player_sleeping).read())(player.raw()) } {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        }
    }

    /// Award experience at a world position.
    pub fn award_experience(
        self,
        world: World<'_>,
        position: [f64; 3],
        amount: u32,
    ) -> Option<bool> {
        let api = self
            .0
            .upto(std::mem::offset_of!(abi::MobsV1, award_experience))?;
        // SAFETY: the field is present and the world is borrowed.
        Some(unsafe {
            ((&raw const (*api).award_experience).read())(world.raw(), abi::Vec3(position), amount)
        })
    }

    /// Increment a vanilla statistic identified by its category and registry key.
    pub fn award_stat(
        self,
        player: Entity<'_>,
        category: &str,
        stat: &str,
        amount: i32,
    ) -> Option<bool> {
        let api = self.0.upto(std::mem::offset_of!(abi::MobsV1, award_stat))?;
        // SAFETY: the field is present and all inputs live through the call.
        Some(unsafe {
            ((&raw const (*api).award_stat).read())(
                player.raw(),
                abi::Str::new(category.as_bytes()),
                abi::Str::new(stat.as_bytes()),
                amount,
            )
        })
    }

    /// Read a boolean gamerule by its vanilla registry key.
    pub fn boolean_gamerule(self, world: World<'_>, rule: &str) -> Option<bool> {
        let api = self
            .0
            .upto(std::mem::offset_of!(abi::MobsV1, boolean_gamerule))?;
        // SAFETY: the field is present and both inputs live through the call.
        match unsafe {
            ((&raw const (*api).boolean_gamerule).read())(
                world.raw(),
                abi::Str::new(rule.as_bytes()),
            )
        } {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        }
    }

    /// Hear deaths for one registered mob kind. Different kinds can have
    /// independent handlers, dispatched by the mob's persisted native identity.
    pub fn on_death(
        self,
        reg: &mut Registry<'_>,
        kind: &str,
        handler: DeathHandler,
    ) -> Result<(), RegistrationError> {
        let Some(api) = self.0.upto(std::mem::offset_of!(abi::MobsV1, on_death)) else {
            return Err(RegistrationError::InvalidDefinition);
        };
        {
            let mut guard = death_callbacks();
            let callbacks = guard.get_or_insert_with(BTreeMap::new);
            if callbacks.contains_key(kind) {
                return Err(RegistrationError::DuplicateId);
            }
            callbacks.insert(kind.to_owned(), handler);
        }
        // SAFETY: registration is live for the call and the trampoline is static.
        let registered = RegistrationError::from_code(unsafe {
            ((&raw const (*api).on_death).read())(reg.reg, s(kind), death)
        });
        if registered.is_err()
            && let Some(callbacks) = death_callbacks().as_mut()
        {
            callbacks.remove(kind);
        }
        registered
    }

    pub fn uuid(self, entity: Entity<'_>) -> Option<[u8; 16]> {
        let api = self.0.upto(std::mem::offset_of!(abi::MobsV1, uuid))?;
        let mut out = [0; 16];
        // SAFETY: the entity is live for this callback and `out` outlives the call.
        unsafe { ((&raw const (*api).uuid).read())(entity.raw(), &raw mut out) }.then_some(out)
    }

    /// The loaded entity with `uuid` in `world`.
    pub fn by_uuid(self, world: World<'_>, uuid: &[u8; 16]) -> Option<i32> {
        let api = self.0.upto(std::mem::offset_of!(abi::MobsV1, by_uuid))?;
        // SAFETY: the world and the UUID are borrowed for the call.
        let id = unsafe { ((&raw const (*api).by_uuid).read())(world.raw(), uuid) };
        (id >= 0).then_some(id)
    }

    /// Health and max health of a living entity.
    pub fn health(self, entity: Entity<'_>) -> Option<(f32, f32)> {
        let api = self.0.upto(std::mem::offset_of!(abi::MobsV1, health))?;
        let mut max = 0.0;
        // SAFETY: the entity is live for this callback and `max` outlives the call.
        let health = unsafe { ((&raw const (*api).health).read())(entity.raw(), &raw mut max) };
        (health >= 0.0).then_some((health, max))
    }

    pub fn heal(self, entity: Entity<'_>, amount: f32) -> bool {
        let Some(api) = self.0.upto(std::mem::offset_of!(abi::MobsV1, heal)) else {
            return false;
        };
        // SAFETY: the entity is live for this callback.
        unsafe { ((&raw const (*api).heal).read())(entity.raw(), amount) }
    }

    /// Direct passengers, the controlling one first.
    pub fn passengers(self, entity: Entity<'_>) -> Vec<i32> {
        let Some(api) = self.0.upto(std::mem::offset_of!(abi::MobsV1, passengers)) else {
            return Vec::new();
        };
        let mut ids = vec![0; 8];
        loop {
            // SAFETY: the entity is live and `ids` has the capacity passed.
            let count = unsafe {
                ((&raw const (*api).passengers).read())(entity.raw(), ids.as_mut_ptr(), ids.len())
            };
            if count <= ids.len() {
                ids.truncate(count);
                return ids;
            }
            ids.resize(count, 0);
        }
    }

    /// Hearts when `tamed`, smoke otherwise.
    pub fn tame_effect(self, world: World<'_>, entity: Entity<'_>, tamed: bool) -> bool {
        let Some(api) = self.0.upto(std::mem::offset_of!(abi::MobsV1, tame_effect)) else {
            return false;
        };
        // SAFETY: the world and entity are live for this callback.
        unsafe { ((&raw const (*api).tame_effect).read())(world.raw(), entity.raw(), tamed) }
    }

    pub fn set_no_gravity(self, entity: Entity<'_>, no_gravity: bool) -> bool {
        let Some(api) = self
            .0
            .upto(std::mem::offset_of!(abi::MobsV1, set_no_gravity))
        else {
            return false;
        };
        // SAFETY: the entity is live for this callback.
        unsafe { ((&raw const (*api).set_no_gravity).read())(entity.raw(), no_gravity) }
    }

    /// Set it burning for `ticks`; 0 puts it out.
    pub fn set_on_fire(self, entity: Entity<'_>, ticks: u32) -> bool {
        let Some(api) = self.0.upto(std::mem::offset_of!(abi::MobsV1, set_on_fire)) else {
            return false;
        };
        // SAFETY: the entity is live for this callback.
        unsafe { ((&raw const (*api).set_on_fire).read())(entity.raw(), ticks) }
    }

    pub fn reset_hurt_cooldown(self, entity: Entity<'_>) -> bool {
        let Some(api) = self
            .0
            .upto(std::mem::offset_of!(abi::MobsV1, reset_hurt_cooldown))
        else {
            return false;
        };
        // SAFETY: the entity is live for this callback.
        unsafe { ((&raw const (*api).reset_hurt_cooldown).read())(entity.raw()) }
    }

    /// A player's held movement keys (`PLAYER_INPUT_*` bits).
    pub fn player_input(self, player: Entity<'_>) -> Option<i32> {
        let api = self
            .0
            .upto(std::mem::offset_of!(abi::MobsV1, player_input))?;
        // SAFETY: the entity is live for this callback.
        let bits = unsafe { ((&raw const (*api).player_input).read())(player.raw()) };
        (bits >= 0).then_some(bits)
    }

    /// 0 survival, 1 creative, 2 adventure, 3 spectator.
    pub fn game_mode(self, player: Entity<'_>) -> Option<i32> {
        let api = self.0.upto(std::mem::offset_of!(abi::MobsV1, game_mode))?;
        // SAFETY: the entity is live for this callback.
        let mode = unsafe { ((&raw const (*api).game_mode).read())(player.raw()) };
        (mode >= 0).then_some(mode)
    }

    /// Plain text in the player's chat, or above the hotbar.
    pub fn message(self, player: Entity<'_>, text: &str, action_bar: bool) -> bool {
        let Some(api) = self.0.upto(std::mem::offset_of!(abi::MobsV1, message)) else {
            return false;
        };
        // SAFETY: the entity is live and the text borrowed for the call.
        unsafe { ((&raw const (*api).message).read())(player.raw(), s(text), action_bar) }
    }

    /// Keep a mob from naturally despawning.
    pub fn make_persistent(self, mob: Entity<'_>) -> bool {
        let Some(api) = self
            .0
            .upto(std::mem::offset_of!(abi::MobsV1, make_persistent))
        else {
            return false;
        };
        // SAFETY: the mob is live for this callback.
        unsafe { ((&raw const (*api).make_persistent).read())(mob.raw()) }
    }

    pub fn add_mob(self, reg: &mut Registry<'_>, mob: &Mob<'_>) -> Result<(), RegistrationError> {
        let attributes: Vec<abi::MobAttribute> = mob
            .attributes
            .iter()
            .map(|&(id, value)| abi::MobAttribute { id: s(id), value })
            .collect();
        let def = abi::MobDefinition {
            id: s(mob.id),
            carrier: s(mob.carrier),
            attributes: attributes.as_ptr(),
            attribute_count: attributes.len(),
        };
        // SAFETY: registration and the borrowed definition are live for the call.
        RegistrationError::from_code(unsafe {
            ((&raw const (*self.0.ptr).add_mob).read())(reg.reg, &def)
        })
    }

    /// Spawn a registered mob type or a vanilla mob; its entity id.
    pub fn spawn(self, world: World<'_>, kind: &str, position: [f64; 3]) -> Option<i32> {
        // SAFETY: the world is borrowed for this call.
        let id = unsafe {
            ((&raw const (*self.0.ptr).spawn).read())(world.raw(), s(kind), abi::Vec3(position))
        };
        (id >= 0).then_some(id)
    }

    /// The registered mob type of `entity`, if it is a mod mob.
    pub fn kind(self, entity: Entity<'_>) -> Option<&'static str> {
        // SAFETY: the entity is live for this callback; the host returns a static id.
        let kind = unsafe { ((&raw const (*self.0.ptr).kind).read())(entity.raw()).str() };
        (!kind.is_empty()).then_some(kind)
    }

    /// The mod's saved bytes on a mod mob.
    pub fn state(self, entity: Entity<'_>) -> Option<Vec<u8>> {
        let mut data = Vec::new();
        let sink = abi::Sink {
            ctx: (&raw mut data).cast(),
            put: crate::content::bytes,
        };
        // SAFETY: the sink collects bytes synchronously into the live Vec.
        unsafe { ((&raw const (*self.0.ptr).state).read())(entity.raw(), sink) }.then_some(data)
    }

    /// Replace them, at most [`MAX_STATE`] bytes. False when it isn't a mod mob.
    pub fn set_state(self, entity: Entity<'_>, state: &[u8]) -> bool {
        // SAFETY: the entity and bytes are borrowed for this call.
        unsafe { ((&raw const (*self.0.ptr).set_state).read())(entity.raw(), abi::Str::new(state)) }
    }
}
