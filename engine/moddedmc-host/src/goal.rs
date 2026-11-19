//! AI goals mods add to mobs (`entities/1` `add_goal`): a vanilla goal whose decisions and ticks
//! go to the mods' `goal_event`.

use std::cell::Cell;

use moddedmc_api::abi::{goal_control, goal_event};
use pumpkin_core::entity::{
    ai::goal::{Controls, Goal},
    mob::{Mob, MobEntity},
};

use crate::{HOST, lock, native::Native};

thread_local! {
    /// Set while mods hear about a goal, so a goal cannot add goals to the selector running it.
    static IN_GOAL: Cell<bool> = const { Cell::new(false) };
}

/// Adds goal `key` to the mob; false if called from a goal event.
pub fn add(mob: &MobEntity, priority: u32, controls: u32, key: String) -> bool {
    if IN_GOAL.get() {
        return false;
    }
    let mut taken = Controls::empty();
    for (bit, control) in [
        (goal_control::MOVE, Controls::MOVE),
        (goal_control::LOOK, Controls::LOOK),
        (goal_control::JUMP, Controls::JUMP),
        (goal_control::TARGET, Controls::TARGET),
    ] {
        if controls & bit != 0 {
            taken = taken | control;
        }
    }
    let priority = u8::try_from(priority).unwrap_or(u8::MAX);
    mob.goals_selector.add_goal(
        priority,
        Box::new(ModGoal {
            key,
            controls: taken,
        }),
    );
    true
}

struct ModGoal {
    key: String,
    controls: Controls,
}

impl ModGoal {
    /// Asks every mod; true if any says yes.
    fn ask(&self, phase: u32, mob: &dyn Mob) -> bool {
        let entity = &mob.get_mob_entity().living_entity.entity;
        let world = entity.world.load();
        let dimension = world.dimension.minecraft_name;
        let id = entity.entity_uuid;
        let mods: Vec<Native> = lock(&HOST.mods).iter().map(|m| m.module).collect();
        IN_GOAL.set(true);
        let mut yes = false;
        for module in mods {
            yes |= module.goal_event(phase, &self.key, dimension, id);
        }
        IN_GOAL.set(false);
        yes
    }
}

impl Goal for ModGoal {
    fn can_start(&mut self, mob: &dyn Mob) -> bool {
        self.ask(goal_event::CAN_START, mob)
    }

    fn should_continue(&mut self, mob: &dyn Mob) -> bool {
        self.ask(goal_event::CONTINUE, mob)
    }

    fn start(&mut self, mob: &dyn Mob) {
        self.ask(goal_event::START, mob);
    }

    fn stop(&mut self, mob: &dyn Mob) {
        self.ask(goal_event::STOP, mob);
    }

    fn tick(&mut self, mob: &dyn Mob) {
        self.ask(goal_event::TICK, mob);
    }

    fn should_run_every_tick(&self) -> bool {
        true
    }

    fn controls(&self) -> Controls {
        self.controls
    }
}
