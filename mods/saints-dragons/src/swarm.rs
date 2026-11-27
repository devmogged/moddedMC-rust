//! Suspended wandering and combat for released Draconian Swarm members.

use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
};

use moddedmc_api::{
    Pos,
    content::World,
    entities::{
        ENTITY_ALIVE, ENTITY_LIVING, ENTITY_PLAYER, Entity, EntityBody, EntityChange, EntityView,
        SET_BODY_YAW, SET_HEAD_YAW, SET_MOTION, SET_YAW,
    },
    mob_ai::goal_phase,
    tick_count,
};

use crate::{
    CONTENT, ENTITIES, EQUIPMENT, MOB_AI, MOBS,
    ai::random,
    flight::{add, cell, length_squared, offset},
};

pub const WANDER: u64 = 10;
pub const TARGETING: u64 = 15;
pub const COMBAT: u64 = 16;
pub const MEMBERS: [(&str, f64); 3] = [("latcher", 4.0), ("winged", 1.5), ("whettled", 4.0)];
const WANDER_INTERVAL: u64 = 32;
const ARRIVAL: f64 = 0.6;

/// AsyncDragonPathfinder.calculateSwarmFlyingPathAsync ignores Pellucida.
/// shortcut: native movement still collides; use an entity-context block filter when available.
fn clear_body(world: World<'_>, body: EntityBody, offset: [f64; 3]) -> bool {
    let pellucida = CONTENT
        .get()
        .and_then(|content| content.state("saintsdragons:draconian_pellucida"));
    clear_body_with(
        body,
        offset,
        pellucida,
        |pos| world.collision_shape(pos).map(|shape| shape.is_empty()),
        |pos| {
            let mut state = [moddedmc_api::content::UNLOADED_STATE];
            world.read(&[pos], &mut state).then_some(state[0])
        },
    )
}

fn clear_body_with(
    body: EntityBody,
    offset: [f64; 3],
    pellucida: Option<u32>,
    mut empty_shape: impl FnMut(Pos) -> Option<bool>,
    mut state: impl FnMut(Pos) -> Option<u32>,
) -> bool {
    let min = cell(add(body.min, offset));
    let max = cell(add(body.max.map(|v| v - 1.0e-5), offset));
    (min.x..=max.x).all(|x| {
        (min.y..=max.y).all(|y| {
            (min.z..=max.z).all(|z| {
                let pos = Pos { x, y, z };
                empty_shape(pos).is_some_and(|empty| {
                    empty || pellucida.is_some_and(|id| state(pos) == Some(id))
                })
            })
        })
    })
}

#[derive(Clone, Copy)]
struct Course {
    at: [f64; 3],
    until: u64,
}

static COURSES: Mutex<Option<HashMap<i32, Course>>> = Mutex::new(None);

pub fn damage(name: &str) -> Option<f64> {
    MEMBERS
        .iter()
        .find_map(|(member, damage)| (*member == name).then_some(*damage))
}

pub fn forget(id: i32) {
    forget_course(id);
    let mut all = FIGHTS.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(fights) = all.as_mut() {
        fights.remove(&id);
    }
    drop(all);
    release(id);
}

fn forget_course(id: i32) {
    if let Some(courses) = COURSES
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_mut()
    {
        courses.remove(&id);
    }
}

pub fn hover(mob: Entity<'_>, motion: [f64; 3]) {
    if let Some(mobs) = MOBS.get() {
        mobs.set_no_gravity(mob, true);
    }
    let active = COURSES
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .is_some_and(|courses| courses.contains_key(&mob.id()));
    if !active && MOB_AI.get().and_then(|ai| ai.target(mob)).is_none() && motion != [0.0; 3] {
        let motion = if length_squared(motion) < 0.0001 {
            [0.0; 3]
        } else {
            motion.map(|speed| speed * 0.86)
        };
        mob.change(&EntityChange {
            mask: SET_MOTION,
            motion,
            ..EntityChange::default()
        });
    }
}

const FOLLOW_RANGE: f64 = 64.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Member {
    Latcher,
    Winged,
    Whettled,
}

impl Member {
    fn of(mob: Entity<'_>) -> Option<Self> {
        match MOBS.get()?.kind(mob)? {
            "saintsdragons:latcher" => Some(Self::Latcher),
            "saintsdragons:winged" => Some(Self::Winged),
            "saintsdragons:whettled" => Some(Self::Whettled),
            _ => None,
        }
    }

    fn width(self) -> f64 {
        if self == Self::Latcher { 1.75 } else { 1.5 }
    }

    fn speed(self) -> f64 {
        match self {
            Self::Latcher => 0.8,
            Self::Winged => 1.0,
            Self::Whettled => 0.9,
        }
    }

    /// LatcherBiteGoal, WingedPullAttackGoal and WhettledClawAttackGoal.
    fn melee(self) -> (f64, u64, u64, f32) {
        match self {
            Self::Latcher => (1.25, 2, 12, 4.0),
            Self::Winged => (1.75, 5, 16, 1.5),
            Self::Whettled => (2.5, 5, 18, 4.0),
        }
    }

    fn in_reach(self, distance_sq: f64, width: f64) -> bool {
        distance_sq <= (self.width() * 0.5 + width * 0.5 + self.melee().0).powi(2)
    }
}

/// DraconianArmorSet.pacifiesSwarm and AbstractDraconianSwarmEntity.isNeutralToward.
fn neutral_to_armor(armor: Option<[String; 4]>, last_attacker: bool) -> bool {
    !last_attacker
        && armor.is_some_and(|ids| {
            ids == [
                "saintsdragons:draconian_helmet",
                "saintsdragons:draconian_chestplate",
                "saintsdragons:draconian_leggings",
                "saintsdragons:draconian_boots",
            ]
        })
}

/// AbstractDraconianSwarmEntity.canHitWithSwarmAttack.
fn can_hit(mob: Entity<'_>, target: Entity<'_>) -> bool {
    let flags = target.flags();
    let Some(view) = target.view() else {
        return false;
    };
    target.id() != mob.id()
        && flags & ENTITY_ALIVE != 0
        && view.kind & ENTITY_LIVING != 0
        && target.type_id() != "enderman"
        && Member::of(target).is_none()
        && !matches!(
            MOBS.get().and_then(|mobs| mobs.game_mode(target)),
            Some(1 | 3)
        )
        && !(view.kind & ENTITY_PLAYER != 0
            && neutral_to_armor(
                EQUIPMENT
                    .get()
                    .and_then(|equipment| equipment.worn_armor(target)),
                MOB_AI
                    .get()
                    .and_then(|ai| ai.last_hurt_by(mob))
                    .is_some_and(|(id, _)| id == target.id()),
            ))
}

#[derive(Clone, Copy)]
struct Victim {
    view: EntityView,
    width: f64,
    center: [f64; 3],
}

fn victim(world: World<'_>, mob: Entity<'_>, id: i32) -> Option<Victim> {
    ENTITIES.get()?.with_entity(world, id, |target| {
        if !can_hit(mob, target) {
            return None;
        }
        let body = target.body()?;
        Some(Victim {
            view: target.view()?,
            width: body.max[0] - body.min[0],
            center: std::array::from_fn(|i| (body.min[i] + body.max[i]) * 0.5),
        })
    })?
}

// The SDK has no vanilla sensing/LOS query. Sample the straight sight line
// against block collision shapes; navigation around obstacles remains separate.
fn visible(world: World<'_>, from: [f64; 3], to: [f64; 3]) -> bool {
    let delta = offset(to, from);
    let steps = (length_squared(delta).sqrt() * 4.0).ceil() as u32;
    (1..steps).all(|step| {
        let point = add(from, delta.map(|v| v * f64::from(step) / f64::from(steps)));
        let local = point.map(|v| v - v.floor());
        world.collision_shape(cell(point)).is_some_and(|shape| {
            !shape
                .iter()
                .any(|b| (0..3).all(|i| local[i] >= b.min[i] && local[i] < b.max[i]))
        })
    })
}

/// HurtByTargetGoal, followed by players, then other living entities.
pub fn targeting(phase: u32, world: World<'_>, mob: Entity<'_>) -> bool {
    if phase != goal_phase::TICK {
        return matches!(phase, goal_phase::CAN_START | goal_phase::CONTINUE);
    }
    let (Some(ai), Some(entities), Some(view), Some(member)) =
        (MOB_AI.get(), ENTITIES.get(), mob.view(), Member::of(mob))
    else {
        return false;
    };
    let origin = view.position;
    let center = add(origin, [0.0, member.width() * 0.5, 0.0]);
    let nucleus = crate::nucleus::combat_center(mob);
    let eligible = |v: Victim| {
        length_squared(offset(v.view.position, origin)) <= FOLLOW_RANGE * FOLLOW_RANGE
            && nucleus.is_none_or(|at| {
                length_squared(offset(at, origin)) <= FOLLOW_RANGE * FOLLOW_RANGE
                    && length_squared(offset(at, v.view.position)) <= FOLLOW_RANGE * FOLLOW_RANGE
            })
    };
    let mut current = ai.target(mob).and_then(|id| {
        victim(world, mob, id)
            .filter(|v| eligible(*v))
            .map(|v| (id, v))
    });
    let last = ai.last_hurt_by(mob);
    let new_hurt = last.is_some_and(|(_, stamp)| {
        with_fight(mob.id(), |f| f.last_hurt.replace(stamp) != Some(stamp))
    });
    if new_hurt
        && let Some((id, _)) = last
        && victim(world, mob, id).is_some_and(eligible)
    {
        ai.set_target(world, mob, Some(id));
        with_fight(mob.id(), |f| f.unseen_since = None);
        return false;
    }
    let now = tick_count();
    let scan = (now + mob.id() as u64).is_multiple_of(10);
    if let Some((id, v)) = current {
        if last.is_some_and(|(attacker, _)| attacker == id) {
            return false;
        }
        if scan {
            let seen = visible(world, center, v.center);
            if with_fight(mob.id(), |f| {
                if seen {
                    f.unseen_since = None;
                    false
                } else {
                    now - *f.unseen_since.get_or_insert(now) >= 60
                }
            }) {
                current = None;
            }
        }
    }
    if current.is_none() && ai.target(mob).is_some() {
        ai.set_target(world, mob, None);
        with_fight(mob.id(), |f| f.unseen_since = None);
    }
    if !scan || current.is_some_and(|(_, v)| v.view.kind & ENTITY_PLAYER != 0) {
        return false;
    }
    let nearest = |ids: Vec<i32>| {
        let mut candidates: Vec<_> = ids
            .into_iter()
            .filter_map(|id| {
                let v = victim(world, mob, id)?;
                eligible(v).then_some((
                    id,
                    v.center,
                    length_squared(offset(v.view.position, origin)),
                ))
            })
            .collect();
        candidates.sort_by(|a, b| a.2.total_cmp(&b.2).then_with(|| a.0.cmp(&b.0)));
        candidates
            .into_iter()
            .find(|(_, at, _)| visible(world, center, *at))
            .map(|(id, _, _)| id)
    };
    let selected = nearest(entities.players(world)).or_else(|| {
        if current.is_some() {
            return None;
        }
        nearest(entities.entities_matching(
            world,
            origin.map(|v| v - FOLLOW_RANGE),
            origin.map(|v| v + FOLLOW_RANGE),
            mob.id(),
            0,
        ))
    });
    if let Some(id) = selected {
        ai.set_target(world, mob, Some(id));
        with_fight(mob.id(), |f| f.unseen_since = None);
    }
    false
}

#[derive(Clone, Copy)]
enum Action {
    Melee(u64),
    Charge(u64, [f64; 3], [f64; 3]),
    Dive(u64, [f64; 3], f64, f64),
}

#[derive(Default)]
struct Fight {
    target: Option<i32>,
    last_hurt: Option<i32>,
    unseen_since: Option<u64>,
    reacted_hurt: Option<i32>,
    action: Option<Action>,
    next_melee: u64,
    next_charge: u64,
    pulls: u8,
    orbit_until: u64,
    orbit_point: Option<[f64; 3]>,
    orbit_refresh: u64,
    retreat: Option<([f64; 3], u64)>,
    clip: Option<(&'static str, u64)>,
}

// These are Java's transient goal fields, not saved mob attributes.
static FIGHTS: Mutex<Option<HashMap<i32, Fight>>> = Mutex::new(None);
static RESERVATIONS: Mutex<Option<HashMap<i32, (i32, u64)>>> = Mutex::new(None);

fn with_fight<T>(id: i32, f: impl FnOnce(&mut Fight) -> T) -> T {
    f(FIGHTS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get_or_insert_default()
        .entry(id)
        .or_default())
}

/// DraconianSwarmCoordinator: one attacker per target, reservation expires after 80 ticks.
fn claim(id: i32, target: i32, now: u64) -> bool {
    let mut all = RESERVATIONS.lock().unwrap_or_else(PoisonError::into_inner);
    let all = all.get_or_insert_default();
    all.retain(|_, (_, until)| *until > now);
    if all.get(&target).is_some_and(|(owner, _)| *owner != id) {
        return false;
    }
    all.insert(target, (id, now + 80));
    true
}

fn release(id: i32) {
    if let Some(all) = RESERVATIONS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_mut()
    {
        all.retain(|_, (owner, _)| *owner != id);
    }
}

fn reserved_by_other(id: i32, target: i32, now: u64) -> bool {
    RESERVATIONS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .and_then(|all| all.get(&target))
        .is_some_and(|(owner, until)| *owner != id && *until > now)
}

fn unit(v: [f64; 3]) -> [f64; 3] {
    let distance = length_squared(v).sqrt();
    if distance < 0.0001 {
        [0.0, 0.0, 1.0]
    } else {
        v.map(|x| x / distance)
    }
}

fn move_toward(world: World<'_>, mob: Entity<'_>, view: EntityView, at: [f64; 3], speed: f64) {
    let delta = offset(at, view.position);
    let distance = length_squared(delta).sqrt();
    let desired = unit(delta).map(|v| v * speed.min(distance));
    let motion = if mob
        .body()
        .is_some_and(|body| clear_body(world, body, desired))
    {
        desired
    } else {
        [0.0; 3]
    };
    mob.change(&EntityChange {
        mask: SET_MOTION,
        motion,
        ..EntityChange::default()
    });
}

fn nearby(world: World<'_>, mob: Entity<'_>, member: Member, radius: f64) -> Vec<i32> {
    let (Some(entities), Some(view)) = (ENTITIES.get(), mob.view()) else {
        return vec![];
    };
    let center = add(view.position, [0.0, member.width() * 0.5, 0.0]);
    let extent = member.width() * 0.5 + radius;
    entities
        .entities_matching(
            world,
            center.map(|v| v - extent),
            center.map(|v| v + extent),
            mob.id(),
            0,
        )
        .into_iter()
        .filter(|&id| victim(world, mob, id).is_some())
        .collect()
}

fn impulse(world: World<'_>, id: i32, direction: [f64; 3], retain: f64, strength: f64, up: f64) {
    if let Some(entities) = ENTITIES.get() {
        entities.with_entity(world, id, |target| {
            if let Some(view) = target.view() {
                let motion = add(
                    add(
                        view.motion.map(|v| v * retain),
                        direction.map(|v| v * strength),
                    ),
                    [0.0, up, 0.0],
                );
                target.change(&EntityChange {
                    mask: SET_MOTION,
                    motion,
                    ..EntityChange::default()
                });
            }
        });
    }
}

fn retreat(f: &mut Fight, member: Member, view: EntityView, target: Victim, now: u64) {
    let distance = if member == Member::Winged { 9.0 } else { 7.0 };
    let direction = unit(offset(view.position, target.view.position));
    f.retreat = Some((
        add(
            add(view.position, direction.map(|v| v * distance)),
            [0.0, 1.5, 0.0],
        ),
        now + 30,
    ));
    f.action = None;
}

fn orbit(f: &mut Fight, member: Member, now: u64) {
    f.orbit_until = now
        + if member == Member::Winged {
            25 + random(25)
        } else {
            0
        };
    f.orbit_point = None;
    f.orbit_refresh = now;
}

fn orbit_slot(world: World<'_>, mob: Entity<'_>, target: i32, center: [f64; 3]) -> f64 {
    let (Some(entities), Some(ai)) = (ENTITIES.get(), MOB_AI.get()) else {
        return 0.0;
    };
    let mut members: Vec<_> = entities
        .entities_matching(
            world,
            center.map(|v| v - 40.0),
            center.map(|v| v + 40.0),
            mob.id(),
            0,
        )
        .into_iter()
        .filter(|&id| {
            entities
                .with_entity(world, id, |e| {
                    Member::of(e).is_some()
                        && e.flags() & ENTITY_ALIVE != 0
                        && ai.target(e) == Some(target)
                })
                .unwrap_or(false)
        })
        .collect();
    members.push(mob.id());
    members.sort_unstable();
    let index = members.iter().position(|&id| id == mob.id()).unwrap_or(0);
    index as f64 * std::f64::consts::TAU / members.len() as f64
}

pub fn clip(id: i32, now: u64) -> Option<&'static str> {
    FIGHTS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()?
        .get(&id)?
        .clip
        .and_then(|(clip, until)| (now < until).then_some(clip))
}

fn start_clip(
    world: World<'_>,
    mob: Entity<'_>,
    f: &mut Fight,
    clip: &'static str,
    until: u64,
    sound: Option<&str>,
) {
    f.clip = Some((clip, until));
    if cfg!(feature = "original-assets")
        && let (Some(sound), Some(effects), Some(view)) = (sound, crate::EFFECTS.get(), mob.view())
    {
        effects.play_sound(
            world,
            view.position,
            sound,
            moddedmc_api::effects::SOUND_NEUTRAL,
            1.0,
            0.95 + random(101) as f32 / 1000.0,
        );
    }
}

/// LatcherPursuitGoal and DraconianSwarmCombatMovementGoal, plus their attack goals.
pub fn combat(phase: u32, world: World<'_>, mob: Entity<'_>) -> bool {
    let (Some(ai), Some(member), Some(view)) = (MOB_AI.get(), Member::of(mob), mob.view()) else {
        return false;
    };
    let target = ai
        .target(mob)
        .and_then(|id| victim(world, mob, id).map(|v| (id, v)));
    if phase == goal_phase::STOP {
        with_fight(mob.id(), |f| {
            f.target = None;
            f.action = None;
            f.retreat = None;
        });
        release(mob.id());
        return false;
    }
    if phase != goal_phase::TICK {
        return target.is_some() && matches!(phase, goal_phase::CAN_START | goal_phase::CONTINUE);
    }
    let Some((id, target)) = target else {
        return false;
    };
    let now = tick_count();
    let mut f = with_fight(mob.id(), std::mem::take);
    if f.target != Some(id) {
        release(mob.id());
        f.target = Some(id);
        f.action = None;
        f.retreat = None;
        orbit(&mut f, member, now);
    }
    ai.look_at(mob, target.center);
    let hurt = ai.last_hurt_by(mob).map(|(_, stamp)| stamp);
    if member != Member::Latcher && hurt.is_some() && f.reacted_hurt != hurt {
        f.reacted_hurt = hurt;
        release(mob.id());
        retreat(&mut f, member, view, target, now);
    }
    tick_fight(world, mob, member, view, id, target, now, &mut f);
    with_fight(mob.id(), |state| *state = f);
    false
}

#[allow(clippy::too_many_arguments)]
fn tick_fight(
    world: World<'_>,
    mob: Entity<'_>,
    member: Member,
    view: EntityView,
    id: i32,
    target: Victim,
    now: u64,
    f: &mut Fight,
) {
    let Some(ai) = MOB_AI.get() else { return };
    let distance_sq = length_squared(offset(target.view.position, view.position));
    let in_reach = member.in_reach(distance_sq, target.width);
    let center = add(view.position, [0.0, member.width() * 0.5, 0.0]);
    if let Some((at, until)) = f.retreat {
        if now < until && length_squared(offset(at, view.position)) >= 2.25 {
            move_toward(world, mob, view, at, member.speed() * 1.1);
            return;
        }
        f.retreat = None;
        orbit(f, member, now);
    }
    if f.action.is_none()
        && now >= f.orbit_until
        && member != Member::Latcher
        && reserved_by_other(mob.id(), id, now)
    {
        f.orbit_until = now
            + if member == Member::Winged {
                25 + random(25)
            } else {
                35 + random(25)
            };
        f.orbit_refresh = now;
    }
    if f.action.is_none() && now >= f.orbit_until && claim(mob.id(), id, now) {
        if member == Member::Whettled
            && now >= f.next_charge
            && (12.25..=225.0).contains(&distance_sq)
        {
            f.action = Some(Action::Charge(now, [0.0; 3], [0.0; 3]));
        } else if member == Member::Winged && f.pulls >= 2 && distance_sq <= 256.0 {
            f.pulls = 0;
            f.action = Some(Action::Dive(now, [0.0; 3], 1.0, 1.0));
        } else if in_reach && now >= f.next_melee && !(member == Member::Winged && f.pulls >= 2) {
            f.action = Some(Action::Melee(now));
            f.next_melee = now + member.melee().2 + 1;
            let (clip, sound) = match member {
                Member::Latcher => (
                    if length_squared(view.motion) > 0.003 {
                        "bite_move"
                    } else {
                        "bite"
                    },
                    Some("saintsdragons:latcher_bite"),
                ),
                Member::Winged => ("attack", None),
                Member::Whettled => ("clawattack", Some("saintsdragons:whettled_strike")),
            };
            start_clip(world, mob, f, clip, now + member.melee().2, sound);
        } else {
            release(mob.id());
        }
    }
    match f.action {
        Some(Action::Melee(start)) => {
            let (_, impact, interval, damage) = member.melee();
            if now == start + impact {
                let victims = if member == Member::Whettled {
                    nearby(world, mob, member, 1.25)
                } else if in_reach {
                    vec![id]
                } else {
                    vec![]
                };
                let mut hit = false;
                for victim in victims {
                    if ai.attack(world, mob, victim, damage) {
                        hit = true;
                    }
                }
                if hit && member == Member::Winged {
                    f.pulls = (f.pulls + 1).min(2);
                    impulse(
                        world,
                        id,
                        unit(offset(center, target.center)),
                        0.25,
                        0.85,
                        0.0,
                    );
                }
                if hit && member != Member::Latcher {
                    release(mob.id());
                    retreat(f, member, view, target, now);
                    return;
                }
            }
            if now >= start + interval || (!in_reach && member != Member::Winged) {
                f.action = None;
                release(mob.id());
            }
        }
        Some(Action::Charge(start, mut direction, mut destination)) => {
            let elapsed = now - start;
            if elapsed < 12 {
                move_toward(world, mob, view, view.position, 0.0);
                return;
            }
            if direction == [0.0; 3] {
                direction = unit(offset(target.center, center));
                destination = add(target.center, direction.map(|v| v * 7.0));
                f.action = Some(Action::Charge(start, direction, destination));
                start_clip(
                    world,
                    mob,
                    f,
                    "movehornattack",
                    now + 24,
                    Some("saintsdragons:whettled_strike2"),
                );
            }
            let remaining = offset(destination, center);
            if elapsed >= 36 || (0..3).map(|i| remaining[i] * direction[i]).sum::<f64>() <= 0.0 {
                f.action = None;
                f.next_charge = now + 55;
                release(mob.id());
                return;
            }
            move_toward(world, mob, view, add(view.position, remaining), 1.5);
            let hits = nearby(world, mob, member, 0.85);
            let hit = hits
                .iter()
                .find(|&&v| v == id)
                .or(hits.first())
                .copied()
                .filter(|&v| ai.attack(world, mob, v, 9.0));
            if let Some(v) = hit {
                impulse(world, v, direction, 0.2, 1.25, 0.16);
                retreat(f, member, view, target, now);
            }
            if hit.is_some() || elapsed >= 36 {
                f.action = None;
                f.next_charge = now + 55;
                release(mob.id());
            }
            return;
        }
        Some(Action::Dive(
            start,
            mut direction,
            mut horizontal_correction,
            mut vertical_correction,
        )) => {
            let elapsed = now - start;
            if elapsed <= 8 {
                let away = unit([
                    center[0] - target.center[0],
                    0.0,
                    center[2] - target.center[2],
                ]);
                move_toward(
                    world,
                    mob,
                    view,
                    add(add(target.center, away.map(|v| v * 3.0)), [0.0, 6.0, 0.0]),
                    0.85,
                );
                return;
            }
            // WingedDiveBombGoal: track for eight ticks, then follow the 21-tick arc.
            let frame = ((elapsed - 9) as f64 / 21.0).clamp(0.0, 1.0);
            let difference = offset(target.center, center);
            let horizontal = [difference[0], 0.0, difference[2]];
            if direction == [0.0; 3] {
                direction = unit(horizontal);
                horizontal_correction = (0.1 * length_squared(horizontal).sqrt()).min(1.8);
                vertical_correction = (0.09 * difference[1].abs()).min(2.0);
                start_clip(world, mob, f, "attack2", now + 21, None);
            } else if frame < 0.55 && length_squared(horizontal) > 0.0001 {
                direction = unit(add(
                    direction.map(|v| v * 0.82),
                    unit(horizontal).map(|v| v * 0.18),
                ));
                horizontal_correction += 0.18
                    * ((0.1 * length_squared(horizontal).sqrt()).min(1.8) - horizontal_correction);
            }
            f.action = Some(Action::Dive(
                start,
                direction,
                horizontal_correction,
                vertical_correction,
            ));
            let forward = (horizontal_correction * 1.4 * (1.0 - (2.0 * (frame - 1.0)).exp()))
                .clamp(0.08, 1.85);
            let mut vertical = (vertical_correction
                * -1.35
                * ((frame / 0.8).min(1.0) * std::f64::consts::PI).cos())
            .clamp(-1.45, 0.95);
            if view.position[1] <= target.view.position[1] + 0.45 {
                vertical = vertical.max(0.35);
            } else if vertical < 0.0
                && !mob
                    .body()
                    .is_some_and(|body| clear_body(world, body, [0.0, vertical * 1.5, 0.0]))
            {
                vertical = 0.45;
            }
            let velocity = add(direction.map(|v| v * forward), [0.0, vertical, 0.0]);
            move_toward(
                world,
                mob,
                view,
                add(view.position, velocity),
                length_squared(velocity).sqrt(),
            );
            let hits = nearby(world, mob, member, 0.85);
            let hit = hits
                .iter()
                .find(|&&v| v == id)
                .or(hits.first())
                .copied()
                .filter(|&v| ai.attack(world, mob, v, 2.025));
            if let Some(v) = hit {
                impulse(world, v, unit(velocity), 0.25, 0.9, 0.12);
                retreat(f, member, view, target, now);
            }
            if hit.is_some() || elapsed >= 29 {
                f.action = None;
                release(mob.id());
            }
            return;
        }
        None => {}
    }
    if now < f.orbit_until {
        if now >= f.orbit_refresh || f.orbit_point.is_none() {
            let angle = now as f64 * 0.07 * if id & 1 == 0 { 1.0 } else { -1.0 }
                + orbit_slot(world, mob, id, target.center);
            let (radius, height) = if member == Member::Winged {
                (8.0, 3.0)
            } else {
                (6.0, 1.5)
            };
            f.orbit_point = Some(add(
                target.center,
                [angle.cos() * radius, height, angle.sin() * radius],
            ));
            f.orbit_refresh = now + 10;
        }
        if let Some(at) = f.orbit_point {
            move_toward(world, mob, view, at, member.speed() * 0.85);
        }
    } else if in_reach {
        move_toward(world, mob, view, view.position, 0.0);
    } else {
        let prediction = if member == Member::Winged { 3.0 } else { 2.0 };
        let at = add(
            target.center,
            [
                target.view.motion[0] * prediction,
                target.view.motion[1],
                target.view.motion[2] * prediction,
            ],
        );
        move_toward(world, mob, view, at, member.speed());
    }
}

pub fn wander(phase: u32, world: World<'_>, mob: Entity<'_>) -> bool {
    if phase == goal_phase::STOP {
        forget_course(mob.id());
        return false;
    }
    if phase == goal_phase::CAN_START && random(WANDER_INTERVAL) != 0 {
        return false;
    }
    let (Some(view), Some(ai)) = (mob.view(), MOB_AI.get()) else {
        return false;
    };
    if ai.target(mob).is_some() {
        return false;
    }
    let now = tick_count();
    if phase == goal_phase::CAN_START {
        let Some(at) = destination(world, mob, view.position, view.yaw) else {
            return false;
        };
        let mut locked = COURSES.lock().unwrap_or_else(PoisonError::into_inner);
        let courses = locked.get_or_insert_default();
        courses.retain(|_, course| course.until > now);
        courses.insert(
            mob.id(),
            Course {
                at,
                until: now + 200,
            },
        );
        return true;
    }
    let course = COURSES
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .and_then(|courses| courses.get(&mob.id()))
        .copied();
    let Some(course) = course else {
        return false;
    };
    let delta = offset(course.at, view.position);
    let distance = length_squared(delta).sqrt();
    if phase == goal_phase::CONTINUE {
        return course.until > now && distance > ARRIVAL;
    }
    if phase != goal_phase::TICK || distance <= ARRIVAL {
        return false;
    }
    let speed = match MOBS.get().and_then(|mobs| mobs.kind(mob)) {
        Some("saintsdragons:latcher") => 0.34,
        Some("saintsdragons:winged") => 0.43,
        Some("saintsdragons:whettled") => 0.38,
        _ => return false,
    } * (distance / (ARRIVAL * 2.5)).clamp(0.25, 1.0);
    let motion = std::array::from_fn(|axis| {
        let weight = if axis == 1 { 0.12 } else { 0.16 };
        view.motion[axis] + weight * (delta[axis] * speed / distance - view.motion[axis])
    });
    if !mob
        .body()
        .is_some_and(|body| clear_body(world, body, motion))
    {
        forget_course(mob.id());
        return false;
    }
    let heading = -motion[0].atan2(motion[2]).to_degrees() as f32;
    let turn = (heading - view.yaw + 180.0).rem_euclid(360.0) - 180.0;
    let yaw = view.yaw + turn.clamp(-7.0, 7.0);
    mob.change(&EntityChange {
        mask: SET_MOTION | SET_YAW | SET_BODY_YAW | SET_HEAD_YAW,
        motion,
        yaw,
        body_yaw: yaw,
        head_yaw: yaw,
        ..EntityChange::default()
    });
    true
}

// shortcut: routes are straight, use waypoint search when combat needs turns around obstacles.
fn destination(world: World<'_>, mob: Entity<'_>, from: [f64; 3], yaw: f32) -> Option<[f64; 3]> {
    let body = mob.body()?;
    for _ in 0..4 {
        let heading = f64::from(yaw).to_radians() + (random(181) as f64 - 90.0).to_radians();
        let distance = (4 + random(13)) as f64;
        let delta = [
            -heading.sin() * distance,
            random(17) as f64 - 8.0,
            heading.cos() * distance,
        ];
        let at = add(from, delta);
        if world.fluid_id(cell(at)).as_deref() != Some("minecraft:empty") {
            continue;
        }
        let steps = length_squared(delta).sqrt().ceil() as u32;
        if (1..=steps).all(|step| {
            clear_body(
                world,
                body,
                delta.map(|v| v * f64::from(step) / f64::from(steps)),
            )
        }) {
            return Some(at);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swarm_clearance_ignores_only_pellucida_in_shifted_body() {
        let body = EntityBody {
            min: [0.25, 0.0, 0.25],
            max: [1.75, 1.5, 1.75],
            ..EntityBody::default()
        };
        let delta = [1.0, 0.0, 0.0];
        let pellucida = Pos { x: 1, y: 0, z: 0 };
        let stone = Pos { x: 2, y: 1, z: 1 };
        let state = |pos| Some(if pos == pellucida { 42 } else { 43 });
        let shapes = |pos| Some(pos != pellucida);
        assert!(clear_body_with(body, delta, Some(42), shapes, state));
        assert!(!clear_body_with(body, delta, None, shapes, state));
        assert!(!clear_body_with(
            body,
            delta,
            Some(42),
            |pos| Some(pos != pellucida && pos != stone),
            state,
        ));
        assert!(!clear_body_with(
            body,
            delta,
            Some(42),
            |pos| (pos != stone).then_some(pos != pellucida),
            state,
        ));
    }

    #[test]
    fn full_draconian_set_pacifies_unless_last_attacker() {
        let full = [
            "saintsdragons:draconian_helmet",
            "saintsdragons:draconian_chestplate",
            "saintsdragons:draconian_leggings",
            "saintsdragons:draconian_boots",
        ]
        .map(str::to_owned);
        assert!(neutral_to_armor(Some(full.clone()), false));
        assert!(!neutral_to_armor(Some(full.clone()), true));
        for slot in 0..4 {
            let mut partial = full.clone();
            partial[slot] = "minecraft:air".to_owned();
            assert!(!neutral_to_armor(Some(partial), false));
        }
        assert!(!neutral_to_armor(None, false));
    }

    #[test]
    fn released_melee_reach_and_timing() {
        assert!(Member::Latcher.in_reach(2.425_f64.powi(2), 0.6));
        assert!(!Member::Latcher.in_reach(2.426_f64.powi(2), 0.6));
        assert_eq!(Member::Winged.melee(), (1.75, 5, 16, 1.5));
        assert_eq!(Member::Whettled.melee(), (2.5, 5, 18, 4.0));
    }

    #[test]
    fn reservations_expire_and_release() {
        assert!(claim(-101, -102, 0));
        assert!(!claim(-103, -102, 79));
        assert!(claim(-103, -102, 80));
        release(-103);
        assert!(claim(-101, -102, 81));
        release(-101);
    }
}
