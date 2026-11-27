//! Hunger-driven prey tracking and fresh-drop consumption.
use crate::{ENTITIES, MOB_AI, MOBS, STACKS, TAGS, flight, hunger, taming};
use fastnbt::Value;
use moddedmc_api::{
    content::World,
    entities::{ENTITY_ALIVE, Entity},
    mob_ai::goal_phase,
    tick_count,
};
use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
};

pub const GOAL: u64 = 15;
#[derive(Default)]
struct Hunt {
    prey: Option<i32>,
    site: Option<([f64; 3], u64)>,
    item: Option<i32>,
    next_eat: u64,
    next_attack: u64,
    seen: u64,
}
static HUNTS: Mutex<Option<HashMap<i32, Hunt>>> = Mutex::new(None);

pub fn supported(name: &str) -> bool {
    matches!(
        name,
        "cindervane" | "raevyx" | "ignivorus" | "varasuchus" | "volitans"
    )
}

fn prey_type(name: &str, kind: &str) -> bool {
    let kind = kind.trim_start_matches("minecraft:");
    match name {
        "cindervane" => matches!(kind, "chicken" | "cod" | "salmon"),
        "raevyx" => matches!(kind, "pig" | "sheep"),
        "ignivorus" => matches!(kind, "cod" | "cow" | "pig" | "salmon" | "sheep"),
        "varasuchus" => matches!(kind, "cod" | "cow" | "salmon" | "tropical_fish"),
        "volitans" => matches!(kind, "cod" | "pufferfish" | "salmon" | "tropical_fish"),
        _ => false,
    }
}

fn eligible(mob: Entity<'_>) -> bool {
    mob.flags() & ENTITY_ALIVE != 0
        && !crate::eggs::is_juvenile(mob)
        && mob.vehicle().is_none()
        && MOBS.get().is_some_and(|m| m.passengers(mob).is_empty())
        && taming::order_name(mob) != "stay"
        && crate::roost::sleep_clip(mob.id()).is_none()
        && hunger::level(mob) < 100
}

pub fn forget(id: i32) {
    if let Some(all) = HUNTS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_mut()
    {
        all.remove(&id);
    }
}

pub fn active(id: i32) -> bool {
    HUNTS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .and_then(|all| all.get(&id))
        .is_some_and(|hunt| hunt.prey.is_some() || hunt.site.is_some())
}

pub fn sweep(now: u64) {
    if now.is_multiple_of(200)
        && let Some(all) = HUNTS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_mut()
    {
        all.retain(|_, hunt| now.saturating_sub(hunt.seen) < 400);
    }
}

/// Track dead prey independently of the movement goal: native combat may own movement.
pub fn tick(world: World<'_>, mob: Entity<'_>, name: &str) {
    if !supported(name) {
        return;
    }
    let (Some(ai), Some(mobs), Some(entities), Some(view)) =
        (MOB_AI.get(), MOBS.get(), ENTITIES.get(), mob.view())
    else {
        return;
    };
    let now = tick_count();
    let mut lock = HUNTS.lock().unwrap_or_else(PoisonError::into_inner);
    let all = lock.get_or_insert_default();
    if !eligible(mob) {
        if let Some(hunt) = all.remove(&mob.id())
            && hunt.prey.is_some()
            && hunt.prey == ai.target(mob)
        {
            ai.set_target(world, mob, None);
        }
        return;
    }
    let hunt = all.entry(mob.id()).or_default();
    hunt.seen = now;
    if let Some(prey) = hunt.prey {
        match entities.with_entity(world, prey, |target| {
            let alive = target.flags() & ENTITY_ALIVE != 0;
            let credited = !alive
                && (mobs.kill_credit(target) == Some(mob.id())
                    || ai
                        .last_hurt_by(target)
                        .is_some_and(|(id, _)| id == mob.id()));
            (alive, credited, target.view().map(|v| v.position))
        }) {
            Some((false, true, Some(pos))) => {
                hunt.site = Some((pos, now + 60));
                hunt.prey = None;
                if ai.target(mob) == Some(prey) {
                    ai.set_target(world, mob, None);
                }
            }
            Some((true, _, _)) if ai.target(mob) == Some(prey) => return,
            _ => hunt.prey = None,
        }
    }
    if hunt.site.is_some()
        && let Some(target) = ai.target(mob)
        && entities.with_entity(world, target, |e| prey_type(name, e.type_id())) == Some(true)
    {
        ai.set_target(world, mob, None);
    }
    if hunt.site.is_some_and(|(_, until)| now > until) || ai.target(mob).is_some() {
        hunt.site = None;
        hunt.item = None;
    }
    if hunt.site.is_none()
        && let Some(target) = ai.target(mob)
        && entities.with_entity(world, target, |e| {
            e.flags() & ENTITY_ALIVE != 0 && prey_type(name, e.type_id())
        }) == Some(true)
    {
        hunt.prey = Some(target);
    }
    if hunt.site.is_some()
        || ai.target(mob).is_some()
        || hunger::level(mob) > 60
        || !(now + mob.id() as u64).is_multiple_of(80)
    {
        return;
    }
    let pos = view.position;
    let nearest = entities
        .entities_in(
            world,
            pos.map(|v| v - 16.0),
            pos.map(|v| v + 16.0),
            mob.id(),
        )
        .into_iter()
        .filter_map(|id| {
            entities
                .with_entity(world, id, |target| {
                    if target.flags() & ENTITY_ALIVE == 0
                        || mobs.kind(target).is_some()
                        || !prey_type(name, target.type_id())
                    {
                        return None;
                    }
                    let distance =
                        flight::length_squared(flight::offset(target.view()?.position, pos));
                    (distance <= 256.0).then_some((distance, id))
                })
                .flatten()
        })
        .min_by(|a, b| a.0.total_cmp(&b.0));
    if let Some((_, id)) = nearest
        && ai.set_target(world, mob, Some(id))
    {
        hunt.prey = Some(id);
    }
}

fn food(item: Entity<'_>, name: &str, fresh: bool) -> Option<(HashMap<String, Value>, [f64; 3])> {
    if fresh && !item.age().is_some_and(|age| age <= 100) {
        return None;
    }
    let bytes = STACKS.get()?.entity_stack(item)?;
    let stack: HashMap<String, Value> = fastnbt::from_bytes(&bytes).ok()?;
    if !matches!(stack.get("count"), Some(Value::Int(count)) if *count > 0) {
        return None;
    }
    let Value::String(id) = stack.get("id")? else {
        return None;
    };
    if !TAGS
        .get()?
        .contains("item", &format!("saintsdragons:foods/{name}"), id)
    {
        return None;
    }
    Some((stack, item.view()?.position))
}

pub fn goal(phase: u32, world: World<'_>, mob: Entity<'_>) -> bool {
    let (Some(ai), Some(mobs), Some(entities), Some(stacks), Some(view)) = (
        MOB_AI.get(),
        MOBS.get(),
        ENTITIES.get(),
        STACKS.get(),
        mob.view(),
    ) else {
        return false;
    };
    let Some(name) = mobs
        .kind(mob)
        .and_then(|k| k.strip_prefix("saintsdragons:"))
    else {
        return false;
    };
    if phase == goal_phase::STOP {
        ai.stop(mob);
        return false;
    }
    if !eligible(mob) {
        return false;
    }
    let now = tick_count();
    let mut lock = HUNTS.lock().unwrap_or_else(PoisonError::into_inner);
    let Some(hunt) = lock.as_mut().and_then(|all| all.get_mut(&mob.id())) else {
        return false;
    };
    let moving = hunt.site.is_some() || (name != "cindervane" && hunt.prey.is_some());
    if matches!(phase, goal_phase::CAN_START | goal_phase::CONTINUE) {
        return moving;
    }
    if phase != goal_phase::TICK {
        return false;
    }
    if let Some(prey) = hunt.prey
        && name != "cindervane"
    {
        if let Some(Some(pos)) =
            entities.with_entity(world, prey, |target| target.view().map(|v| v.position))
        {
            ai.look_at(mob, pos);
            if flight::length_squared(flight::offset(pos, view.position)) <= 9.0 {
                ai.stop(mob);
                if now >= hunt.next_attack {
                    let damage = crate::roster::melee_base(name, false).unwrap_or(4.0) as f32
                        * hunger::melee_multiplier(mob);
                    ai.attack(world, mob, prey, damage);
                    hunt.next_attack = now + 20;
                }
            } else {
                ai.walk_to(mob, pos, 1.0);
            }
        }
        return false;
    }
    let Some((site, _)) = hunt.site else {
        return false;
    };
    if hunt.item.is_some_and(|id| {
        entities.with_entity(world, id, |e| food(e, name, false).is_some()) != Some(true)
    }) {
        hunt.item = None;
    }
    if hunt.item.is_none() && now.is_multiple_of(5) {
        let min = [site[0] - 8.0, site[1] - 5.0, site[2] - 8.0];
        let max = [site[0] + 8.0, site[1] + 5.0, site[2] + 8.0];
        hunt.item = entities
            .entities_in(world, min, max, mob.id())
            .into_iter()
            .filter_map(|id| {
                entities
                    .with_entity(world, id, |e| {
                        food(e, name, true).map(|(_, pos)| {
                            (
                                flight::length_squared(flight::offset(pos, view.position)),
                                id,
                            )
                        })
                    })
                    .flatten()
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, id)| id);
    }
    if let Some(id) = hunt.item {
        entities.with_entity(world, id, |item| {
            let Some((mut stack, pos)) = food(item, name, false) else {
                return;
            };
            ai.look_at(mob, pos);
            let reach = mob
                .body()
                .map_or(1.5, |b| ((b.max[0] - b.min[0]) * 0.75 + 0.75).max(1.5));
            if flight::length_squared(flight::offset(pos, view.position)) > reach * reach {
                let wet = |p| {
                    matches!(
                        world.fluid_id(flight::cell(p)).as_deref(),
                        Some("minecraft:water" | "minecraft:flowing_water")
                    )
                };
                // shortcut: the generic navigation goal has no dedicated airborne landing intent.
                ai.walk_to(
                    mob,
                    pos,
                    if wet(view.position) || wet(pos) {
                        0.25
                    } else {
                        1.0
                    },
                );
                return;
            }
            ai.stop(mob);
            if now < hunt.next_eat
                || (name == "cindervane" && crate::combat::active_attack(mob).is_some())
            {
                return;
            }
            let Some(Value::Int(count)) = stack.get_mut("count") else {
                return;
            };
            if *count <= 0 {
                return;
            }
            *count -= 1;
            let remaining = if *count == 0 {
                Vec::new()
            } else {
                let Ok(bytes) = fastnbt::to_bytes(&stack) else {
                    return;
                };
                bytes
            };
            if stacks.set_entity_stack(item, &remaining) {
                hunger::feed(mob, false);
                crate::audio::eat(world, mob);
                if name == "varasuchus" {
                    crate::baby::start_eating(mob.id(), 20);
                }
                hunt.next_eat = now + 20;
                if remaining.is_empty() {
                    hunt.item = None;
                }
            }
        });
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn released_prey_lists_are_species_specific() {
        assert!(prey_type("cindervane", "minecraft:chicken"));
        assert!(!prey_type("cindervane", "pig"));
        assert!(prey_type("raevyx", "pig"));
        assert!(!prey_type("raevyx", "cod"));
        assert!(prey_type("volitans", "pufferfish"));
        assert!(!prey_type("varasuchus", "pufferfish"));
        assert!(!supported("stegonaut"));
    }
}
