//! AI for mod mobs: goals every mob of a type runs, and the walking, looking and targeting they
//! use.

use std::sync::OnceLock;

use crate::{
    InterfaceUnavailable, RegistrationError, Registry, abi, content::World, entities::Entity,
    guard, require_interface, s,
};

pub use abi::{goal_control, goal_phase};

/// Answers a goal phase (`goal_phase`) for one mob; the answer counts for `CAN_START` and
/// `CONTINUE`. `key` is the goal's.
pub type GoalHandler = fn(phase: u32, key: u64, world: World<'_>, mob: Entity<'_>) -> bool;

#[derive(Clone, Copy)]
pub struct MobAi(&'static abi::MobAiV1);

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

pub fn require() -> Result<MobAi, InterfaceUnavailable> {
    // SAFETY: this is the published table type for moddedmc:mob_ai v1.
    unsafe { require_interface(abi::MOB_AI, 1) }.map(MobAi)
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
        RegistrationError::from_code(unsafe { (self.0.add_goal)(reg.reg, &def) })
    }

    /// Mobs of `kind` drop their carrier's own goals and targets.
    pub fn replace_goals(
        self,
        reg: &mut Registry<'_>,
        kind: &str,
    ) -> Result<(), RegistrationError> {
        // SAFETY: registration is live for the call.
        RegistrationError::from_code(unsafe { (self.0.replace_goals)(reg.reg, s(kind)) })
    }

    /// Path to `target` at `speed` times the movement speed.
    pub fn walk_to(self, mob: Entity<'_>, target: [f64; 3], speed: f64) -> bool {
        // SAFETY: the mob is live for this callback.
        unsafe { (self.0.walk_to)(mob.raw(), abi::Vec3(target), speed) }
    }

    pub fn stop(self, mob: Entity<'_>) {
        // SAFETY: the mob is live for this callback.
        unsafe { (self.0.stop)(mob.raw()) }
    }

    pub fn navigating(self, mob: Entity<'_>) -> bool {
        // SAFETY: the mob is live for this callback.
        unsafe { (self.0.navigating)(mob.raw()) }
    }

    pub fn look_at(self, mob: Entity<'_>, position: [f64; 3]) -> bool {
        // SAFETY: the mob is live for this callback.
        unsafe { (self.0.look_at)(mob.raw(), abi::Vec3(position)) }
    }

    /// Target the loaded entity `target`, or clear with `None`.
    pub fn set_target(self, world: World<'_>, mob: Entity<'_>, target: Option<i32>) -> bool {
        // SAFETY: the world and mob are live for this callback.
        unsafe { (self.0.set_target)(world.raw(), mob.raw(), target.unwrap_or(-1)) }
    }

    pub fn target(self, mob: Entity<'_>) -> Option<i32> {
        // SAFETY: the mob is live for this callback.
        let id = unsafe { (self.0.target)(mob.raw()) };
        (id >= 0).then_some(id)
    }
}
