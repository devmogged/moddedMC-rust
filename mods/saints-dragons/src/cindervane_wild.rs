//! Wild social behavior from CindervaneBrain (0.9.88).
use std::{
    collections::HashMap,
    sync::{LazyLock, Mutex, PoisonError},
};

use fastnbt::IntArray;
use moddedmc_api::{
    content::World,
    entities::{ENTITY_ALIVE, Entity},
    mob_ai::goal_phase,
    tick_count,
};

use crate::{
    CINDERVANE, ENTITIES, MOB_AI, MOBS, appearance::Appearance, eggs, flight, pack_flight, taming,
};

pub const SOCIAL: u64 = 40;
pub const DRINK: u64 = 41;
const PARENT: &str = "saintsdragons:parent";
const PACK: &str = "saintsdragons:pack";
const PACK_RADIUS: f64 = 48.0;
const PACK_CAPACITY: usize = 4;

#[derive(Clone, Copy, Default)]
struct Follow {
    next_search: u64,
    target: Option<i32>,
    protecting: bool,
}

static FOLLOW: LazyLock<Mutex<HashMap<i32, Follow>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn forget(id: i32) {
    FOLLOW
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .remove(&id);
    DRINKS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .remove(&id);
}

fn link(mob: Entity<'_>, key: &str) -> Option<[u8; 16]> {
    let data = fastnbt::from_bytes::<HashMap<String, IntArray>>(&mob.data(key)?).ok()?;
    let uuid = data.get("uuid")?;
    if uuid.len() != 4 {
        return None;
    }
    let mut bytes = [0; 16];
    for (dst, value) in bytes.as_chunks_mut::<4>().0.iter_mut().zip(uuid.iter()) {
        dst.copy_from_slice(&value.to_be_bytes());
    }
    Some(bytes)
}

fn save_link(mob: Entity<'_>, key: &str, uuid: Option<[u8; 16]>) {
    if link(mob, key) == uuid {
        return;
    }
    let mut data = HashMap::new();
    if let Some(uuid) = uuid {
        data.insert(
            "uuid",
            IntArray::new(
                uuid.as_chunks::<4>()
                    .0
                    .iter()
                    .map(|part| i32::from_be_bytes(*part))
                    .collect(),
            ),
        );
    }
    if let Ok(nbt) = fastnbt::to_bytes(&data) {
        mob.set_data(key, &nbt);
    }
}

fn adult(mob: Entity<'_>) -> bool {
    MOBS.get()
        .is_some_and(|mobs| mobs.kind(mob) == Some(CINDERVANE))
        && mob.flags() & ENTITY_ALIVE != 0
        && !eggs::is_juvenile(mob)
}

fn pack_member(mob: Entity<'_>) -> bool {
    adult(mob) && !taming::is_tamed(mob)
}

fn nearby(world: World<'_>, mob: Entity<'_>, radius: f64, vertical: f64) -> Vec<i32> {
    let (Some(entities), Some(view)) = (ENTITIES.get(), mob.view()) else {
        return Vec::new();
    };
    let extent = [radius, vertical, radius];
    entities.entities_in(
        world,
        flight::offset(view.position, extent),
        flight::add(view.position, extent),
        mob.id(),
    )
}

fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    flight::length_squared(flight::offset(a, b))
}

fn parent_minimum(a: f64, b: f64) -> f64 {
    (2.75_f64.max((a + b) * 0.5 + 1.5)).powi(2)
}

fn width(mob: Entity<'_>) -> f64 {
    mob.body().map_or(0.0, |body| body.max[0] - body.min[0])
}

/// DragonFollowParentBehaviour.resolveParent: retain an assigned adult, otherwise choose
/// the nearest living female in the 20 x 8 search box.
fn find_parent(world: World<'_>, mob: Entity<'_>) -> Option<i32> {
    let (entities, mobs, view) = (ENTITIES.get()?, MOBS.get()?, mob.view()?);
    let assigned = link(mob, PARENT);
    let mut best: Option<(f64, i32, [u8; 16])> = None;
    for id in nearby(world, mob, 20.0, 8.0) {
        let Some(Some((uuid, female, at))) = entities.with_entity(world, id, |other| {
            adult(other)
                .then(|| {
                    Some((
                        mobs.uuid(other)?,
                        Appearance::is_female(other),
                        other.view()?.position,
                    ))
                })
                .flatten()
        }) else {
            continue;
        };
        if Some(uuid) == assigned {
            return Some(id);
        }
        let d = distance(view.position, at);
        if female && best.is_none_or(|(old, _, _)| d < old) {
            best = Some((d, id, uuid));
        }
    }
    save_link(mob, PARENT, best.map(|(_, _, uuid)| uuid));
    best.map(|(_, id, _)| id)
}

#[derive(Clone, Copy)]
struct Member {
    id: i32,
    uuid: [u8; 16],
    leader: Option<[u8; 16]>,
    female: bool,
    health: f32,
}

fn member(mob: Entity<'_>) -> Option<Member> {
    let mobs = MOBS.get()?;
    if !pack_member(mob) {
        return None;
    }
    let (hp, max) = mobs.health(mob)?;
    Some(Member {
        id: mob.id(),
        uuid: mobs.uuid(mob)?,
        leader: link(mob, PACK),
        female: Appearance::is_female(mob),
        health: hp / max.max(1.0),
    })
}

fn better(candidate: Member, current: Member) -> bool {
    let priority = |m: Member| (m.health * 100.0).round() as i32;
    priority(candidate) > priority(current)
        || (priority(candidate) == priority(current)
            && if (candidate.health - current.health).abs() > 0.0001 {
                candidate.health > current.health
            } else {
                candidate.uuid < current.uuid
            })
}

fn capacity(candidate: Member, me: Member, others: &[Member]) -> bool {
    let followers = others
        .iter()
        .filter(|m| m.uuid != candidate.uuid && m.leader == Some(candidate.uuid));
    if me.leader == Some(candidate.uuid) {
        followers.filter(|m| m.uuid < me.uuid).count() < PACK_CAPACITY - 1
    } else {
        followers.count() < PACK_CAPACITY - 1
    }
}

/// DragonPackFollowBehaviour.resolveLeader: male leaders, health priority, UUID tiebreak,
/// and at most three followers. Existing valid assignments take precedence.
fn find_leader(world: World<'_>, mob: Entity<'_>) -> Option<i32> {
    let entities = ENTITIES.get()?;
    let me = member(mob)?;
    let others: Vec<_> = nearby(world, mob, PACK_RADIUS, PACK_RADIUS)
        .into_iter()
        .filter_map(|id| entities.with_entity(world, id, member).flatten())
        .collect();
    let usable = |m: Member| {
        !m.female && m.leader.is_none_or(|uuid| uuid == m.uuid) && capacity(m, me, &others)
    };
    let selected = others
        .iter()
        .copied()
        .find(|m| Some(m.uuid) == me.leader && usable(*m))
        .or_else(|| {
            let mut best = (!me.female).then_some(me);
            for candidate in others.iter().copied().filter(|m| usable(*m)) {
                if best.is_none_or(|current| better(candidate, current)) {
                    best = Some(candidate);
                }
            }
            best.filter(|m| m.id != me.id)
        });
    save_link(mob, PACK, Some(selected.map_or(me.uuid, |m| m.uuid)));
    selected.map(|m| m.id)
}

/// CindervaneAutonomousFlightBehaviour.isGroundedPackFollower.
pub fn assigned_follower(mob: Entity<'_>) -> bool {
    pack_member(mob)
        && link(mob, PACK)
            .is_some_and(|leader| MOBS.get().and_then(|m| m.uuid(mob)) != Some(leader))
}

/// CindervanePackFollowBehaviour and DragonFollowParentBehaviour.
/// Ground paths stop explicitly at the released arrival thresholds.
pub fn social(phase: u32, world: World<'_>, mob: Entity<'_>) -> bool {
    let (Some(ai), Some(mobs), Some(entities), Some(view)) =
        (MOB_AI.get(), MOBS.get(), ENTITIES.get(), mob.view())
    else {
        return false;
    };
    let baby = eggs::is_juvenile(mob);
    if phase == goal_phase::STOP {
        ai.stop(mob);
        pack_flight::stop(mob.id());
        return false;
    }
    if taming::is_tamed(mob)
        || ai.target(mob).is_some()
        || !mobs.passengers(mob).is_empty()
        || mob.vehicle().is_some()
        || eggs::water_at(world, flight::cell(view.position))
        || flight::autonomous_landing(mob.id()).is_some()
        || protecting(mob.id())
    {
        return false;
    }
    let now = tick_count();
    let mut state = FOLLOW
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(&mob.id())
        .copied()
        .unwrap_or_default();
    if matches!(phase, goal_phase::CAN_START | goal_phase::CONTINUE) && now >= state.next_search {
        state.target = if baby {
            find_parent(world, mob)
        } else {
            find_leader(world, mob)
        };
        state.next_search =
            now + if baby || state.target.is_none() {
                20
            } else {
                60
            } + crate::ai::random(11);
        FOLLOW
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(mob.id(), state);
    }
    let target = state.target.and_then(|target| {
        entities
            .with_entity(world, target, |other| {
                if !adult(other) || (!baby && !pack_member(other)) {
                    return None;
                }
                let other_view = other.view()?;
                let body = other.body()?;
                let airborne = !other_view.on_ground
                    && flight::autonomous_landing(other.id()) != Some(true)
                    && (flight::autonomous_landing(other.id()) == Some(false)
                        || (1..=4)
                            .all(|y| flight::clear_body(world, body, [0.0, -f64::from(y), 0.0])));
                Some((
                    pack_flight::Leader {
                        id: other.id(),
                        view: other_view,
                        height: body.max[1] - body.min[1],
                        airborne,
                    },
                    if baby {
                        parent_minimum(width(mob), width(other))
                    } else {
                        100.0
                    },
                ))
            })
            .flatten()
    });
    let following_air = !baby && pack_flight::landing(mob.id()).is_some();
    if !baby {
        let leader = target.map(|(leader, _)| leader);
        if let Some(leader) = leader.filter(|leader| {
            leader.airborne && distance(view.position, leader.view.position) <= PACK_RADIUS.powi(2)
        }) {
            return match phase {
                goal_phase::CAN_START | goal_phase::CONTINUE => {
                    !view.on_ground
                        || mob.body().is_some_and(|body| {
                            (1..=8)
                                .all(|y| flight::clear_body(world, body, [0.0, f64::from(y), 0.0]))
                        })
                }
                goal_phase::START | goal_phase::TICK => {
                    ai.stop(mob);
                    pack_flight::follow(world, mob, view, leader);
                    false
                }
                _ => false,
            };
        }
        if following_air && !view.on_ground {
            return match phase {
                goal_phase::CAN_START | goal_phase::CONTINUE => true,
                goal_phase::START | goal_phase::TICK => {
                    pack_flight::land(world, mob, view, leader);
                    false
                }
                _ => false,
            };
        }
        if following_air {
            pack_flight::stop(mob.id());
        }
    }
    let Some((leader, minimum)) = target else {
        return false;
    };
    if !baby && !leader.view.on_ground {
        return false;
    }
    let at = leader.view.position;
    let d = distance(view.position, at);
    let in_range = if baby {
        d >= minimum && d <= 576.0
    } else {
        d > minimum && d <= PACK_RADIUS.powi(2)
    };
    match phase {
        goal_phase::CAN_START => in_range && (baby || d > 400.0),
        goal_phase::CONTINUE => in_range,
        goal_phase::START | goal_phase::TICK => {
            ai.look_at(mob, at);
            if !in_range || (baby && d < minimum * 1.2) {
                ai.stop(mob);
            } else if phase == goal_phase::START || now.is_multiple_of(8) || !ai.navigating(mob) {
                ai.walk_to(mob, at, if baby { 1.15 } else { 1.0 });
            }
            false
        }
        _ => false,
    }
}

pub fn protecting(id: i32) -> bool {
    FOLLOW
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(&id)
        .is_some_and(|state| state.protecting)
}

/// CindervaneTargetingBehaviour: a female protects her assigned babies; other wild adults
/// assist nearby packmates. Poll on staggered ticks so crowds do not search every tick.
pub fn defend(world: World<'_>, mob: Entity<'_>) -> Option<(bool, Option<i32>)> {
    let (entities, mobs, ai, view) = (ENTITIES.get()?, MOBS.get()?, MOB_AI.get()?, mob.view()?);
    if !pack_member(mob) {
        if let Some(state) = FOLLOW
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get_mut(&mob.id())
        {
            state.protecting = false;
        }
        return None;
    }
    if !(tick_count() + mob.id() as u64).is_multiple_of(10) {
        return None;
    }
    let uuid = mobs.uuid(mob)?;
    let usable = |id, range: f64| {
        entities
            .with_entity(world, id, |other| {
                crate::abilities::enemy(mob, other)
                    && mobs.kind(other) != Some(CINDERVANE)
                    && other
                        .view()
                        .is_some_and(|v| distance(view.position, v.position) <= range.powi(2))
            })
            .unwrap_or(false)
    };
    let mut babies = false;
    let mut threat: Option<(i32, i32)> = None;
    if Appearance::is_female(mob) {
        for id in nearby(world, mob, 16.0, 16.0) {
            if let Some(candidate) = entities
                .with_entity(world, id, |baby| {
                    if mobs.kind(baby) != Some(CINDERVANE)
                        || !eggs::is_juvenile(baby)
                        || baby.flags() & ENTITY_ALIVE == 0
                    {
                        return None;
                    }
                    if link(baby, PARENT) == Some(uuid) {
                        babies = true;
                        ai.last_hurt_by(baby)
                    } else {
                        None
                    }
                })
                .flatten()
                && usable(candidate.0, 16.0)
                && threat.is_none_or(|old| candidate.1 > old.1)
            {
                threat = Some(candidate);
            }
        }
    }
    FOLLOW
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .entry(mob.id())
        .or_default()
        .protecting = babies;
    if babies {
        return Some((true, threat.map(|(id, _)| id)));
    }
    let mut pack = nearby(world, mob, PACK_RADIUS, PACK_RADIUS);
    if let Some(leader) = link(mob, PACK).and_then(|uuid| mobs.by_uuid(world, &uuid))
        && leader != mob.id()
    {
        pack.insert(0, leader);
    }
    for id in pack {
        let Some(Some((target, attacker))) = entities.with_entity(world, id, |other| {
            pack_member(other).then(|| (ai.target(other), ai.last_hurt_by(other).map(|(id, _)| id)))
        }) else {
            continue;
        };
        if let Some(target) = [target, attacker]
            .into_iter()
            .flatten()
            .find(|id| usable(*id, 72.0))
        {
            return Some((false, Some(target)));
        }
    }
    Some((false, None))
}

#[derive(Clone, Copy)]
struct DrinkSite {
    water: moddedmc_api::Pos,
    stance: [f64; 3],
}

#[derive(Clone, Copy, Default)]
struct DrinkState {
    next: u64,
    site: Option<DrinkSite>,
    deadline: u64,
    align: Option<u64>,
    drinking: bool,
}

static DRINKS: LazyLock<Mutex<HashMap<i32, DrinkState>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn drinking(id: i32) -> bool {
    DRINKS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(&id)
        .is_some_and(|state| state.drinking)
}

fn dry(world: World<'_>, at: [f64; 3]) -> bool {
    let pos = flight::cell(at);
    let support = moddedmc_api::Pos {
        y: pos.y - 1,
        ..pos
    };
    world.fluid_id(pos).as_deref() == Some("minecraft:empty")
        && world.fluid_id(support).as_deref() == Some("minecraft:empty")
        && world.face_sturdy(support, 1)
}

fn source(world: World<'_>, pos: moddedmc_api::Pos) -> bool {
    let above = moddedmc_api::Pos {
        y: pos.y + 1,
        ..pos
    };
    world.fluid_id(pos).as_deref() == Some("minecraft:water")
        && world.fluid_id(above).as_deref() == Some("minecraft:empty")
        && world
            .collision_shape(above)
            .is_some_and(|shape| shape.is_empty())
}

fn water_surface(pos: moddedmc_api::Pos) -> [f64; 3] {
    [
        f64::from(pos.x) + 0.5,
        f64::from(pos.y) + 1.0,
        f64::from(pos.z) + 0.5,
    ]
}

fn in_reach(at: [f64; 3], water: [f64; 3]) -> bool {
    (at[0] - water[0]).hypot(at[2] - water[2]) <= 4.5 && (at[1] - water[1]).abs() <= 2.5
}

/// DragonDrinkBehaviour.findSites: source water within 12 horizontal and four vertical blocks.
/// shortcut: the host provides vanilla walk_to but no path endpoint preview or node limit.
/// Find a dry nearby stance and reject a failed approach after 240 ticks.
fn drink_site(world: World<'_>, mob: Entity<'_>) -> Option<DrinkSite> {
    let view = mob.view()?;
    let body = mob.body()?;
    let origin = flight::cell(view.position);
    for r in 0_i32..=12 {
        for dx in -r..=r {
            for dz in -r..=r {
                if dx.abs().max(dz.abs()) != r || dx * dx + dz * dz > 144 {
                    continue;
                }
                for dy in (-4..=4).rev() {
                    let water = moddedmc_api::Pos {
                        x: origin.x + dx,
                        y: origin.y + dy,
                        z: origin.z + dz,
                    };
                    if !source(world, water) {
                        continue;
                    }
                    let surface = water_surface(water);
                    if dry(world, view.position) && in_reach(view.position, surface) {
                        return Some(DrinkSite {
                            water,
                            stance: view.position,
                        });
                    }
                    for (x, z) in [
                        (3, 0),
                        (-3, 0),
                        (0, 3),
                        (0, -3),
                        (2, 2),
                        (2, -2),
                        (-2, 2),
                        (-2, -2),
                    ] {
                        for y in -2..=2 {
                            let stance = [
                                surface[0] + f64::from(x),
                                surface[1] + f64::from(y),
                                surface[2] + f64::from(z),
                            ];
                            if dry(world, stance)
                                && flight::clear_body(
                                    world,
                                    body,
                                    flight::offset(stance, view.position),
                                )
                            {
                                return Some(DrinkSite { water, stance });
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

/// DragonDrinkBehaviour: approach at .65, align 15 ticks, play the 80-tick drinking clip,
/// then wait 1200–2400 ticks (100 on failure).
pub fn drink(phase: u32, world: World<'_>, mob: Entity<'_>) -> bool {
    let (Some(ai), Some(mobs), Some(view)) = (MOB_AI.get(), MOBS.get(), mob.view()) else {
        return false;
    };
    let now = tick_count();
    let mut state = DRINKS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(&mob.id())
        .copied()
        .unwrap_or_default();
    let eligible = pack_member(mob)
        && ai.target(mob).is_none()
        && mobs.passengers(mob).is_empty()
        && mob.vehicle().is_none()
        && !flight::active(mob.id())
        && !eggs::water_at(world, flight::cell(view.position));
    let result = match phase {
        goal_phase::CAN_START if eligible && view.on_ground && now >= state.next => {
            state.site = drink_site(world, mob);
            state.next = now + 100;
            state.site.is_some()
        }
        goal_phase::CONTINUE => {
            eligible
                && state.site.is_some_and(|site| source(world, site.water))
                && now < state.deadline
        }
        goal_phase::START => {
            state.deadline = now + 240;
            state.align = None;
            ai.stop(mob);
            false
        }
        goal_phase::TICK => {
            if let Some(site) = state.site {
                let water = water_surface(site.water);
                if view.on_ground && dry(world, view.position) && in_reach(view.position, water) {
                    ai.stop(mob);
                    ai.look_at(mob, water);
                    let yaw = (water[2] - view.position[2])
                        .atan2(water[0] - view.position[0])
                        .to_degrees() as f32
                        - 90.0;
                    let delta = (yaw - view.yaw + 180.0).rem_euclid(360.0) - 180.0;
                    let turned = view.yaw + delta.clamp(-10.0, 10.0);
                    mob.change(&moddedmc_api::entities::EntityChange {
                        mask: moddedmc_api::entities::SET_YAW
                            | moddedmc_api::entities::SET_BODY_YAW
                            | moddedmc_api::entities::SET_HEAD_YAW,
                        yaw: turned,
                        body_yaw: turned,
                        head_yaw: turned,
                        ..Default::default()
                    });
                    let align = *state.align.get_or_insert(now + 15);
                    if now >= align && !state.drinking {
                        state.drinking = true;
                        state.deadline = now + 80;
                        state.next = state.deadline + 1200 + crate::ai::random(1201);
                    }
                } else if state.align.is_some() {
                    state.deadline = now;
                    state.next = now + 100;
                } else if !ai.navigating(mob) && !ai.walk_to(mob, site.stance, 0.65) {
                    state.deadline = now;
                }
            }
            false
        }
        goal_phase::STOP => {
            if !state.drinking || now < state.deadline {
                state.next = now + 100;
            }
            state.drinking = false;
            state.site = None;
            ai.stop(mob);
            false
        }
        _ => false,
    };
    if phase != goal_phase::CONTINUE {
        DRINKS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(mob.id(), state);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(id: u8, hp: f32, leader: Option<u8>) -> Member {
        Member {
            id: i32::from(id),
            uuid: [id; 16],
            leader: leader.map(|v| [v; 16]),
            female: false,
            health: hp,
        }
    }

    #[test]
    fn pack_rank_and_capacity() {
        assert!(better(m(2, 0.9, None), m(1, 0.8, None)));
        assert!(better(m(1, 1.0, None), m(2, 1.0, None)));
        let leader = m(1, 1.0, None);
        let followers = [m(2, 1.0, Some(1)), m(3, 1.0, Some(1)), m(4, 1.0, Some(1))];
        assert!(!capacity(leader, m(5, 1.0, None), &followers));
        assert!(capacity(leader, followers[2], &followers));
        assert!(!capacity(leader, m(5, 1.0, Some(1)), &followers));
    }

    #[test]
    fn parent_spacing_uses_both_widths() {
        assert_eq!(parent_minimum(0.5, 1.0), 2.75_f64.powi(2));
        assert_eq!(parent_minimum(2.0, 4.0), 4.5_f64.powi(2));
    }

    #[test]
    fn drinking_requires_horizontal_and_vertical_reach() {
        assert!(in_reach([0.0, 2.5, 0.0], [4.5, 0.0, 0.0]));
        assert!(!in_reach([0.0, 2.51, 0.0], [4.5, 0.0, 0.0]));
        assert!(!in_reach([0.0, 0.0, 0.0], [4.0, 0.0, 4.0]));
    }
}
