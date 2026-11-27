//! Volitans combat from Saint's Dragons 0.9.88.
use crate::{
    CONTENT, EFFECTS, ENTITIES, MOB_AI, MOBS, TAGS,
    abilities::enemy,
    flight::{add, cell, length_squared, offset},
};
use moddedmc_api::{
    content::{BlockUpdate, UPDATE_ALL, World},
    effects::SOUND_NEUTRAL,
    entities::{
        ENTITY_PLAYER, Entity, EntityChange, EntityView, SET_BODY_YAW, SET_MOTION, SET_YAW,
    },
    mob_ai::goal_phase,
    tick_count,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    sync::{Mutex, PoisonError},
};

pub const TARGET: u64 = 130;
pub const COMBAT: u64 = 131;
const KEY: &str = "saintsdragons:volitans_combat";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Attack {
    Bite,
    Claw,
    Gore,
    Breath,
    Orb,
    Roar,
    Burrow,
}
impl Attack {
    fn name(self) -> &'static str {
        match self {
            Self::Bite => "bite",
            Self::Claw => "claw",
            Self::Gore => "horn_gore",
            Self::Breath => "breath",
            Self::Orb => "poison_ball",
            Self::Roar => "roar",
            Self::Burrow => "burrow",
        }
    }
    fn melee(self) -> bool {
        matches!(self, Self::Bite | Self::Claw | Self::Gore)
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Medium {
    Ground,
    Water,
    Air,
}
#[derive(Serialize, Deserialize)]
#[serde(default)]
struct Saved {
    last: String,
    ready: [i64; 7],
    major: i64,
    cadence: i64,
    energy: f32,
    energy_tick: i64,
    poison_last: bool,
}
impl Default for Saved {
    fn default() -> Self {
        Self {
            last: "none".into(),
            ready: [0; 7],
            major: 0,
            cadence: 0,
            energy: 1.0,
            energy_tick: 0,
            poison_last: false,
        }
    }
}
fn saved(mob: Entity<'_>) -> Saved {
    mob.data(KEY)
        .and_then(|bytes| fastnbt::from_bytes(&bytes).ok())
        .unwrap_or_default()
}
fn save(mob: Entity<'_>, state: &Saved) {
    if let Ok(bytes) = fastnbt::to_bytes(state) {
        mob.set_data(KEY, &bytes);
    }
}
fn now(world: World<'_>) -> i64 {
    world.game_time().unwrap_or(tick_count() as i64)
}
fn energy(s: &Saved, time: i64) -> f32 {
    (s.energy + (time - s.energy_tick).max(0) as f32 * 0.0025).min(1.0)
}

struct Action {
    kind: Attack,
    target: i32,
    start: u64,
    duration: u64,
    medium: Medium,
    poison: bool,
    hit: HashSet<i32>,
    exit: Option<(u64, bool)>,
    burrow_origin: [f64; 3],
    burrow_progress: u64,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum ShotKind {
    Water,
    Poison,
    Orb(i32),
    Spine,
}
struct Shot {
    kind: ShotKind,
    pos: [f64; 3],
    velocity: [f64; 3],
    age: u32,
}
#[derive(Default)]
struct Runtime {
    action: Option<Action>,
    shots: Vec<Shot>,
    breath_hits: HashMap<i32, u64>,
    last_hurt: Option<(i32, i32)>,
    owner_hurt: Option<(i32, i32)>,
    engagement: u64,
    repath: u64,
    next_burrow: u64,
    last_target: Option<i32>,
    opener_used: bool,
}
static RUNTIME: Mutex<Option<HashMap<i32, Runtime>>> = Mutex::new(None);
pub fn forget(id: i32) {
    if let Some(all) = RUNTIME
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_mut()
    {
        all.remove(&id);
    }
}
pub fn exit_invulnerable(mob: Entity<'_>) -> bool {
    with_runtime(mob, |rt| {
        rt.action.as_ref().is_some_and(|a| {
            a.kind == Attack::Burrow
                && a.exit
                    .is_some_and(|(start, _)| tick_count().saturating_sub(start) < 52)
        })
    })
}
fn with_runtime<T>(mob: Entity<'_>, f: impl FnOnce(&mut Runtime) -> T) -> T {
    let mut all = RUNTIME.lock().unwrap_or_else(PoisonError::into_inner);
    f(all.get_or_insert_default().entry(mob.id()).or_default())
}
fn medium(world: World<'_>, view: EntityView) -> Medium {
    if wet(world, view.position) {
        Medium::Water
    } else if view.on_ground {
        Medium::Ground
    } else {
        Medium::Air
    }
}
fn wet(world: World<'_>, p: [f64; 3]) -> bool {
    world
        .fluid_id(cell(p))
        .is_some_and(|id| id.ends_with("water"))
}
fn unit(v: [f64; 3]) -> [f64; 3] {
    let n = length_squared(v).sqrt();
    if n < 1e-8 { [0.0; 3] } else { v.map(|x| x / n) }
}
fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    length_squared(offset(a, b)).sqrt()
}
fn motion(mob: Entity<'_>, v: [f64; 3]) {
    mob.change(&EntityChange {
        mask: SET_MOTION,
        motion: v,
        ..Default::default()
    });
}
fn target_info(
    world: World<'_>,
    mob: Entity<'_>,
    id: i32,
) -> Option<(EntityView, f64, [f64; 3], bool)> {
    ENTITIES.get()?.with_entity(world, id, |e| {
        if !enemy(mob, e) {
            return None;
        }
        let (v, b) = (e.view()?, e.body()?);
        Some((
            v,
            b.max[0] - b.min[0],
            std::array::from_fn(|i| (b.min[i] + b.max[i]) * 0.5),
            prey(e),
        ))
    })?
}
fn prey(e: Entity<'_>) -> bool {
    matches!(
        e.type_id(),
        "minecraft:cod" | "minecraft:pufferfish" | "minecraft:salmon" | "minecraft:tropical_fish"
    )
}
fn poison_immune(e: Entity<'_>) -> bool {
    let kind = MOBS
        .get()
        .and_then(|m| m.kind(e))
        .unwrap_or_else(|| e.type_id());
    kind == "saintsdragons:volitans"
        || TAGS.get().is_some_and(|t| {
            t.contains(
                "minecraft:entity_type",
                "saintsdragons:immune_to/poison",
                kind,
            )
        })
}
fn eligible(mob: Entity<'_>) -> bool {
    !crate::eggs::is_juvenile(mob)
        && crate::taming::order_name(mob) != "stay"
        && mob.vehicle().is_none()
        && MOBS.get().is_some_and(|m| m.passengers(mob).is_empty())
}
/// VolitansTargetingBehaviour: owner defense, retaliation, aggressive wild players, hungry fish hunting.
pub fn targeting(phase: u32, world: World<'_>, mob: Entity<'_>) -> bool {
    if phase != goal_phase::TICK {
        return matches!(phase, goal_phase::CAN_START | goal_phase::CONTINUE);
    }
    let (Some(ai), Some(entities), Some(mobs), Some(view)) =
        (MOB_AI.get(), ENTITIES.get(), MOBS.get(), mob.view())
    else {
        return false;
    };
    if !eligible(mob) {
        ai.set_target(world, mob, None);
        return false;
    }
    let valid = |id| {
        target_info(world, mob, id)
            .is_some_and(|(v, _, _, _)| distance(v.position, view.position) <= 64.0)
    };
    let owner = crate::taming::owner(mob).and_then(|uuid| mobs.by_uuid(world, &uuid));
    if let Some(hurt) = owner.and_then(|id| {
        entities
            .with_entity(world, id, |e| ai.last_hurt_by(e))
            .flatten()
    }) && valid(hurt.0)
        && with_runtime(mob, |s| s.owner_hurt.replace(hurt) != Some(hurt))
    {
        ai.set_target(world, mob, Some(hurt.0));
        return false;
    }
    if let Some(hurt) = ai.last_hurt_by(mob)
        && valid(hurt.0)
        && with_runtime(mob, |s| s.last_hurt.replace(hurt) != Some(hurt))
    {
        ai.set_target(world, mob, Some(hurt.0));
        return false;
    }
    if ai.target(mob).is_some_and(valid) {
        return false;
    }
    ai.set_target(world, mob, None);
    if !(tick_count() + mob.id() as u64).is_multiple_of(10) {
        return false;
    }
    let wild = owner.is_none() && !crate::taming::is_tamed(mob);
    let hungry = crate::hunger::level(mob) < 50;
    if !wild && !hungry {
        return false;
    }
    let mut best = None;
    for id in entities.entities_in(
        world,
        add(view.position, [-64.0; 3]),
        add(view.position, [64.0; 3]),
        mob.id(),
    ) {
        let Some((v, _, center, is_prey)) = target_info(world, mob, id) else {
            continue;
        };
        if !(wild && v.kind & ENTITY_PLAYER != 0 || hungry && is_prey) {
            continue;
        }
        let d = distance(view.position, v.position);
        if d <= 64.0
            && best.is_none_or(|(_, old)| d < old)
            && clear_line(world, add(view.position, [0.0, 1.0, 0.0]), center)
        {
            best = Some((id, d));
        }
    }
    if let Some((id, _)) = best {
        ai.set_target(world, mob, Some(id));
    }
    false
}

fn clear_line(world: World<'_>, a: [f64; 3], b: [f64; 3]) -> bool {
    let delta = offset(b, a);
    let n = (length_squared(delta).sqrt() * 4.0).ceil().max(1.0) as u32;
    (1..=n).all(|i| {
        let p = add(a, delta.map(|x| x * f64::from(i) / f64::from(n)));
        block_clear(world, p)
    })
}
fn block_clear(world: World<'_>, p: [f64; 3]) -> bool {
    let pos = cell(p);
    world.collision_shape(pos).is_some_and(|boxes| {
        let local = p.map(|x| x - x.floor());
        !boxes
            .iter()
            .any(|b| (0..3).all(|i| (b.min[i]..=b.max[i]).contains(&local[i])))
    })
}
fn sound(world: World<'_>, mob: Entity<'_>, event: &str, volume: f32) {
    if let (Some(e), Some(v)) = (EFFECTS.get(), mob.view()) {
        let id = if cfg!(feature = "original-assets") {
            format!("saintsdragons:volitans_{event}")
        } else {
            "minecraft:entity.ender_dragon.growl".into()
        };
        e.play_sound(world, v.position, &id, SOUND_NEUTRAL, volume, 1.0);
    }
}
fn particle(world: World<'_>, id: &str, p: [f64; 3], spread: f64, count: u32) {
    if let Some(e) = EFFECTS.get() {
        e.particles(world, id, p, [spread; 3], 0.02, count);
    }
}

/// AI melee ranges and the ground/water/air pacing from the respective Java behaviours.
fn melee_choice(m: Medium, gap: f64, prey: bool, retreat: bool, roll: u64) -> Option<Attack> {
    if gap > if m == Medium::Air { 6.0 } else { 6.2 } {
        return None;
    }
    if prey {
        return (gap <= 4.1 || m == Medium::Air).then_some(Attack::Bite);
    }
    if m == Medium::Air {
        return Some(if roll < 40 {
            Attack::Bite
        } else if roll < 72 {
            Attack::Claw
        } else {
            Attack::Gore
        });
    }
    if gap <= 4.1 {
        return Some(if m == Medium::Ground && retreat {
            Attack::Gore
        } else if roll < if m == Medium::Water { 42 } else { 50 } {
            Attack::Claw
        } else if m == Medium::Water && roll >= 72 {
            Attack::Gore
        } else {
            Attack::Bite
        });
    }
    Some(if gap <= 5.1 && (m == Medium::Ground || roll < 58) {
        Attack::Claw
    } else {
        Attack::Gore
    })
}
#[allow(clippy::too_many_arguments)]
fn choose(
    s: &Saved,
    rt: &Runtime,
    m: Medium,
    gap: f64,
    visible: bool,
    prey: bool,
    retreat: bool,
    time: i64,
) -> Option<Attack> {
    if time < s.cadence || !visible {
        return None;
    }
    let ready = |a: Attack| time >= s.ready[a as usize];
    if let Some(a) = melee_choice(m, gap, prey, retreat, crate::ai::random(100)) {
        return ready(a).then_some(a);
    }
    if prey || time < s.major {
        return None;
    }
    if m == Medium::Ground
        && (8.0..=40.0).contains(&gap)
        && tick_count() >= rt.next_burrow
        && ready(Attack::Burrow)
        && (retreat || gap >= 16.0)
        && crate::ai::random(100) < if retreat { 70 } else { 30 }
    {
        return Some(Attack::Burrow);
    }
    if (7.0..=37.0).contains(&gap) && ready(Attack::Breath) && energy(s, time) >= 0.25 {
        return Some(Attack::Breath);
    }
    if m == Medium::Ground
        && !rt.opener_used
        && (32..=100).contains(&tick_count().saturating_sub(rt.engagement))
        && (5.0..=14.0).contains(&gap)
        && ready(Attack::Roar)
    {
        return Some(Attack::Roar);
    }
    if m != Medium::Ground && (4.5..=12.0).contains(&gap) && ready(Attack::Roar) {
        return Some(Attack::Roar);
    }
    if m != Medium::Water
        && (if m == Medium::Air {
            12.0..=32.0
        } else {
            8.0..=24.0
        })
        .contains(&gap)
        && ready(Attack::Orb)
    {
        return Some(Attack::Orb);
    }
    None
}
fn start(
    world: World<'_>,
    mob: Entity<'_>,
    rt: &mut Runtime,
    kind: Attack,
    id: i32,
    m: Medium,
    v: EntityView,
) {
    let time = now(world);
    let mut s = saved(mob);
    let (duration, cadence, cooldown, major, event, volume) = match kind {
        Attack::Bite => (14, 30, 30, 0, "bite", 1.4),
        Attack::Claw => (10, 30, 30, 0, "claws", 1.9),
        Attack::Gore => (15, 30, 30, 0, "horn_gore", 1.4),
        Attack::Breath => (57 + crate::ai::random(41), 16, 20, 20, "breath_start", 2.0),
        Attack::Orb => (
            26 + crate::ai::random(8) + if m == Medium::Air { 2 } else { 0 },
            16,
            if m == Medium::Air { 120 } else { 110 },
            90,
            "poison_ball_ready",
            2.0,
        ),
        Attack::Roar => (
            if m == Medium::Ground { 64 } else { 33 },
            24,
            if m == Medium::Air { 140 } else { 200 },
            120,
            if m == Medium::Ground {
                "roar"
            } else {
                "roar_air_water"
            },
            2.0,
        ),
        Attack::Burrow => (
            314,
            14,
            160 + crate::ai::random(61) as i64,
            80,
            "enter_burrow",
            2.0,
        ),
    };
    let poison = kind == Attack::Breath
        && !s.poison_last
        && crate::ai::random(100) < 75
        && ENTITIES.get().is_some_and(|e| {
            e.with_entity(world, id, |e| !poison_immune(e))
                .unwrap_or(false)
        });
    if kind == Attack::Breath {
        s.energy = energy(&s, time);
        s.energy_tick = time;
        s.poison_last = poison;
    }
    if kind == Attack::Roar {
        rt.opener_used = true;
    }
    if kind == Attack::Burrow {
        rt.next_burrow = tick_count() + 20;
    }
    s.last = kind.name().into();
    s.ready[kind as usize] = time + cooldown;
    s.cadence = time + cadence;
    if major > 0 {
        s.major = time + major;
    }
    save(mob, &s);
    rt.action = Some(Action {
        kind,
        target: id,
        start: tick_count(),
        duration,
        medium: m,
        poison,
        hit: HashSet::new(),
        exit: None,
        burrow_origin: v.position,
        burrow_progress: tick_count(),
    });
    sound(world, mob, event, volume);
}
fn aim(mob: Entity<'_>, view: EntityView) -> [f64; 3] {
    let yaw = f64::from(view.yaw).to_radians();
    let height = mob.body().map_or(1.5, |b| (b.max[1] - b.min[1]) * 0.7);
    add(view.position, [-yaw.sin() * 1.8, height, yaw.cos() * 1.8])
}
fn face(mob: Entity<'_>, target: [f64; 3], view: EntityView) {
    let [x, _, z] = offset(target, view.position);
    if x.hypot(z) > 0.001 {
        let yaw = (z.atan2(x).to_degrees() - 90.0) as f32;
        mob.change(&EntityChange {
            mask: SET_YAW | SET_BODY_YAW,
            yaw,
            body_yaw: yaw,
            ..Default::default()
        });
    }
}
#[allow(clippy::too_many_arguments)]
fn chase(
    world: World<'_>,
    mob: Entity<'_>,
    rt: &mut Runtime,
    view: EntityView,
    at: [f64; 3],
    m: Medium,
    gap: f64,
    burrow: bool,
) {
    let Some(ai) = MOB_AI.get() else {
        return;
    };
    if gap <= 3.5 {
        ai.stop(mob);
        return;
    }
    if m == Medium::Ground {
        if tick_count() >= rt.repath || !ai.navigating(mob) {
            rt.repath = tick_count() + 5;
            ai.walk_to(mob, at, if burrow { 1.55 } else { 1.2 });
        }
    } else {
        // shortcut: local collision checked swimming/flight, until species navigation is exposed.
        ai.stop(mob);
        let dir = unit(offset(at, view.position));
        let speed = if m == Medium::Water {
            0.28
        } else if view.position[1] - at[1] > 7.0 {
            0.70
        } else {
            0.46
        };
        let step = dir.map(|x| x * speed);
        if clear_line(world, view.position, add(view.position, step)) {
            motion(mob, step);
        }
        if m == Medium::Air {
            mob.reset_fall_distance();
        }
    }
}
pub fn goal(phase: u32, world: World<'_>, mob: Entity<'_>) -> bool {
    let (Some(ai), Some(view)) = (MOB_AI.get(), mob.view()) else {
        return false;
    };
    let target = ai
        .target(mob)
        .and_then(|id| target_info(world, mob, id).map(|info| (id, info)));
    match phase {
        goal_phase::CAN_START | goal_phase::CONTINUE => {
            eligible(mob) && (target.is_some() || with_runtime(mob, |s| s.action.is_some()))
        }
        goal_phase::STOP => {
            if with_runtime(mob, |s| s.action.take().is_some()) {
                ai.stop(mob);
            }
            false
        }
        goal_phase::TICK => {
            let mut rt = with_runtime(mob, std::mem::take);
            tick_combat(world, mob, &mut rt, view, target);
            with_runtime(mob, |s| *s = rt);
            false
        }
        _ => false,
    }
}
type Target = (i32, (EntityView, f64, [f64; 3], bool));
fn tick_combat(
    world: World<'_>,
    mob: Entity<'_>,
    rt: &mut Runtime,
    view: EntityView,
    target: Option<Target>,
) {
    let Some(ai) = MOB_AI.get() else {
        return;
    };
    let m = medium(world, view);
    let gap = target.map_or(f64::INFINITY, |(_, (v, w, _, _))| {
        let own = mob.body().map_or(2.0, |b| b.max[0] - b.min[0]);
        (distance(view.position, v.position) - (own + w) * 0.5).max(0.0)
    });
    if let Some((id, (v, _, center, _))) = target {
        if rt.last_target != Some(id) {
            rt.last_target = Some(id);
            rt.engagement = tick_count();
            rt.opener_used = false;
        }
        ai.look_at(mob, center);
        face(mob, v.position, view);
    }
    if let Some(mut action) = rt.action.take() {
        if run_action(world, mob, rt, &mut action, view, target, gap, m) {
            rt.action = Some(action);
        }
        return;
    }
    let Some((id, (v, _, center, prey))) = target else {
        return;
    };
    let visible = clear_line(world, aim(mob, view), center);
    let retreat = unit(offset(v.position, view.position))
        .iter()
        .zip(v.motion)
        .map(|(a, b)| a * b)
        .sum::<f64>()
        >= 0.07;
    let s = saved(mob);
    if let Some(a) = choose(&s, rt, m, gap, visible, prey, retreat, now(world)) {
        start(world, mob, rt, a, id, m, view);
        ai.stop(mob);
    } else {
        chase(world, mob, rt, view, v.position, m, gap, false);
    }
}
#[allow(clippy::too_many_arguments)]
fn run_action(
    world: World<'_>,
    mob: Entity<'_>,
    rt: &mut Runtime,
    a: &mut Action,
    v: EntityView,
    target: Option<Target>,
    gap: f64,
    m: Medium,
) -> bool {
    let age = tick_count().saturating_sub(a.start);
    if (a.kind != Attack::Burrow || age < 14 || a.exit.is_some())
        && let Some(ai) = MOB_AI.get()
    {
        ai.stop(mob);
    }
    if a.kind.melee()
        && age == if a.kind == Attack::Claw { 3 } else { 5 }
        && let Some(e) = ENTITIES.get()
    {
        e.with_entity(world, a.target, |t| {
            if !enemy(mob, t) {
                return;
            }
            let damage = match a.kind {
                Attack::Bite => 12.0,
                Attack::Claw => 11.0,
                _ => 15.0,
            };
            hit(world, mob, t, damage * crate::hunger::melee_multiplier(mob));
            if a.kind != Attack::Claw
                && let Some(tv) = t.view()
            {
                let dir = unit(offset(tv.position, v.position));
                let push = if a.kind == Attack::Gore { 1.4 } else { 0.3 };
                motion(
                    t,
                    add(
                        tv.motion,
                        [
                            dir[0] * push,
                            if a.kind == Attack::Gore { 0.35 } else { 0.1 },
                            dir[2] * push,
                        ],
                    ),
                );
            }
        });
    }
    match a.kind {
        Attack::Orb => {
            if age + 8 == a.duration {
                sound(world, mob, "poison_ball_shoot", 2.0);
            }
            if age == a.duration
                && let Some((_, (tv, _, center, _))) = target
            {
                let origin = aim(mob, v);
                let lead = add(center, tv.motion.map(|x| x * 0.6));
                let velocity = unit(offset(lead, origin)).map(|x| x * 3.5);
                // shortcut: the ABI spawns vanilla projectiles; a snowball carries the
                // orb until custom projectile types and collision hooks are available.
                if let Some(id) = ENTITIES.get().and_then(|e| {
                    e.spawn_projectile(
                        world,
                        "minecraft:snowball",
                        mob.id(),
                        None,
                        origin,
                        velocity,
                    )
                }) {
                    rt.shots.push(Shot {
                        kind: ShotKind::Orb(id),
                        pos: origin,
                        velocity,
                        age: 0,
                    });
                }
            }
        }
        Attack::Breath => {
            if age >= 17 {
                let time = now(world);
                let mut s = saved(mob);
                if gap < 4.5 || s.energy < 1.0 / 240.0 || target.is_none() || age >= a.duration {
                    s.ready[a.kind as usize] = time + 40 + crate::ai::random(21) as i64;
                    s.energy_tick = time;
                    save(mob, &s);
                    sound(world, mob, "breath_end", 2.0);
                    return false;
                }
                if age == 17 {
                    sound(world, mob, "breathing", 2.0);
                }
                s.energy = (s.energy - 1.0 / 240.0).max(0.0);
                s.energy_tick = time;
                save(mob, &s);
                if let Some((_, (_, _, center, _))) = target {
                    let origin = aim(mob, v);
                    rt.shots.push(Shot {
                        kind: if a.poison {
                            ShotKind::Poison
                        } else {
                            ShotKind::Water
                        },
                        pos: origin,
                        velocity: unit(offset(center, origin)).map(|x| x * 1.6),
                        age: 0,
                    });
                }
            }
        }
        Attack::Roar => {
            let ground = a.medium == Medium::Ground;
            let begin = if ground { 23 } else { 0 };
            let end = if ground { 63 } else { 33 };
            if (begin..end).contains(&age) {
                for_near(
                    world,
                    mob,
                    v.position,
                    if ground { 20.0 } else { 12.0 },
                    true,
                    |t| {
                        if !poison_immune(t) && a.hit.insert(t.id()) {
                            hit(world, mob, t, if ground { 10.0 } else { 7.0 });
                            t.add_effect(
                                "minecraft:poison",
                                if ground { 1200 } else { 200 },
                                if ground { 2 } else { 1 },
                                false,
                                true,
                            );
                            stun(t, if ground { 40 } else { 20 });
                        }
                    },
                );
                let start = if ground { 23 } else { 5 };
                if age >= start && (age - start).is_multiple_of(6) {
                    let pulse = (age - start) / 6;
                    spine_ring(
                        rt,
                        mob,
                        v,
                        if ground { 10 } else { 5 },
                        pulse,
                        if ground { 1.2 } else { 1.05 },
                        0.22,
                        1.0,
                    );
                    spine_ring(
                        rt,
                        mob,
                        v,
                        if ground { 8 } else { 4 },
                        pulse,
                        if ground { 1.45 } else { 1.25 },
                        0.05,
                        0.85,
                    );
                }
            }
        }
        Attack::Burrow => {
            if age == 13 {
                particle(world, "minecraft:cloud", v.position, 1.2, 15);
            }
            if age >= 14 && a.exit.is_none() {
                if distance(v.position, a.burrow_origin) > 1.5 {
                    a.burrow_origin = v.position;
                    a.burrow_progress = tick_count();
                }
                let reached = gap <= 4.75 && m == Medium::Ground;
                let aborted = m != Medium::Ground
                    || target.is_none()
                    || age >= 314
                    || tick_count() - a.burrow_progress >= 50;
                if reached || aborted {
                    a.exit = Some((tick_count(), reached && !aborted));
                    sound(world, mob, "burrow_exit", 2.0);
                } else if let Some((_, (tv, _, _, _))) = target {
                    chase(world, mob, rt, v, tv.position, m, gap, true);
                    if age.is_multiple_of(6) {
                        particle(world, "minecraft:cloud", v.position, 0.8, 8);
                    }
                }
            }
            if let Some((exit, burst)) = a.exit {
                let elapsed = tick_count() - exit;
                if elapsed == 28 && burst {
                    particle(world, "minecraft:cloud", v.position, 3.0, 120);
                    for_near(world, mob, v.position, 12.0, true, |t| {
                        hit(world, mob, t, 30.0);
                        if let Some(tv) = t.view() {
                            motion(
                                t,
                                [
                                    tv.motion[0] * 0.35,
                                    tv.motion[1].max(1.0),
                                    tv.motion[2] * 0.35,
                                ],
                            );
                        }
                    });
                }
                return elapsed < 52;
            }
            return true;
        }
        _ => {}
    }
    age < a.duration
}
fn hit(world: World<'_>, mob: Entity<'_>, t: Entity<'_>, damage: f32) {
    if let Some(ai) = MOB_AI.get() {
        ai.attack(world, mob, t.id(), damage);
    }
}
fn stun(t: Entity<'_>, ticks: i32) {
    if t.view().is_some_and(|v| v.kind & ENTITY_PLAYER == 0) {
        if let Some(ai) = MOB_AI.get() {
            ai.stop(t);
        }
        motion(t, [0.0; 3]);
        t.add_effect("minecraft:slowness", ticks, 6, false, true);
        t.add_effect("minecraft:weakness", ticks, 1, false, true);
        t.add_effect("minecraft:blindness", ticks.min(20), 0, false, true);
    }
}
fn for_near(
    world: World<'_>,
    mob: Entity<'_>,
    p: [f64; 3],
    radius: f64,
    sphere: bool,
    mut f: impl FnMut(Entity<'_>),
) {
    if let Some(e) = ENTITIES.get() {
        for id in e.entities_in(world, add(p, [-radius; 3]), add(p, [radius; 3]), mob.id()) {
            e.with_entity(world, id, |t| {
                if enemy(mob, t)
                    && (!sphere || t.view().is_some_and(|v| distance(v.position, p) <= radius))
                {
                    f(t);
                }
            });
        }
    }
}
#[allow(clippy::too_many_arguments)]
fn spine_ring(
    rt: &mut Runtime,
    mob: Entity<'_>,
    v: EntityView,
    count: u32,
    pulse: u64,
    speed: f64,
    y: f64,
    scale: f64,
) {
    let (width, height) = mob
        .body()
        .map_or((2.0, 2.0), |b| (b.max[0] - b.min[0], b.max[1] - b.min[1]));
    let r = (width * 0.65).max(1.25) * scale;
    let p = add(v.position, [0.0, height * 0.55, 0.0]);
    for i in 0..count {
        let angle = std::f64::consts::TAU * f64::from(i) / f64::from(count) + pulse as f64 * 0.22;
        let (sin, cos) = angle.sin_cos();
        rt.shots.push(Shot {
            kind: ShotKind::Spine,
            pos: add(p, [cos * r, 0.0, sin * r]),
            velocity: unit([cos, y, sin]).map(|x| x * speed),
            age: 0,
        });
    }
}
fn impact_orb(world: World<'_>, mob: Entity<'_>, p: [f64; 3]) {
    particle(world, "minecraft:witch", p, 1.8, 80);
    for_near(world, mob, p, 5.0, false, |t| {
        if poison_immune(t) {
            return;
        }
        hit(world, mob, t, 12.0);
        t.add_effect("minecraft:poison", 120, 0, false, true);
        if let Some(v) = t.view() {
            let d = unit(offset(v.position, p));
            motion(t, add(v.motion, [d[0] * 0.396, 0.05, d[2] * 0.396]));
        }
    });
}
fn orb_motion(mut velocity: [f64; 3], age: u32) -> [f64; 3] {
    // VolitansPoisonOrbEntity.tick: ramp gravity over the first 20 ticks.
    velocity[1] -= 0.05 * (f64::from(age) / 20.0).min(1.0);
    velocity
}
fn finish_orb(world: World<'_>, mob: Entity<'_>, id: i32, p: [f64; 3]) {
    if let Some(e) = ENTITIES.get() {
        e.with_entity(world, id, |orb| {
            if orb.projectile_owner() == Some(mob.id()) {
                orb.discard();
            }
        });
    }
    impact_orb(world, mob, p);
}
fn water_contact(world: World<'_>, mob: Entity<'_>, p: [f64; 3]) {
    let pos = cell(p);
    let mut states = [0];
    if !world.read(&[pos], &mut states) {
        return;
    }
    let Some(content) = CONTENT.get() else {
        return;
    };
    let Some(name) = content.describe_state(states[0]) else {
        return;
    };
    if world.fluid_id(pos).is_some_and(|s| s.ends_with("lava")) {
        if let Some(state) = content.state("minecraft:cobblestone") {
            world.write(&[BlockUpdate {
                position: pos,
                state,
                flags: UPDATE_ALL,
            }]);
        }
    } else if (name.starts_with("minecraft:fire") || name.starts_with("minecraft:soul_fire"))
        && world.destroy(pos, false)
        && let Some(owner) = crate::taming::owner(mob)
    {
        crate::advancements::award_uuid(world, owner, "fire_hydrant", "fire_hydrant");
    }
}
/// VolitansBreathStream and PoisonOrbEntity swept movement. Spine rings have zero impact damage in Java.
fn step_shot(
    world: World<'_>,
    mob: Entity<'_>,
    shot: &mut Shot,
    breath_hits: &mut HashMap<i32, u64>,
) -> bool {
    shot.age += 1;
    let orb = if let ShotKind::Orb(id) = shot.kind {
        shot.velocity = orb_motion(shot.velocity, shot.age);
        Some(id)
    } else {
        None
    };
    let next = add(shot.pos, shot.velocity);
    let breath = matches!(shot.kind, ShotKind::Water | ShotKind::Poison);
    let radius = if breath {
        0.8 + f64::from(shot.age) * 0.16
    } else {
        0.35
    };
    let n = (length_squared(shot.velocity).sqrt() * 4.0).ceil().max(1.0) as u32;
    let candidates = ENTITIES
        .get()
        .map(|entities| {
            let min = std::array::from_fn(|i| shot.pos[i].min(next[i]) - radius);
            let max = std::array::from_fn(|i| shot.pos[i].max(next[i]) + radius);
            entities.entities_in(world, min, max, mob.id())
        })
        .unwrap_or_default();
    for i in 1..=n {
        let p = add(
            shot.pos,
            shot.velocity.map(|x| x * f64::from(i) / f64::from(n)),
        );
        if shot.kind == ShotKind::Water {
            water_contact(world, mob, p);
        }
        if !block_clear(world, p) {
            if let Some(id) = orb {
                finish_orb(world, mob, id, p);
            }
            return false;
        }
        let mut touched = false;
        if let Some(entities) = ENTITIES.get() {
            for &id in &candidates {
                entities.with_entity(world, id, |t| {
                    if !enemy(mob, t)
                        || !t.body().is_some_and(|b| {
                            (0..3).all(|axis| {
                                (b.min[axis] - radius..=b.max[axis] + radius).contains(&p[axis])
                            })
                        })
                    {
                        return;
                    }
                    touched = true;
                    if breath
                        && breath_hits
                            .get(&id)
                            .is_none_or(|last| tick_count().saturating_sub(*last) >= 2)
                    {
                        breath_hits.insert(id, tick_count());
                        if shot.kind == ShotKind::Poison {
                            if !poison_immune(t) {
                                hit(world, mob, t, 1.4);
                                t.add_effect("minecraft:poison", 80, 0, false, true);
                            }
                        } else {
                            hit(world, mob, t, 1.8);
                            if let Some(v) = t.view() {
                                let d = unit(offset(v.position, p));
                                motion(t, add(v.motion, [d[0] * 0.14, 0.04, d[2] * 0.14]));
                            }
                        }
                    }
                });
            }
        }
        if touched && !breath {
            if let Some(id) = orb {
                finish_orb(world, mob, id, p);
            }
            return false;
        }
    }
    let (visual, lifetime, drag) = match shot.kind {
        ShotKind::Water => ("minecraft:splash", 28, 0.997),
        ShotKind::Poison => ("minecraft:witch", 28, 0.997),
        ShotKind::Orb(_) => ("minecraft:witch", 201, 0.99),
        ShotKind::Spine => ("minecraft:crit", 40, 0.99),
    };
    particle(world, visual, next, if breath { 0.15 } else { 0.12 }, 2);
    if let Some(id) = orb {
        // Keep the real projectile on the release trajectory. If the vanilla
        // carrier was removed before our sweep found a hit, stop tracking it.
        let live = ENTITIES.get().and_then(|e| {
            e.with_entity(world, id, |entity| {
                if entity.projectile_owner() != Some(mob.id()) {
                    return false;
                }
                entity.set_position(shot.pos);
                motion(entity, shot.velocity);
                true
            })
        });
        if live != Some(true) {
            return false;
        }
    }
    shot.pos = next;
    shot.velocity = shot.velocity.map(|x| x * drag);
    if shot.kind == ShotKind::Spine {
        shot.velocity[1] -= 0.05;
    }
    if shot.age >= lifetime {
        if let Some(id) = orb {
            finish_orb(world, mob, id, next);
        }
        return false;
    }
    true
}
pub fn ambient(world: World<'_>, mob: Entity<'_>) -> Option<&'static str> {
    let (mut shots, mut hits, clip) = with_runtime(mob, |rt| {
        let clip = rt.action.as_ref().map(|a| match a.kind {
            Attack::Bite => "bite",
            Attack::Claw => "swipe_left",
            Attack::Gore => "horn_gore",
            Attack::Breath => {
                if tick_count() - a.start < 17 {
                    "breath_start"
                } else {
                    "breathing"
                }
            }
            Attack::Orb => {
                if tick_count() - a.start < 13 {
                    "poison_ball_ready"
                } else if tick_count() - a.start + 8 < a.duration {
                    "poison_ball_hold"
                } else {
                    "poison_ball_shoot"
                }
            }
            Attack::Roar => {
                if a.medium == Medium::Ground {
                    "roar"
                } else {
                    "roar_air_water"
                }
            }
            Attack::Burrow => {
                if a.exit.is_some() {
                    "burrow_exit"
                } else if tick_count() - a.start < 14 {
                    "enter_burrow"
                } else {
                    "burrow_move"
                }
            }
        });
        (
            std::mem::take(&mut rt.shots),
            std::mem::take(&mut rt.breath_hits),
            clip,
        )
    });
    hits.retain(|_, last| tick_count().saturating_sub(*last) < 2);
    shots.retain_mut(|shot| step_shot(world, mob, shot, &mut hits));
    with_runtime(mob, |rt| {
        rt.shots = shots;
        rt.breath_hits = hits;
    });
    clip
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn orb_gravity_ramps_then_caps() {
        assert_eq!(orb_motion([3.5, 0.0, 0.0], 10), [3.5, -0.025, 0.0]);
        assert_eq!(orb_motion([3.5, 0.0, 0.0], 20), [3.5, -0.05, 0.0]);
        assert_eq!(orb_motion([3.5, 0.0, 0.0], 200), [3.5, -0.05, 0.0]);
    }
    #[test]
    fn melee_ranges_and_environment_weights() {
        assert_eq!(
            melee_choice(Medium::Ground, 4.0, false, false, 20),
            Some(Attack::Claw)
        );
        assert_eq!(
            melee_choice(Medium::Ground, 4.0, false, true, 20),
            Some(Attack::Gore)
        );
        assert_eq!(
            melee_choice(Medium::Water, 4.0, false, false, 80),
            Some(Attack::Gore)
        );
        assert_eq!(
            melee_choice(Medium::Ground, 6.2, false, false, 0),
            Some(Attack::Gore)
        );
        assert_eq!(melee_choice(Medium::Ground, 6.21, false, false, 0), None);
        assert_eq!(melee_choice(Medium::Air, 6.1, false, false, 0), None);
        assert_eq!(
            melee_choice(Medium::Water, 4.0, true, false, 80),
            Some(Attack::Bite)
        );
    }
    #[test]
    fn saved_state_round_trip_and_energy() {
        let state = Saved {
            energy: 0.5,
            energy_tick: 100,
            ..Saved::default()
        };
        let decoded: Saved = fastnbt::from_bytes(&fastnbt::to_bytes(&state).unwrap()).unwrap();
        assert_eq!(decoded.ready, [0; 7]);
        assert_eq!(energy(&decoded, 200), 0.75);
        assert_eq!(energy(&decoded, 500), 1.0);
    }
}
