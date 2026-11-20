//! `moddedmc:mob_ai`: goals for mod mob types, attached to every mob of the type when it joins a
//! world (spawned or loaded), and the navigation, look and target calls they use.

use std::{
    collections::BTreeMap,
    ptr,
    sync::{Arc, Mutex, PoisonError},
};

use moddedmc_api::abi::{self, EntityRef, Reg, Str, Vec3, WorldRef, code, goal_control};
use pumpkin_core::{
    entity::{
        EntityBase,
        ai::{
            goal::{Controls, Goal},
            pathfinder::NavigatorGoal,
        },
        mob::Mob,
    },
    world::{World, install_entity_added_hook},
};
use pumpkin_util::math::vector3::Vector3;

use crate::{
    content::{entity, world},
    mobs, native,
};

pub static MOB_AI: abi::MobAiV1 = abi::MobAiV1 {
    size: size_of::<abi::MobAiV1>(),
    add_goal,
    replace_goals,
    walk_to,
    stop,
    navigating,
    look_at,
    set_target,
    target,
};

#[derive(Clone, Copy)]
pub struct StagedGoal {
    key: u64,
    priority: u8,
    controls: u32,
    callback: abi::GoalCallback,
}

/// The AI a mod stages for one mob type.
#[derive(Default)]
pub struct TypeAi {
    replace: bool,
    goals: Vec<StagedGoal>,
}

static TYPES: Mutex<BTreeMap<String, TypeAi>> = Mutex::new(BTreeMap::new());

fn types() -> std::sync::MutexGuard<'static, BTreeMap<String, TypeAi>> {
    TYPES.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The staged AI for `kind`, when it names a mod mob type registered or staged by now.
///
/// # Safety
/// `reg` is a live registration and `kind` is borrowed for the call.
unsafe fn staged<'a>(reg: Reg, kind: Str) -> Option<&'a mut TypeAi> {
    // SAFETY: the caller passes a live registration and a borrowed kind.
    let (staging, kind) = unsafe { (native::staging(reg), kind.str()) };
    if !mobs::known(&staging.mobs, kind) {
        return None;
    }
    Some(staging.mob_ai.entry(kind.to_owned()).or_default())
}

unsafe extern "C" fn add_goal(reg: Reg, def: *const abi::GoalDefinition) -> i32 {
    // SAFETY: the definition is borrowed for the call.
    let Some(def) = (unsafe { def.as_ref() }) else {
        return code::INVALID_DEFINITION;
    };
    // SAFETY: registration is live for the call.
    let Some(ai) = (unsafe { staged(reg, def.kind) }) else {
        return code::INVALID_DEFINITION;
    };
    ai.goals.push(StagedGoal {
        key: def.key,
        priority: u8::try_from(def.priority).unwrap_or(u8::MAX),
        controls: def.controls,
        callback: def.callback,
    });
    code::OK
}

unsafe extern "C" fn replace_goals(reg: Reg, kind: Str) -> i32 {
    // SAFETY: registration is live for the call.
    let Some(ai) = (unsafe { staged(reg, kind) }) else {
        return code::INVALID_DEFINITION;
    };
    ai.replace = true;
    code::OK
}

/// Commit a mod's staged AI after its registration succeeded.
pub fn register(staged: BTreeMap<String, TypeAi>) {
    let mut types = types();
    for (kind, ai) in staged {
        let entry = types.entry(kind).or_default();
        entry.replace |= ai.replace;
        entry.goals.extend(ai.goals);
    }
}

pub fn install() -> bool {
    install_entity_added_hook(attach)
}

/// Give a mod mob its type's goals as it joins a world.
fn attach(_world: &World, entity: &Arc<dyn EntityBase>) {
    let Some(mob) = entity.get_mob() else { return };
    let Some(kind) = mobs::registered_kind(entity.as_ref()) else {
        return;
    };
    let (replace, goals) = match types().get(kind) {
        Some(ai) => (ai.replace, ai.goals.clone()),
        None => return,
    };
    let me = mob.get_mob_entity();
    if replace {
        me.goals_selector.clear(mob);
        me.target_selector.clear(mob);
    }
    for goal in goals {
        let selector = if goal.controls & goal_control::TARGET == 0 {
            &me.goals_selector
        } else {
            &me.target_selector
        };
        selector.add_goal(goal.priority, Box::new(ModGoal(goal)));
    }
}

struct ModGoal(StagedGoal);

impl ModGoal {
    fn ask(&self, phase: u32, mob: &dyn Mob) -> bool {
        let world = mob.get_entity().world.load_full();
        let base: &dyn EntityBase = mob;
        // SAFETY: the world and the mob outlive this synchronous callback.
        unsafe {
            (self.0.callback)(
                phase,
                self.0.key,
                ptr::from_ref(world.as_ref()).cast(),
                ptr::from_ref(&base).cast(),
            )
        }
    }
}

impl Goal for ModGoal {
    fn can_start(&mut self, mob: &dyn Mob) -> bool {
        self.ask(abi::goal_phase::CAN_START, mob)
    }

    fn should_continue(&mut self, mob: &dyn Mob) -> bool {
        self.ask(abi::goal_phase::CONTINUE, mob)
    }

    fn start(&mut self, mob: &dyn Mob) {
        self.ask(abi::goal_phase::START, mob);
    }

    fn stop(&mut self, mob: &dyn Mob) {
        self.ask(abi::goal_phase::STOP, mob);
    }

    fn tick(&mut self, mob: &dyn Mob) {
        self.ask(abi::goal_phase::TICK, mob);
    }

    fn should_run_every_tick(&self) -> bool {
        true
    }

    fn controls(&self) -> Controls {
        [
            (goal_control::MOVE, Controls::MOVE),
            (goal_control::LOOK, Controls::LOOK),
            (goal_control::JUMP, Controls::JUMP),
            (goal_control::TARGET, Controls::TARGET),
        ]
        .into_iter()
        .filter(|(bit, _)| self.0.controls & bit != 0)
        .fold(Controls::empty(), |all, (_, control)| all | control)
    }
}

/// # Safety
/// `raw` is an entity a callback passed, still live.
unsafe fn mob<'a>(raw: EntityRef) -> Option<&'a dyn Mob> {
    // SAFETY: the caller passes a live entity.
    unsafe { entity(raw) }?.get_mob()
}

fn finite(Vec3([x, y, z]): Vec3) -> Option<Vector3<f64>> {
    (x.is_finite() && y.is_finite() && z.is_finite()).then(|| Vector3::new(x, y, z))
}

unsafe extern "C" fn walk_to(raw: EntityRef, target: Vec3, speed: f64) -> bool {
    // SAFETY: the mob is live for the callback.
    let (Some(mob), Some(target)) = (unsafe { mob(raw) }, finite(target)) else {
        return false;
    };
    if !speed.is_finite() || speed <= 0.0 {
        return false;
    }
    let position = mob.get_entity().pos.load();
    mob.get_mob_entity()
        .navigator
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .set_progress(NavigatorGoal::new(position, target, speed));
    true
}

unsafe extern "C" fn stop(raw: EntityRef) {
    // SAFETY: the mob is live for the callback.
    if let Some(mob) = unsafe { mob(raw) } {
        mob.get_mob_entity()
            .navigator
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .stop();
    }
}

unsafe extern "C" fn navigating(raw: EntityRef) -> bool {
    // SAFETY: the mob is live for the callback.
    unsafe { mob(raw) }.is_some_and(|mob| {
        !mob.get_mob_entity()
            .navigator
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_idle()
    })
}

unsafe extern "C" fn look_at(raw: EntityRef, position: Vec3) -> bool {
    // SAFETY: the mob is live for the callback.
    let (Some(mob), Some(at)) = (unsafe { mob(raw) }, finite(position)) else {
        return false;
    };
    mob.get_mob_entity()
        .look_control
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .look_at(mob, at.x, at.y, at.z);
    true
}

unsafe extern "C" fn set_target(raw_world: WorldRef, raw: EntityRef, target: i32) -> bool {
    // SAFETY: the world and the mob are live for the callback.
    let (Some(world), Some(mob)) = (unsafe { world(raw_world) }, unsafe { mob(raw) }) else {
        return false;
    };
    let target = if target < 0 {
        None
    } else {
        match world.get_entity_by_id(target) {
            Some(found) if found.get_entity().entity_id != mob.get_entity().entity_id => {
                Some(found)
            }
            _ => return false,
        }
    };
    mob.apply_mob_target(target);
    true
}

unsafe extern "C" fn target(raw: EntityRef) -> i32 {
    // SAFETY: the mob is live for the callback.
    unsafe { mob(raw) }
        .and_then(|mob| mob.get_mob_entity().get_target())
        .map_or(-1, |t| t.get_entity().entity_id)
}
