//! AI for mod mobs: goals every mob of a type runs, and the walking, looking and targeting they
//! use.

use std::sync::OnceLock;

use crate::{
    InterfacePrefix, InterfaceUnavailable, RegistrationError, Registry, abi, content::World,
    entities::Entity, guard, s,
};

pub use abi::{goal_control, goal_phase};

/// Answers a goal phase (`goal_phase`) for one mob; the answer counts for `CAN_START` and
/// `CONTINUE`. `key` is the goal's.
pub type GoalHandler = fn(phase: u32, key: u64, world: World<'_>, mob: Entity<'_>) -> bool;

/// Hears a hit a mob is about to take: the damage type id and the amount before armour. False
/// cancels it.
pub type HurtHandler =
    fn(world: World<'_>, mob: Entity<'_>, damage_type: &str, amount: f32) -> bool;

/// Calls appended to `moddedmc:mob_ai` v1 after its first release.
#[derive(Clone, Copy)]
pub enum MobAiExtension {
    /// `on_hurt`, `last_hurt_by` and `attack`.
    Combat,
}

#[derive(Clone, Copy)]
pub struct MobAi(InterfacePrefix<abi::MobAiV1>);

/// A goal every mob of a mod mob type gets.
pub struct Goal<'a> {
    pub kind: &'a str,
    pub key: u64,
    /// Lower runs first.
    pub priority: u32,
    /// `goal_control` bits.
    pub controls: u32,
}

static HANDLER: OnceLock<GoalHandler> = OnceLock::new();
static HURT: OnceLock<HurtHandler> = OnceLock::new();

pub fn require() -> Result<MobAi, InterfaceUnavailable> {
    // SAFETY: this is the published table type for moddedmc:mob_ai v1; appended fields are read
    // only after require_extension checks the size.
    unsafe { InterfacePrefix::require(abi::MOB_AI, 1, abi::MOB_AI_V1_BASE_SIZE) }.map(MobAi)
}

unsafe extern "C" fn hurt(
    world: abi::WorldRef,
    mob: abi::EntityRef,
    damage_type: abi::Str,
    amount: f32,
) -> bool {
    guard("mob hurt", true, || {
        let (Some(handler), Some(world), Some(mob)) =
            (HURT.get(), World::borrowed(world), Entity::borrowed(mob))
        else {
            return true;
        };
        // SAFETY: the host borrows the id for this call.
        handler(world, mob, unsafe { damage_type.str() }, amount)
    })
}

unsafe extern "C" fn goal(phase: u32, key: u64, world: abi::WorldRef, mob: abi::EntityRef) -> bool {
    guard("mob goal", false, || {
        let (Some(handler), Some(world), Some(mob)) =
            (HANDLER.get(), World::borrowed(world), Entity::borrowed(mob))
        else {
            return false;
        };
        handler(phase, key, world, mob)
    })
}

impl MobAi {
    pub fn require_extension(self, extension: MobAiExtension) -> Result<(), InterfaceUnavailable> {
        let (end, name) = match extension {
            MobAiExtension::Combat => (
                std::mem::offset_of!(abi::MobAiV1, attack),
                "moddedmc:mob_ai.combat",
            ),
        };
        if self.0.size >= end + size_of::<usize>() {
            Ok(())
        } else {
            Err(InterfaceUnavailable { name, version: 1 })
        }
    }

    /// Hear every hit mobs of `kind` are about to take. A mod has one handler: the first one
    /// passed serves every kind. Needs `MobAiExtension::Combat`.
    pub fn on_hurt(
        self,
        reg: &mut Registry<'_>,
        kind: &str,
        handler: HurtHandler,
    ) -> Result<(), RegistrationError> {
        let Some(api) = self.0.full() else {
            return Err(RegistrationError::InvalidDefinition);
        };
        let _ = HURT.set(handler);
        // SAFETY: registration is live for the call; the size was checked by require_extension.
        RegistrationError::from_code(unsafe { (api.on_hurt)(reg.reg, s(kind), hurt) })
    }

    /// Who last hurt `mob` and its tick count then (vanilla `getLastHurtByMob` and
    /// `getLastHurtByMobTimestamp`). Needs `MobAiExtension::Combat`.
    pub fn last_hurt_by(self, mob: Entity<'_>) -> Option<(i32, i32)> {
        let api = self.0.full()?;
        let mut timestamp = 0;
        // SAFETY: the mob is live for this callback and the timestamp outlives the call.
        let id = unsafe { (api.last_hurt_by)(mob.raw(), &raw mut timestamp) };
        (id >= 0).then_some((id, timestamp))
    }

    /// `mob` hits the loaded entity `target` for `amount` as a mob attack. Needs
    /// `MobAiExtension::Combat`.
    pub fn attack(self, world: World<'_>, mob: Entity<'_>, target: i32, amount: f32) -> bool {
        let Some(api) = self.0.full() else {
            return false;
        };
        // SAFETY: the world and mob are live for this callback.
        unsafe { (api.attack)(world.raw(), mob.raw(), target, amount) }
    }

    /// Set the mod's goal handler, once, before adding goals. Needs `moddedmc:entities`.
    pub fn on_goal(self, handler: GoalHandler) -> Result<(), RegistrationError> {
        HANDLER
            .set(handler)
            .map_err(|_| RegistrationError::DuplicateId)
    }

    pub fn add_goal(
        self,
        reg: &mut Registry<'_>,
        goal: &Goal<'_>,
    ) -> Result<(), RegistrationError> {
        if HANDLER.get().is_none() {
            return Err(RegistrationError::InvalidDefinition);
        }
        let def = abi::GoalDefinition {
            kind: s(goal.kind),
            key: goal.key,
            priority: goal.priority,
            controls: goal.controls,
            callback: self::goal,
        };
        // SAFETY: registration and the borrowed definition are live for the call.
        RegistrationError::from_code(unsafe {
            ((&raw const (*self.0.ptr).add_goal).read())(reg.reg, &def)
        })
    }

    /// Mobs of `kind` drop their carrier's own goals and targets.
    pub fn replace_goals(
        self,
        reg: &mut Registry<'_>,
        kind: &str,
    ) -> Result<(), RegistrationError> {
        // SAFETY: registration is live for the call.
        RegistrationError::from_code(unsafe {
            ((&raw const (*self.0.ptr).replace_goals).read())(reg.reg, s(kind))
        })
    }

    /// Path to `target` at `speed` times the movement speed.
    pub fn walk_to(self, mob: Entity<'_>, target: [f64; 3], speed: f64) -> bool {
        // SAFETY: the mob is live for this callback.
        unsafe { ((&raw const (*self.0.ptr).walk_to).read())(mob.raw(), abi::Vec3(target), speed) }
    }

    pub fn stop(self, mob: Entity<'_>) {
        // SAFETY: the mob is live for this callback.
        unsafe { ((&raw const (*self.0.ptr).stop).read())(mob.raw()) }
    }

    pub fn navigating(self, mob: Entity<'_>) -> bool {
        // SAFETY: the mob is live for this callback.
        unsafe { ((&raw const (*self.0.ptr).navigating).read())(mob.raw()) }
    }

    pub fn look_at(self, mob: Entity<'_>, position: [f64; 3]) -> bool {
        // SAFETY: the mob is live for this callback.
        unsafe { ((&raw const (*self.0.ptr).look_at).read())(mob.raw(), abi::Vec3(position)) }
    }

    /// Target the loaded entity `target`, or clear with `None`.
    pub fn set_target(self, world: World<'_>, mob: Entity<'_>, target: Option<i32>) -> bool {
        // SAFETY: the world and mob are live for this callback.
        unsafe {
            ((&raw const (*self.0.ptr).set_target).read())(
                world.raw(),
                mob.raw(),
                target.unwrap_or(-1),
            )
        }
    }

    pub fn target(self, mob: Entity<'_>) -> Option<i32> {
        // SAFETY: the mob is live for this callback.
        let id = unsafe { ((&raw const (*self.0.ptr).target).read())(mob.raw()) };
        (id >= 0).then_some(id)
    }
}
