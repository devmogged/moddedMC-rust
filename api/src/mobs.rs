//! Mob types for mods: register one carried by a vanilla mob, spawn it, and keep the mod's state
//! on each mob across restarts.

use std::sync::OnceLock;

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
}

/// Hears a mod mob die, before its loot drops.
pub type DeathHandler = fn(world: World<'_>, mob: Entity<'_>);

static DEATH: OnceLock<DeathHandler> = OnceLock::new();

pub fn require() -> Result<Mobs, InterfaceUnavailable> {
    // SAFETY: this is the published table type for moddedmc:mobs v1; appended fields are read
    // only after require_extension checks the size.
    unsafe { InterfacePrefix::require(abi::MOBS, 1, abi::MOBS_V1_BASE_SIZE) }.map(Mobs)
}

unsafe extern "C" fn death(world: abi::WorldRef, mob: abi::EntityRef) {
    guard("mob death", (), || {
        if let (Some(handler), Some(world), Some(mob)) =
            (DEATH.get(), World::borrowed(world), Entity::borrowed(mob))
        {
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
        };
        if self.0.size >= end + size_of::<usize>() {
            Ok(())
        } else {
            Err(InterfaceUnavailable { name, version: 1 })
        }
    }

    // `on_death` to `make_persistent` need `MobsExtension::Taming`.

    /// Hear mobs of `kind` die. A mod has one handler: the first one passed serves every kind.
    pub fn on_death(
        self,
        reg: &mut Registry<'_>,
        kind: &str,
        handler: DeathHandler,
    ) -> Result<(), RegistrationError> {
        let Some(api) = self.0.full() else {
            return Err(RegistrationError::InvalidDefinition);
        };
        let _ = DEATH.set(handler);
        // SAFETY: registration is live for the call and the trampoline is static.
        RegistrationError::from_code(unsafe { (api.on_death)(reg.reg, s(kind), death) })
    }

    pub fn uuid(self, entity: Entity<'_>) -> Option<[u8; 16]> {
        let api = self.0.full()?;
        let mut out = [0; 16];
        // SAFETY: the entity is live for this callback and `out` outlives the call.
        unsafe { (api.uuid)(entity.raw(), &raw mut out) }.then_some(out)
    }

    /// The loaded entity with `uuid` in `world`.
    pub fn by_uuid(self, world: World<'_>, uuid: &[u8; 16]) -> Option<i32> {
        let api = self.0.full()?;
        // SAFETY: the world and the UUID are borrowed for the call.
        let id = unsafe { (api.by_uuid)(world.raw(), uuid) };
        (id >= 0).then_some(id)
    }

    /// Health and max health of a living entity.
    pub fn health(self, entity: Entity<'_>) -> Option<(f32, f32)> {
        let api = self.0.full()?;
        let mut max = 0.0;
        // SAFETY: the entity is live for this callback and `max` outlives the call.
        let health = unsafe { (api.health)(entity.raw(), &raw mut max) };
        (health >= 0.0).then_some((health, max))
    }

    pub fn heal(self, entity: Entity<'_>, amount: f32) -> bool {
        let Some(api) = self.0.full() else {
            return false;
        };
        // SAFETY: the entity is live for this callback.
        unsafe { (api.heal)(entity.raw(), amount) }
    }

    /// Direct passengers, the controlling one first.
    pub fn passengers(self, entity: Entity<'_>) -> Vec<i32> {
        let Some(api) = self.0.full() else {
            return Vec::new();
        };
        let mut ids = vec![0; 8];
        loop {
            // SAFETY: the entity is live and `ids` has the capacity passed.
            let count = unsafe { (api.passengers)(entity.raw(), ids.as_mut_ptr(), ids.len()) };
            if count <= ids.len() {
                ids.truncate(count);
                return ids;
            }
            ids.resize(count, 0);
        }
    }

    /// Hearts when `tamed`, smoke otherwise.
    pub fn tame_effect(self, world: World<'_>, entity: Entity<'_>, tamed: bool) -> bool {
        let Some(api) = self.0.full() else {
            return false;
        };
        // SAFETY: the world and entity are live for this callback.
        unsafe { (api.tame_effect)(world.raw(), entity.raw(), tamed) }
    }

    pub fn set_no_gravity(self, entity: Entity<'_>, no_gravity: bool) -> bool {
        let Some(api) = self.0.full() else {
            return false;
        };
        // SAFETY: the entity is live for this callback.
        unsafe { (api.set_no_gravity)(entity.raw(), no_gravity) }
    }

    /// Set it burning for `ticks`; 0 puts it out.
    pub fn set_on_fire(self, entity: Entity<'_>, ticks: u32) -> bool {
        let Some(api) = self.0.full() else {
            return false;
        };
        // SAFETY: the entity is live for this callback.
        unsafe { (api.set_on_fire)(entity.raw(), ticks) }
    }

    pub fn reset_hurt_cooldown(self, entity: Entity<'_>) -> bool {
        let Some(api) = self.0.full() else {
            return false;
        };
        // SAFETY: the entity is live for this callback.
        unsafe { (api.reset_hurt_cooldown)(entity.raw()) }
    }

    /// A player's held movement keys (`PLAYER_INPUT_*` bits).
    pub fn player_input(self, player: Entity<'_>) -> Option<i32> {
        let api = self.0.full()?;
        // SAFETY: the entity is live for this callback.
        let bits = unsafe { (api.player_input)(player.raw()) };
        (bits >= 0).then_some(bits)
    }

    /// 0 survival, 1 creative, 2 adventure, 3 spectator.
    pub fn game_mode(self, player: Entity<'_>) -> Option<i32> {
        let api = self.0.full()?;
        // SAFETY: the entity is live for this callback.
        let mode = unsafe { (api.game_mode)(player.raw()) };
        (mode >= 0).then_some(mode)
    }

    /// Plain text in the player's chat, or above the hotbar.
    pub fn message(self, player: Entity<'_>, text: &str, action_bar: bool) -> bool {
        let Some(api) = self.0.full() else {
            return false;
        };
        // SAFETY: the entity is live and the text borrowed for the call.
        unsafe { (api.message)(player.raw(), s(text), action_bar) }
    }

    /// Keep a mob from naturally despawning.
    pub fn make_persistent(self, mob: Entity<'_>) -> bool {
        let Some(api) = self.0.full() else {
            return false;
        };
        // SAFETY: the mob is live for this callback.
        unsafe { (api.make_persistent)(mob.raw()) }
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
