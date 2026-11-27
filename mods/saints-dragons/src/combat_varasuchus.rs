//! Varasuchus combat tracks and brain from Saint's Dragons 0.9.88.
use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
};

use moddedmc_api::{
    content::World,
    effects::SOUND_NEUTRAL,
    entities::{Entity, EntityChange, SET_BODY_YAW, SET_MOTION, SET_YAW},
    mob_ai::goal_phase,
    tick_count,
};
use serde::{Deserialize, Serialize};

use crate::{EFFECTS, ENTITIES, MOB_AI, MOBS, abilities, flight, hunger, taming};

pub const TARGET: u64 = 150;
pub const COMBAT: u64 = 151;
const KIND: &str = "saintsdragons:varasuchus";
const DATA: &str = "saintsdragons:varasuchus_combat";
const CADENCE: u64 = 30;
const BARRAGE_HITS: &[u64] = &[2, 8, 14, 20, 24, 30, 36, 41, 46, 51, 61, 71, 78, 82];

#[derive(Clone, Copy, Default, Serialize, Deserialize)]
#[serde(default)]
struct Saved {
    #[serde(rename = "PhaseTwo")]
    phase_two: bool,
}
impl Saved {
    fn load(mob: Entity<'_>) -> Self {
        mob.data(DATA)
            .and_then(|bytes| fastnbt::from_bytes(&bytes).ok())
            .unwrap_or_default()
    }
    fn save(self, mob: Entity<'_>) {
        if let Ok(bytes) = fastnbt::to_bytes(&self) {
            mob.set_data(DATA, &bytes);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Attack {
    Bite,
    Bite2,
    Horn,
    Claw,
    Tail,
    Barrage,
    Shift,
}
impl Attack {
    fn duration(self, wet: bool) -> u64 {
        match self {
            Self::Bite | Self::Bite2 | Self::Horn => 17,
            Self::Claw => 5,
            Self::Tail => 29,
            Self::Barrage => 93,
            Self::Shift => {
                if wet {
                    33
                } else {
                    21
                }
            }
        }
    }
    fn cooldown(self) -> u64 {
        match self {
            Self::Bite | Self::Bite2 => 15,
            Self::Horn | Self::Claw => 3,
            Self::Tail => 5,
            Self::Barrage => 16,
            Self::Shift => 0,
        }
    }
    fn hit(self, tick: u64) -> bool {
        match self {
            Self::Bite | Self::Bite2 | Self::Horn => tick == 5,
            Self::Claw => tick == 1,
            Self::Tail => tick == 12,
            Self::Barrage => BARRAGE_HITS.contains(&tick),
            Self::Shift => false,
        }
    }
    fn damage(self, phase_two: bool) -> f32 {
        match self {
            Self::Bite => 15.0,
            Self::Bite2 => 25.0,
            Self::Horn => {
                if phase_two {
                    15.8
                } else {
                    8.0
                }
            }
            Self::Claw => 12.0,
            Self::Tail => 7.0,
            Self::Barrage => 15.0,
            Self::Shift => 0.0,
        }
    }
    fn clip(self, phase_two: bool, wet: bool, left: bool) -> &'static str {
        match self {
            Self::Bite => "bite",
            Self::Bite2 => "bite2",
            Self::Horn => "horn_gore",
            Self::Claw => {
                if left {
                    "claw_left"
                } else {
                    "claw_right"
                }
            }
            Self::Tail => {
                if left {
                    "tail_attack_left"
                } else {
                    "tail_attack_right"
                }
            }
            Self::Barrage => "slash_barrage",
            Self::Shift => match (phase_two, wet) {
                (true, true) => "phase2_underwater",
                (true, false) => "phase2",
                (false, true) => "phase1_underwater",
                (false, false) => "phase1",
            },
        }
    }
    fn sound(self, phase_two: bool, wet: bool) -> &'static str {
        match self {
            Self::Bite => "saintsdragons:varasuchus_bite1",
            Self::Bite2 => "saintsdragons:varasuchus_bite2",
            Self::Horn => "saintsdragons:varasuchus_horngore",
            Self::Claw => "saintsdragons:varasuchus_claw",
            Self::Tail => "saintsdragons:varasuchus_tail_attack",
            Self::Barrage => "saintsdragons:varasuchus_slash_barrage",
            Self::Shift => match (phase_two, wet) {
                (true, true) => "saintsdragons:varasuchus_phase2_underwater",
                (true, false) => "saintsdragons:varasuchus_phase2",
                (false, true) => "saintsdragons:varasuchus_phase1_underwater",
                (false, false) => "saintsdragons:varasuchus_phase1",
            },
        }
    }
}
#[derive(Clone, Copy)]
struct Running {
    attack: Attack,
    start: u64,
    end: u64,
    clip: &'static str,
}
#[derive(Clone, Copy, Default)]
struct State {
    active: Option<Running>,
    next: u64,
    ready: [u64; 7],
    last_hurt: Option<i32>,
    last_owner_hurt: Option<i32>,
    linger: Option<u64>,
    left_claw: bool,
    left_tail: bool,
    committed: bool,
}
// Short-lived ability and targeting timers. Persist only the phase, as in Varasuchus.addAdditionalSaveData.
static STATES: Mutex<Option<HashMap<i32, State>>> = Mutex::new(None);
fn state(mob: Entity<'_>, f: impl FnOnce(&mut State)) {
    let mut all = STATES.lock().unwrap_or_else(PoisonError::into_inner);
    f(all.get_or_insert_default().entry(mob.id()).or_default());
}
fn snapshot(mob: Entity<'_>) -> State {
    let mut copy = State::default();
    state(mob, |s| copy = *s);
    copy
}
fn wet(world: World<'_>, at: [f64; 3]) -> bool {
    matches!(
        world.fluid_id(flight::cell(at)).as_deref(),
        Some("minecraft:water" | "minecraft:flowing_water")
    )
}
fn dist(a: [f64; 3], b: [f64; 3]) -> f64 {
    flight::length_squared(flight::offset(a, b)).sqrt()
}
/// VarasuchusCombatBehaviour.tryPerformAttacks checks sensing only on land.
fn has_line_of_sight(world: World<'_>, mob: Entity<'_>, target: i32) -> bool {
    // shortcut: the SDK exposes collision bounds, not the pose-specific eye height.
    let eye = |e: Entity<'_>| {
        let mut p = e.view()?.position;
        let body = e.body()?;
        p[1] = body.min[1] + (body.max[1] - body.min[1]) * 0.85;
        Some(p)
    };
    ENTITIES.get().is_some_and(|entities| {
        entities.with_entity(world, target, |other| {
            eye(mob).zip(eye(other)).is_some_and(|(from, to)| {
                world
                    .raycast(from, to, Some(mob.id()))
                    .is_some_and(|(block, _)| block.is_none())
            })
        }) == Some(true)
    })
}

/// Vanilla armor reduction used by both released Varasuchus penetration solvers.
fn damage_after_armor(damage: f32, armor: f32, toughness: f32) -> f32 {
    let reduction = (armor - damage / (2.0 + toughness / 4.0))
        .max(armor * 0.2)
        .min(20.0);
    damage * (1.0 - reduction / 25.0)
}

/// VarasuchusHornGoreAbility and VarasuchusTailAttackAbility use different
/// initial bounds, iteration counts and return values for their inverse search.
fn penetrating_damage(
    attack: Attack,
    phase_two: bool,
    damage: f32,
    armor: f32,
    toughness: f32,
) -> f32 {
    let (desired, mut lo, mut hi, steps) = match attack {
        Attack::Horn => {
            let penetration = if phase_two { 5.0 } else { 3.0 };
            let desired = damage_after_armor(damage, (armor - penetration).max(0.0), toughness);
            (desired, 0.0, (desired + 16.0).max(16.0), 20)
        }
        Attack::Tail => (damage, damage, damage, 10),
        _ => return damage,
    };
    for _ in 0..8 {
        if damage_after_armor(hi, armor, toughness) >= desired {
            break;
        }
        hi *= 2.0;
    }
    for _ in 0..steps {
        let mid = (lo + hi) * 0.5;
        if damage_after_armor(mid, armor, toughness) < desired {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    if attack == Attack::Horn {
        (lo + hi) * 0.5
    } else {
        hi
    }
}

/// LivingEntity.knockback, followed by the horn ability's minimum vertical lift.
fn horn_motion(
    motion: [f64; 3],
    forward: [f64; 3],
    grounded: bool,
    resistance: f64,
    phase_two: bool,
) -> [f64; 3] {
    let (strength, lift) = if phase_two { (3.5, 0.9) } else { (2.0, 0.5) };
    let strength = strength * (1.0 - resistance);
    let mut motion = motion;
    if strength > 0.0 {
        motion[0] = motion[0] * 0.5 + forward[0] * strength;
        motion[2] = motion[2] * 0.5 + forward[2] * strength;
        if grounded {
            motion[1] = (motion[1] * 0.5 + strength).min(0.4);
        }
    }
    motion[1] = motion[1].max(lift);
    motion
}
fn enemy(mob: Entity<'_>, other: Entity<'_>) -> bool {
    abilities::enemy(mob, other)
        && !MOBS.get().is_some_and(|m| m.kind(other) == Some(KIND))
        && !taming::owner(mob).is_some_and(|owner| taming::owner(other) == Some(owner))
}
fn eligible(mob: Entity<'_>) -> bool {
    !crate::eggs::is_juvenile(mob)
        && taming::order_name(mob) != "stay"
        && mob.vehicle().is_none()
        && MOBS.get().is_some_and(|m| m.passengers(mob).is_empty())
}
fn target_at(world: World<'_>, mob: Entity<'_>, id: i32) -> Option<([f64; 3], f64, bool)> {
    ENTITIES.get()?.with_entity(world, id, |other| {
        if !enemy(mob, other) {
            return None;
        }
        let body = other.body()?;
        Some((
            other.view()?.position,
            body.max[0] - body.min[0],
            is_prey(other),
        ))
    })?
}
fn is_prey(other: Entity<'_>) -> bool {
    // Released passive land and aquatic prey; tag registration may be supplied by the pack.
    let id = other.type_id();
    let full = if id.contains(':') {
        id.to_owned()
    } else {
        format!("minecraft:{id}")
    };
    matches!(
        full.as_str(),
        "minecraft:cod" | "minecraft:cow" | "minecraft:salmon" | "minecraft:tropical_fish"
    ) || crate::TAGS.get().is_some_and(|tags| {
        tags.contains(
            "minecraft:entity_type",
            "saintsdragons:hunt_prey/varasuchus",
            &full,
        )
    })
}
fn nearest(
    world: World<'_>,
    mob: Entity<'_>,
    at: [f64; 3],
    range: f64,
    predicate: impl Fn(Entity<'_>) -> bool,
) -> Option<i32> {
    let entities = ENTITIES.get()?;
    entities
        .entities_in(
            world,
            at.map(|x| x - range),
            at.map(|x| x + range),
            mob.id(),
        )
        .into_iter()
        .filter_map(|id| {
            entities.with_entity(world, id, |e| {
                if !enemy(mob, e) || !predicate(e) {
                    return None;
                }
                let d = dist(at, e.view()?.position);
                (d <= range).then_some((d, id))
            })?
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, id)| id)
}

/// VarasuchusTargetingBehaviour: owner defense, retaliation, wild baby defense and hunting.
pub fn targeting(phase: u32, world: World<'_>, mob: Entity<'_>) -> bool {
    if phase != goal_phase::TICK {
        return matches!(phase, goal_phase::CAN_START | goal_phase::CONTINUE);
    }
    let (Some(ai), Some(mobs), Some(entities), Some(view)) =
        (MOB_AI.get(), MOBS.get(), ENTITIES.get(), mob.view())
    else {
        return false;
    };
    if crate::eggs::is_juvenile(mob) {
        // shortcut: last_hurt_by reports successful hits, not the Fabric attack callback.
        if let Some((id, stamp)) = ai.last_hurt_by(mob)
            && snapshot(mob).last_hurt != Some(stamp)
        {
            state(mob, |s| s.last_hurt = Some(stamp));
            entities.with_entity(world, id, |player| {
                if matches!(player.type_id(), "player" | "minecraft:player") {
                    crate::advancements::award(player, "why", "hit_baby");
                }
            });
        }
    }
    if !eligible(mob) {
        ai.set_target(world, mob, None);
        return false;
    }
    let s = snapshot(mob);
    let current = ai.target(mob);
    if let Some(id) = current
        && !target_at(world, mob, id).is_some_and(|(p, _, _)| {
            dist(view.position, p) <= if s.committed { 64.0 } else { 32.0 }
        })
    {
        ai.set_target(world, mob, None);
    }
    let mut found = None;
    if let Some(owner) = taming::owner(mob).and_then(|uuid| mobs.by_uuid(world, &uuid))
        && let Some(Some((id, stamp))) = entities.with_entity(world, owner, |e| ai.last_hurt_by(e))
        && s.last_owner_hurt != Some(stamp)
    {
        state(mob, |s| s.last_owner_hurt = Some(stamp));
        found = target_at(world, mob, id).map(|_| id);
    }
    if let Some((id, stamp)) = ai.last_hurt_by(mob)
        && s.last_hurt != Some(stamp)
    {
        state(mob, |s| s.last_hurt = Some(stamp));
        if found.is_none() {
            found = target_at(world, mob, id).map(|_| id);
        }
    }
    if let Some(id) = found {
        ai.set_target(world, mob, Some(id));
        state(mob, |s| s.committed = true);
    } else if ai.target(mob).is_none() && (tick_count() + mob.id() as u64).is_multiple_of(10) {
        let at = view.position;
        if !taming::is_tamed(mob) {
            // shortcut: nearby wild babies approximate the original assigned-mother and vision checks.
            let baby = entities
                .entities_in(world, at.map(|x| x - 16.0), at.map(|x| x + 16.0), mob.id())
                .into_iter()
                .any(|id| {
                    entities.with_entity(world, id, |e| {
                        mobs.kind(e) == Some(KIND)
                            && crate::eggs::is_juvenile(e)
                            && !taming::is_tamed(e)
                    }) == Some(true)
                });
            if baby {
                found = nearest(world, mob, at, 32.0, |e| {
                    matches!(e.type_id(), "player" | "minecraft:player")
                });
            }
        }
        if found.is_none() && hunger::level(mob) <= 60 && (tick_count() / 10).is_multiple_of(8) {
            found = nearest(world, mob, at, 32.0, is_prey);
        }
        if let Some(id) = found {
            ai.set_target(world, mob, Some(id));
            state(mob, |s| s.committed = false);
        }
    }
    // Phase two remains active for 30 seconds after the target leaves.
    let active = snapshot(mob).active;
    if let Some(r) = active
        && r.attack == Attack::Shift
        && tick_count() >= r.end
    {
        state(mob, |s| s.active = None);
    }
    if Saved::load(mob).phase_two && snapshot(mob).active.is_none() {
        if ai
            .target(mob)
            .and_then(|id| target_at(world, mob, id))
            .is_some_and(|(p, _, _)| dist(view.position, p) <= 32.0)
        {
            state(mob, |s| s.linger = None);
        } else {
            let now = tick_count();
            let mut due = false;
            state(mob, |s| due = now >= *s.linger.get_or_insert(now + 600));
            let in_water = wet(world, view.position);
            if due && (in_water || view.on_ground) {
                start(world, mob, Attack::Shift, false, in_water);
                state(mob, |s| s.linger = None);
            }
        }
    }
    false
}

/// VarasuchusCombatBehaviour.choosePrimaryAttack, in the released priority order.
fn choose(
    phase: bool,
    gap: f64,
    prey: bool,
    wet: bool,
    ready: impl Fn(Attack) -> bool,
    roll: u64,
    coin: bool,
) -> Option<Attack> {
    use Attack::*;
    let bite = if phase { Bite2 } else { Bite };
    if prey {
        return (gap <= if wet { 5.0 } else { 1.45 } && ready(bite)).then_some(bite);
    }
    if gap <= 3.5 && ready(Horn) {
        return Some(Horn);
    }
    if gap > 5.0 {
        return None;
    }
    if phase && ready(Barrage) && roll < 30 {
        return Some(Barrage);
    }
    if phase {
        if ready(bite) && ready(Claw) {
            return Some(if coin { bite } else { Claw });
        }
        if ready(bite) {
            return Some(bite);
        }
        if ready(Claw) {
            return Some(Claw);
        }
    }
    let tail = !phase && !wet && gap > 3.5 && ready(Tail);
    if tail && roll < 35 {
        return Some(Tail);
    }
    if ready(bite) {
        return Some(bite);
    }
    if ready(Horn) {
        return Some(Horn);
    }
    if phase && ready(Barrage) {
        return Some(Barrage);
    }
    tail.then_some(Tail)
}
fn start(world: World<'_>, mob: Entity<'_>, attack: Attack, phase_two: bool, wet: bool) {
    let now = tick_count();
    let end = now + attack.duration(wet);
    state(mob, |s| {
        let left = match attack {
            Attack::Claw => {
                s.left_claw = !s.left_claw;
                s.left_claw
            }
            Attack::Tail => {
                s.left_tail = !s.left_tail;
                s.left_tail
            }
            _ => false,
        };
        s.active = Some(Running {
            attack,
            start: now,
            end,
            clip: attack.clip(phase_two, wet, left),
        });
        s.ready[attack as usize] = end + attack.cooldown();
        if attack != Attack::Shift {
            s.next = now + CADENCE;
        }
    });
    if attack == Attack::Shift {
        Saved { phase_two }.save(mob);
    }
    if let (Some(effects), Some(view)) = (EFFECTS.get(), mob.view()) {
        let sound = if cfg!(feature = "original-assets") {
            attack.sound(phase_two, wet)
        } else {
            "entity.ender_dragon.growl"
        };
        effects.play_sound(world, view.position, sound, SOUND_NEUTRAL, 1.0, 1.0);
    }
}
pub fn clip(mob: Entity<'_>) -> Option<&'static str> {
    snapshot(mob)
        .active
        .filter(|r| tick_count() < r.end)
        .map(|r| r.clip)
}
pub fn combat(phase: u32, world: World<'_>, mob: Entity<'_>) -> bool {
    let (Some(ai), Some(view)) = (MOB_AI.get(), mob.view()) else {
        return false;
    };
    let target = ai
        .target(mob)
        .and_then(|id| target_at(world, mob, id).map(|loc| (id, loc)));
    let valid =
        eligible(mob) && target.is_some_and(|(_, (p, _, _))| dist(view.position, p) <= 32.0);
    let s = snapshot(mob);
    match phase {
        goal_phase::CAN_START | goal_phase::CONTINUE => {
            valid || (eligible(mob) && s.active.is_some_and(|r| r.attack == Attack::Shift))
        }
        goal_phase::STOP => {
            state(mob, |s| {
                if s.active.is_some_and(|r| r.attack != Attack::Shift) {
                    s.active = None;
                }
                s.next = 0;
            });
            ai.stop(mob);
            false
        }
        goal_phase::TICK => {
            let now = tick_count();
            let in_water = wet(world, view.position);
            let phase_two = Saved::load(mob).phase_two;
            if let Some(r) = s.active {
                if now >= r.end {
                    state(mob, |s| s.active = None);
                } else {
                    if r.attack == Attack::Barrage
                        && let Some((_, (p, _, _))) = target
                    {
                        ai.walk_to(mob, p, 1.0);
                    } else {
                        ai.stop(mob);
                    }
                    if r.attack.hit(now - r.start)
                        && let Some((id, _)) = target
                    {
                        hit(world, mob, id, r.attack, phase_two, in_water);
                    }
                    return false;
                }
            }
            let Some((id, (p, width, prey))) = target else {
                return false;
            };
            let prey = prey && !s.committed;
            if !phase_two
                && (view.on_ground || in_water)
                && MOBS
                    .get()
                    .and_then(|m| m.health(mob))
                    .is_some_and(|(hp, max)| hp <= max * 0.5)
            {
                start(world, mob, Attack::Shift, true, in_water);
                ai.stop(mob);
                return false;
            }
            let own_width = mob.body().map_or(2.5, |b| b.max[0] - b.min[0]);
            let gap = (dist(view.position, p) - (own_width + width) * 0.5).max(0.0);
            let delta = flight::offset(p, view.position);
            let yaw = delta[2].atan2(delta[0]).to_degrees() as f32 - 90.0;
            ai.look_at(mob, p);
            mob.change(&EntityChange {
                mask: SET_YAW | SET_BODY_YAW,
                yaw,
                body_yaw: yaw,
                ..EntityChange::default()
            });
            let stop = if prey && !in_water { 1.45 } else { 5.0 };
            if gap > stop {
                if in_water {
                    let length = dist(p, view.position).max(0.01);
                    mob.change(&EntityChange {
                        mask: SET_MOTION,
                        motion: delta.map(|d| d / length * 0.30),
                        ..EntityChange::default()
                    });
                } else if now.is_multiple_of(5) || !ai.navigating(mob) {
                    ai.walk_to(mob, p, 1.5);
                }
            } else {
                ai.stop(mob);
            }
            if now >= s.next
                && gap <= 5.0
                && (in_water || has_line_of_sight(world, mob, id))
                && let Some(attack) = choose(
                    phase_two,
                    gap,
                    prey,
                    in_water,
                    |a| now >= s.ready[a as usize],
                    crate::ai::random(100),
                    crate::ai::random(2) == 0,
                )
            {
                start(world, mob, attack, phase_two, in_water);
                ai.stop(mob);
            }
            false
        }
        _ => false,
    }
}
/// The direct AI bite, gore, claw and barrage use the committed target; tail sweeps a cone.
fn hit(
    world: World<'_>,
    mob: Entity<'_>,
    primary: i32,
    attack: Attack,
    phase_two: bool,
    wet: bool,
) {
    let (Some(ai), Some(entities), Some(view)) = (MOB_AI.get(), ENTITIES.get(), mob.view()) else {
        return;
    };
    let (sin, cos) = f64::from(view.yaw).to_radians().sin_cos();
    let forward = [-sin, 0.0, cos];
    let body = mob.body();
    let origin = body.map_or(view.position, |b| {
        let width = b.max[0] - b.min[0];
        let height = b.max[1] - b.min[1];
        [
            view.position[0] + forward[0] * width * 0.35,
            view.position[1] + height * 0.52,
            view.position[2] + forward[2] * width * 0.35,
        ]
    });
    let end = [
        origin[0] + forward[0] * 9.8,
        origin[1],
        origin[2] + forward[2] * 9.8,
    ];
    let targets = if attack == Attack::Tail {
        entities.entities_in(
            world,
            [
                origin[0].min(end[0]) - 4.0,
                origin[1] - 7.0,
                origin[2].min(end[2]) - 4.0,
            ],
            [
                origin[0].max(end[0]) + 4.0,
                origin[1] + 7.0,
                origin[2].max(end[2]) + 4.0,
            ],
            mob.id(),
        )
    } else {
        vec![primary]
    };
    for id in targets {
        entities.with_entity(world, id, |e| {
            if !enemy(mob, e) {
                return;
            }
            let Some(other) = e.view() else { return };
            let d = flight::offset(other.position, view.position);
            let distance = dist(other.position, view.position);
            if attack == Attack::Tail
                && !e.body().is_some_and(|b| {
                    let toward =
                        std::array::from_fn(|i| origin[i].clamp(b.min[i], b.max[i]) - origin[i]);
                    let gap = flight::length_squared(toward).sqrt();
                    gap <= 3.92
                        || (gap <= 13.8 && toward[0] * forward[0] + toward[2] * forward[2] > 0.0)
                })
            {
                return;
            }
            let mut damage = attack.damage(phase_two) * hunger::melee_multiplier(mob);
            if attack == Attack::Horn
                && let Some(value) = mob
                    .attribute_value("minecraft:attack_damage")
                    .filter(|v| *v > 0.0)
            {
                damage *= value as f32 / 10.0;
            }
            if matches!(attack, Attack::Horn | Attack::Tail)
                && let (Some(armor), Some(toughness)) = (
                    e.attribute_value("minecraft:armor"),
                    e.attribute_value("minecraft:armor_toughness"),
                )
            {
                damage =
                    penetrating_damage(attack, phase_two, damage, armor as f32, toughness as f32);
            }
            ai.attack(world, mob, id, damage);
            let motion = match attack {
                Attack::Bite | Attack::Bite2 => {
                    let (strength, up) = if attack == Attack::Bite2 {
                        (0.35, if wet { 0.2 } else { 0.08 })
                    } else {
                        (0.3, if wet { 0.15 } else { 0.05 })
                    };
                    [
                        other.motion[0] + forward[0] * strength,
                        other.motion[1] + up,
                        other.motion[2] + forward[2] * strength,
                    ]
                }
                Attack::Horn => {
                    let current = e.view().unwrap_or(other);
                    horn_motion(
                        current.motion,
                        forward,
                        current.on_ground,
                        e.attribute_value("minecraft:knockback_resistance")
                            .unwrap_or(0.0),
                        phase_two,
                    )
                }
                Attack::Tail => [
                    other.motion[0] + d[0] / distance.max(0.01) * 1.4,
                    other.motion[1] + 0.2,
                    other.motion[2] + d[2] / distance.max(0.01) * 1.4,
                ],
                Attack::Barrage => [forward[0] * 0.025, 0.0, forward[2] * 0.025],
                _ => return,
            };
            e.change(&EntityChange {
                mask: SET_MOTION,
                motion,
                ..EntityChange::default()
            });
        });
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn horn_pierces_armor_and_tail_deals_flat_damage() {
        for (armor, toughness) in [(0.0, 0.0), (20.0, 8.0), (30.0, 12.0)] {
            for (phase_two, pierce) in [(false, 3.0), (true, 5.0)] {
                let damage = Attack::Horn.damage(phase_two);
                let raw = penetrating_damage(Attack::Horn, phase_two, damage, armor, toughness);
                let expected = damage_after_armor(damage, (armor - pierce).max(0.0), toughness);
                assert!((damage_after_armor(raw, armor, toughness) - expected).abs() < 0.001);
            }
            let raw = penetrating_damage(Attack::Tail, false, 7.0, armor, toughness);
            assert!((damage_after_armor(raw, armor, toughness) - 7.0).abs() < 0.03);
        }
    }
    #[test]
    fn horn_resistance_does_not_cancel_vertical_lift() {
        assert_eq!(
            horn_motion([0.4, 0.0, 0.2], [0.0, 0.0, 1.0], true, 1.0, false),
            [0.4, 0.5, 0.2]
        );
        assert_eq!(
            horn_motion([0.4, 0.0, 0.2], [0.0, 0.0, 1.0], true, 0.0, true),
            [0.2, 0.9, 3.6]
        );
    }
    #[test]
    fn attack_priority_and_phase() {
        let ready = |_| true;
        assert_eq!(
            choose(false, 3.5, false, false, ready, 0, false),
            Some(Attack::Horn)
        );
        assert_eq!(
            choose(false, 4.0, false, false, ready, 34, false),
            Some(Attack::Tail)
        );
        assert_eq!(
            choose(false, 4.0, false, true, ready, 34, false),
            Some(Attack::Bite)
        );
        assert_eq!(
            choose(true, 4.0, false, false, ready, 29, false),
            Some(Attack::Barrage)
        );
        assert_eq!(
            choose(true, 4.0, false, false, ready, 30, false),
            Some(Attack::Claw)
        );
        assert_eq!(choose(true, 1.5, true, false, ready, 0, false), None);
        assert_eq!(
            choose(true, 1.45, true, false, ready, 0, false),
            Some(Attack::Bite2)
        );
    }
    #[test]
    fn released_tracks() {
        assert_eq!((0..93).filter(|&t| Attack::Barrage.hit(t)).count(), 14);
        assert!(Attack::Tail.hit(12));
        assert_eq!(Attack::Shift.duration(true), 33);
        assert_eq!(Attack::Shift.duration(false), 21);
        assert_eq!(Attack::Horn.damage(true), 15.8);
    }
}
