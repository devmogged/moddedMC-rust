//! Mob types for mods: register one carried by a vanilla mob, spawn it, and keep the mod's state
//! on each mob across restarts.

use crate::{
    InterfaceUnavailable, RegistrationError, Registry, abi, content::World, entities::Entity,
    require_interface, s,
};

/// Most bytes a mob's saved state may hold.
pub const MAX_STATE: usize = 64 * 1024;

#[derive(Clone, Copy)]
pub struct Mobs(&'static abi::MobsV1);

/// A mob type to register.
pub struct Mob<'a> {
    pub id: &'a str,
    /// The vanilla mob that carries it, such as `minecraft:ravager`.
    pub carrier: &'a str,
    /// Base attribute values, such as `("minecraft:max_health", 40.0)`.
    pub attributes: &'a [(&'a str, f64)],
}

pub fn require() -> Result<Mobs, InterfaceUnavailable> {
    // SAFETY: this is the published table type for moddedmc:mobs v1.
    unsafe { require_interface(abi::MOBS, 1) }.map(Mobs)
}

impl Mobs {
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
        RegistrationError::from_code(unsafe { (self.0.add_mob)(reg.reg, &def) })
    }

    /// Spawn a registered mob type or a vanilla mob; its entity id.
    pub fn spawn(self, world: World<'_>, kind: &str, position: [f64; 3]) -> Option<i32> {
        // SAFETY: the world is borrowed for this call.
        let id = unsafe { (self.0.spawn)(world.raw(), s(kind), abi::Vec3(position)) };
        (id >= 0).then_some(id)
    }

    /// The registered mob type of `entity`, if it is a mod mob.
    pub fn kind(self, entity: Entity<'_>) -> Option<&'static str> {
        // SAFETY: the entity is live for this callback; the host returns a static id.
        let kind = unsafe { (self.0.kind)(entity.raw()).str() };
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
        unsafe { (self.0.state)(entity.raw(), sink) }.then_some(data)
    }

    /// Replace them, at most [`MAX_STATE`] bytes. False when it isn't a mod mob.
    pub fn set_state(self, entity: Entity<'_>, state: &[u8]) -> bool {
        // SAFETY: the entity and bytes are borrowed for this call.
        unsafe { (self.0.set_state)(entity.raw(), abi::Str::new(state)) }
    }
}
