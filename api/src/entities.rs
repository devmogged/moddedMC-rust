//! `moddedmc:entities/1`: entities a mod drives (see [`crate::abi::EntitiesV1`]).

use crate::{
    Player,
    abi::{self, EntitiesV1, EntityId, Vec3},
    interface, s,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EventKind<'a> {
    /// A player right-clicked it.
    Use,
    /// A player attacked it.
    Attack,
    /// It is about to take `amount` damage from `cause`, a damage type's message id
    /// (`genericKill` for `/kill`, `cactus`, `player`, ...). Return true to cancel it.
    Damage { amount: f32, cause: &'a str },
    /// It died.
    Death,
}

/// Something happened to an entity. Every mod sees every one: check `id` is yours. `player` is
/// set for `Use` and `Attack`.
pub struct Event<'a> {
    pub kind: EventKind<'a>,
    pub dimension: &'a str,
    pub id: EntityId,
    pub player: Option<Player<'a>>,
}

/// The entities interface, if the server offers it.
#[must_use]
pub fn get() -> Option<Entities> {
    interface::<EntitiesV1>(abi::ENTITIES_V1).map(Entities)
}

/// Spawn, move, name and remove entities. An entity's id survives restarts: save it.
#[derive(Clone, Copy)]
pub struct Entities(&'static EntitiesV1);

impl Entities {
    /// Spawns a vanilla entity type, e.g. `minecraft:armor_stand`.
    #[must_use]
    pub fn spawn(&self, dimension: &str, kind: &str, at: Vec3) -> Option<EntityId> {
        let mut id = EntityId::default();
        // SAFETY: borrowed for the call; `id` is writable.
        unsafe { (self.0.spawn)(s(dimension), s(kind), at, &raw mut id) }.then_some(id)
    }

    /// Where it is; `None` if it is gone or not loaded.
    #[must_use]
    pub fn position(&self, dimension: &str, id: EntityId) -> Option<Vec3> {
        let mut at = Vec3::default();
        // SAFETY: borrowed for the call; `at` is writable.
        unsafe { (self.0.position)(s(dimension), id, &raw mut at) }.then_some(at)
    }

    /// Moves it; `None` keeps its facing.
    pub fn move_to(
        &self,
        dimension: &str,
        id: EntityId,
        at: Vec3,
        facing: Option<(f32, f32)>,
    ) -> bool {
        let (yaw, pitch) = facing.unwrap_or((f32::NAN, f32::NAN));
        // SAFETY: borrowed for the call.
        unsafe { (self.0.move_to)(s(dimension), id, at, yaw, pitch) }
    }

    /// Shows `name` above it; empty hides it.
    pub fn set_name(&self, dimension: &str, id: EntityId, name: &str) -> bool {
        // SAFETY: borrowed for the call.
        unsafe { (self.0.set_name)(s(dimension), id, s(name)) }
    }

    pub fn remove(&self, dimension: &str, id: EntityId) -> bool {
        // SAFETY: borrowed for the call.
        unsafe { (self.0.remove)(s(dimension), id) }
    }

    /// Turns a mob's vanilla AI on or off; off, the mod drives it with [`Self::move_to`]. False
    /// if it is not a mob.
    pub fn set_ai(&self, dimension: &str, id: EntityId, enabled: bool) -> bool {
        // SAFETY: borrowed for the call.
        unsafe { (self.0.set_ai)(s(dimension), id, enabled) }
    }

    /// Sends a mob walking to `to` along a path (`speed` 1.0 is its normal pace) and keeps it
    /// there (within about a block and a half), its own wandering held off, until [`Self::release`]. Needs its AI on. False if it
    /// is not a mob.
    pub fn walk_to(&self, dimension: &str, id: EntityId, to: Vec3, speed: f64) -> bool {
        // SAFETY: borrowed for the call.
        unsafe { (self.0.walk_to)(s(dimension), id, to, speed) }
    }

    /// Stops a walk and gives the mob its own goals back.
    pub fn release(&self, dimension: &str, id: EntityId) -> bool {
        let nan = Vec3 {
            x: f64::NAN,
            ..Vec3::default()
        };
        // SAFETY: borrowed for the call.
        unsafe { (self.0.walk_to)(s(dimension), id, nan, 0.0) }
    }
}
