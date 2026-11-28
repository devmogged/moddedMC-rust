//! Volitans combat from Saint's Dragons 0.9.88.
use crate::{
    CONTENT, EFFECTS, ENTITIES, MOB_AI, MOBS, TAGS,
    abilities::enemy,
    combat_raevyx::learning,
    flight::{add, cell, length_squared, offset},
};
use moddedmc_api::{
    content::{BlockUpdate, UPDATE_ALL, World},
    effects::SOUND_NEUTRAL,
    entities::{
        ENTITY_PLAYER, Entity, EntityChange, EntityView, SET_BODY_YAW, SET_HEAD_YAW, SET_MOTION,
        SET_YAW,
    },
    mob_ai::goal_phase,
    mobs::{PLAYER_INPUT_JUMP, PLAYER_INPUT_SPRINT},
    player_actions::{ACTION_PRESSED, ACTION_RELEASED},
    tick_count,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    sync::{Mutex, PoisonError},
    time::{Duration, Instant},
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
    Ultimate,
    Flex,
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
            Self::Ultimate => "ultimate",
            Self::Flex => "flex",
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
    rider_ready: [i64; 9],
    rider_gore: bool,
    depleted: bool,
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
            rider_ready: [0; 9],
            rider_gore: false,
            depleted: false,
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
    rider: bool,
    released: bool,
    learning_firing: u32,
    learning_blocked: u32,
    learning_outcome: learning::Outcome,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum ShotKind {
    Water,
    Poison,
    Orb(i32),
    Spine,
    DashSpine,
}
struct Shot {
    kind: ShotKind,
    pos: [f64; 3],
    velocity: [f64; 3],
    age: u32,
    display: Option<i32>,
    learning_token: u64,
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
    rider: RiderRuntime,
}
#[derive(Default)]
struct RiderRuntime {
    id: Option<i32>,
    primary: Option<Instant>,
    tertiary: Option<u64>,
    action: Option<Action>,
    ascend: bool,
    descend: bool,
    accelerate: bool,
    takeoff_block: u64,
    dash: Option<Dash>,
    dash_ready: u64,
}
struct Dash {
    direction: usize,
    start: u64,
    velocity: [f64; 3],
}
const DASH_CLIPS: [&str; 4] = [
    "dash_forward",
    "dash_backwards",
    "dodge_left",
    "dodge_right",
];
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
        rt.rider
            .action
            .as_ref()
            .or(rt.action.as_ref())
            .is_some_and(|a| {
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
        if !enemy(world, mob, e) {
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
    if let Some(id) = crate::senses::owner_attack(world, mob, valid) {
        ai.set_target(world, mob, Some(id));
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
        Attack::Ultimate | Attack::Flex => return,
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
        rider: false,
        released: false,
        learning_firing: 0,
        learning_blocked: 0,
        learning_outcome: learning::Outcome::Cancelled,
    });
    if kind == Attack::Breath {
        learning::begin(mob, v.position, m == Medium::Air);
    }
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
    stop_gap: f64,
) {
    let Some(ai) = MOB_AI.get() else {
        return;
    };
    if gap <= stop_gap {
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
            learning::defer(mob, learning::Outcome::Cancelled, 30);
            if !eligible(mob) {
                learning::clear(mob);
            } else {
                learning::observe(mob, None, false);
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
    let observation = target
        .and_then(|(id, _)| {
            ENTITIES
                .get()?
                .with_entity(world, id, learning::Observation::from_entity)
                .flatten()
        })
        .filter(|o| clear_line(world, aim(mob, view), o.center));
    let expectation = learning::observe(mob, observation, m == Medium::Air);
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
        } else if action.kind == Attack::Breath {
            learning::defer(mob, action.learning_outcome, 30);
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
        let stop_gap = if m == Medium::Ground
            && !prey
            && visible
            && now(world) >= s.ready[Attack::Breath as usize]
            && energy(&s, now(world)) >= 0.25
        {
            18.0 + expectation.spacing() * 0.5
        } else {
            3.5
        };
        chase(world, mob, rt, view, v.position, m, gap, false, stop_gap);
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
    if !a.rider
        && (a.kind != Attack::Burrow || age < 14 || a.exit.is_some())
        && let Some(ai) = MOB_AI.get()
    {
        ai.stop(mob);
    }
    if a.kind.melee()
        && age == if a.kind == Attack::Claw { 3 } else { 5 }
        && let Some(e) = ENTITIES.get()
    {
        e.with_entity(world, a.target, |t| {
            if !enemy(world, mob, t) {
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
                launch_orb(world, mob, rt, origin, velocity);
            }
        }
        Attack::Breath => {
            if !a.rider {
                if target.is_none_or(|(id, _)| id != a.target) {
                    a.learning_outcome = learning::Outcome::Cancelled;
                    return false;
                }
                if learning::predict(mob, 0.0, 2.0).is_none() {
                    a.learning_blocked += 1;
                } else {
                    a.learning_blocked = 0;
                }
                if a.learning_blocked >= 12 {
                    a.learning_outcome = learning::Outcome::Blocked;
                    let mut s = saved(mob);
                    s.ready[a.kind as usize] = now(world) + 40 + crate::ai::random(21) as i64;
                    s.energy_tick = now(world);
                    save(mob, &s);
                    sound(world, mob, "breath_end", 2.0);
                    return false;
                }
            }
            if age >= 17 {
                let time = now(world);
                let mut s = saved(mob);
                if gap < 4.5 || s.energy < 1.0 / 240.0 || target.is_none() || age >= a.duration {
                    a.learning_outcome = if a.learning_firing >= 10 {
                        learning::Outcome::Completed
                    } else {
                        learning::Outcome::ResponseOnly
                    };
                    s.ready[a.kind as usize] = time + 40 + crate::ai::random(21) as i64;
                    s.energy_tick = time;
                    save(mob, &s);
                    sound(world, mob, "breath_end", 2.0);
                    return false;
                }
                if age == 17 {
                    learning::release(mob);
                    sound(world, mob, "breathing", 2.0);
                }
                s.energy = (s.energy - 1.0 / 240.0).max(0.0);
                s.energy_tick = time;
                save(mob, &s);
                if let Some(center) = learning::predict(mob, 1.0, 2.0) {
                    a.learning_firing += 1;
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
                        display: None,
                        learning_token: learning::token(mob),
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
                let reached = if a.rider {
                    a.released || age >= 314
                } else {
                    gap <= 4.75 && m == Medium::Ground
                };
                let aborted = m != Medium::Ground
                    || (!a.rider
                        && (target.is_none()
                            || age >= 314
                            || tick_count() - a.burrow_progress >= 50));
                if reached || aborted {
                    a.exit = Some((tick_count(), reached && !aborted));
                    sound(world, mob, "burrow_exit", 2.0);
                } else if a.rider {
                    if age.is_multiple_of(6) && length_squared(v.motion) > 0.015 {
                        particle(world, "minecraft:cloud", v.position, 0.8, 8);
                    }
                } else if let Some((_, (tv, _, _, _))) = target {
                    chase(world, mob, rt, v, tv.position, m, gap, true, 3.5);
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
fn launch_orb(
    world: World<'_>,
    mob: Entity<'_>,
    rt: &mut Runtime,
    origin: [f64; 3],
    velocity: [f64; 3],
) {
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
            display: None,
            learning_token: 0,
        });
    }
}

fn rider_cooldown(kind: Attack) -> i64 {
    match kind {
        Attack::Bite => 10,
        Attack::Claw => 8,
        Attack::Gore => 12,
        Attack::Breath | Attack::Orb => 20,
        Attack::Roar => 30,
        Attack::Burrow => 50,
        Attack::Ultimate => 40,
        Attack::Flex => 65,
    }
}

fn primary_held(press: Instant, now: Instant) -> bool {
    now.saturating_duration_since(press) >= Duration::from_millis(180)
}

fn tertiary_held(press: u64, now: u64) -> bool {
    now.saturating_sub(press) >= 5
}

fn controlling(mob: Entity<'_>, player: Entity<'_>) -> bool {
    player.vehicle() == Some(mob.id())
        && MOBS
            .get()
            .is_some_and(|m| m.passengers(mob).first() == Some(&player.id()))
        && crate::taming::is_owner(mob, player)
        && !crate::eggs::is_juvenile(mob)
}

fn reset_rider(rt: &mut Runtime, id: i32) {
    if rt.rider.id != Some(id) {
        rt.rider = RiderRuntime {
            id: Some(id),
            ..Default::default()
        };
    }
}

/// Volitans.getPrimaryRiderDualAbility/getTertiaryRiderDualAbility.
pub fn rider_input(world: World<'_>, mob: Entity<'_>, player: Entity<'_>, action: &str, phase: u8) {
    if !controlling(mob, player) {
        return;
    }
    let Some(view) = mob.view() else {
        return;
    };
    let mut rt = with_runtime(mob, std::mem::take);
    reset_rider(&mut rt, player.id());
    let pressed = phase == ACTION_PRESSED;
    let released = phase == ACTION_RELEASED;
    let active = rt.rider.action.as_ref().map(|a| a.kind);
    let locked = rt
        .rider
        .action
        .as_ref()
        .is_some_and(|a| rider_locked(a, tick_count()));
    match action {
        action if pressed && action.starts_with("moddedmc:double_tap/") => {
            if let Some(i) = ["forward", "back", "left", "right"]
                .iter()
                .position(|d| action.strip_prefix("moddedmc:double_tap/") == Some(*d))
            {
                start_dash(world, mob, &mut rt, view, i);
            }
        }
        "saintsdragons:ascend" => rt.rider.ascend = !released,
        "saintsdragons:descend" => rt.rider.descend = !released,
        "saintsdragons:accelerate" => rt.rider.accelerate = !released,
        "saintsdragons:ability_primary" if pressed && !locked => {
            rt.rider.primary = Some(Instant::now());
        }
        "saintsdragons:ability_primary" if released => {
            if active == Some(Attack::Orb) {
                if let Some(a) = rt.rider.action.as_mut() {
                    a.released = true;
                }
            } else if let Some(press) = rt.rider.primary.take() {
                let held = primary_held(press, Instant::now());
                start_rider(
                    world,
                    mob,
                    &mut rt,
                    if held { Attack::Orb } else { Attack::Roar },
                    view,
                );
                if held && let Some(a) = rt.rider.action.as_mut().filter(|a| a.kind == Attack::Orb)
                {
                    a.released = true;
                }
            }
            rt.rider.primary = None;
        }
        "saintsdragons:ability_tertiary" if pressed && !locked => {
            rt.rider.tertiary = Some(tick_count());
        }
        "saintsdragons:ability_tertiary" if released => {
            if active == Some(Attack::Breath) {
                finish_rider(world, mob, &mut rt);
            } else if let Some(press) = rt.rider.tertiary.take()
                && !tertiary_held(press, tick_count())
            {
                start_rider(world, mob, &mut rt, Attack::Claw, view);
            }
            rt.rider.tertiary = None;
        }
        "saintsdragons:ability_secondary" if pressed => {
            if let Some(a) = rt
                .rider
                .action
                .as_mut()
                .filter(|a| a.kind == Attack::Burrow && a.exit.is_none())
            {
                a.released = true;
            } else if !locked {
                let kind = if medium(world, view) == Medium::Ground {
                    Attack::Burrow
                } else {
                    Attack::Ultimate
                };
                start_rider(world, mob, &mut rt, kind, view);
            }
        }
        "saintsdragons:toggle_melee" if pressed && !locked && active != Some(Attack::Burrow) => {
            if let Some(a) = rt
                .rider
                .action
                .as_mut()
                .filter(|a| a.kind == Attack::Breath)
            {
                a.poison = !a.poison;
                if let Some(m) = MOBS.get() {
                    m.message(
                        player,
                        if a.poison {
                            "Volitans breath mode: Poison"
                        } else {
                            "Volitans breath mode: Water"
                        },
                        true,
                    );
                }
            } else {
                let mut s = saved(mob);
                s.rider_gore = !s.rider_gore;
                save(mob, &s);
            }
        }
        "saintsdragons:flex" if pressed && !locked => {
            start_rider(world, mob, &mut rt, Attack::Flex, view)
        }
        "moddedmc:mounted_attack" if pressed && !locked => {
            let kind = if saved(mob).rider_gore {
                Attack::Gore
            } else {
                Attack::Bite
            };
            start_rider(world, mob, &mut rt, kind, view);
        }
        _ => {}
    }
    with_runtime(mob, |s| *s = rt);
}

fn rider_locked(a: &Action, tick: u64) -> bool {
    match a.kind {
        Attack::Roar => a.medium == Medium::Ground,
        Attack::Ultimate | Attack::Flex => true,
        Attack::Burrow => tick.saturating_sub(a.start) < 14 || a.exit.is_some(),
        _ => false,
    }
}

fn start_rider(world: World<'_>, mob: Entity<'_>, rt: &mut Runtime, kind: Attack, v: EntityView) {
    if rt.rider.action.is_some() || rt.rider.dash.is_some() {
        return;
    }
    let m = medium(world, v);
    let time = now(world);
    let mut s = saved(mob);
    if time < s.rider_ready[kind as usize] {
        return;
    }
    if kind == Attack::Breath {
        s.energy = energy(&s, time);
        s.energy_tick = time;
        if s.energy >= 0.20 {
            s.depleted = false;
        }
        if s.depleted || s.energy <= 0.01 {
            return;
        }
    }
    if matches!(kind, Attack::Burrow | Attack::Flex) && m != Medium::Ground
        || kind == Attack::Ultimate && m != Medium::Air
    {
        return;
    }
    let (duration, event, volume) = match kind {
        Attack::Bite => (14, "bite", 1.4),
        Attack::Claw => (10, "claws", 1.9),
        Attack::Gore => (15, "horn_gore", 1.4),
        Attack::Breath => (257, "breath_start", 2.0),
        Attack::Orb => (u64::MAX, "poison_ball_ready", 2.0),
        Attack::Roar => (
            if m == Medium::Ground { 64 } else { 33 },
            if m == Medium::Ground {
                "roar"
            } else {
                "roar_air_water"
            },
            1.6,
        ),
        Attack::Burrow => (314, "enter_burrow", 2.0),
        Attack::Ultimate => (12025, "slamming", 1.6),
        Attack::Flex => (65, "roar", 1.6),
    };
    s.last = kind.name().into();
    if kind == Attack::Flex {
        s.rider_ready[kind as usize] = time + rider_cooldown(kind);
    }
    save(mob, &s);
    rt.rider.action = Some(Action {
        kind,
        target: 0,
        start: tick_count(),
        duration,
        medium: m,
        poison: false,
        hit: HashSet::new(),
        exit: None,
        burrow_origin: v.position,
        burrow_progress: tick_count(),
        rider: true,
        released: false,
        learning_firing: 0,
        learning_blocked: 0,
        learning_outcome: learning::Outcome::Cancelled,
    });
    sound(world, mob, event, volume);
}

fn finish_rider(world: World<'_>, mob: Entity<'_>, rt: &mut Runtime) {
    if let Some(a) = rt.rider.action.take() {
        let mut s = saved(mob);
        if a.kind != Attack::Flex {
            s.rider_ready[a.kind as usize] = now(world) + rider_cooldown(a.kind);
        }
        if a.kind == Attack::Breath {
            if tick_count().saturating_sub(a.start) < 17 {
                s.energy = energy(&s, now(world));
            }
            s.energy_tick = now(world);
            sound(world, mob, "breath_end", 2.0);
        }
        let block = match a.kind {
            Attack::Orb => 16,
            Attack::Roar if a.medium == Medium::Ground => 10,
            Attack::Burrow | Attack::Ultimate => 8,
            _ => 0,
        };
        rt.rider.takeoff_block = rt.rider.takeoff_block.max(tick_count() + block);
        save(mob, &s);
    }
}

fn look_dir(yaw: f32, pitch: f32) -> [f64; 3] {
    let (sy, cy) = f64::from(yaw).to_radians().sin_cos();
    let (sp, cp) = f64::from(pitch).to_radians().sin_cos();
    [-sy * cp, -sp, cy * cp]
}

fn dash_speed(distance: f64, ticks: i32, drag: f64) -> f64 {
    distance / (0..ticks).map(|i| drag.powi(i)).sum::<f64>()
}

fn start_dash(world: World<'_>, mob: Entity<'_>, rt: &mut Runtime, v: EntityView, i: usize) {
    let tick = tick_count();
    if medium(world, v) != Medium::Ground
        || tick < rt.rider.dash_ready
        || rt.rider.action.is_some()
        || rt.rider.dash.is_some()
    {
        return;
    }
    let (angle, speed) = match i {
        0 => (0.0, dash_speed(26.0, 25, 0.90)),
        1 => (180.0, dash_speed(12.0, 8, 0.90)),
        2 => (-90.0, 1.0),
        _ => (90.0, 1.0),
    };
    rt.rider.dash = Some(Dash {
        direction: i,
        start: tick,
        velocity: look_dir(v.body_yaw + angle, 0.0).map(|x| x * speed),
    });
    rt.rider.dash_ready = tick + 30;
    sound(world, mob, if i < 2 { DASH_CLIPS[i] } else { "dodge" }, 1.6);
}
fn update_dash(world: World<'_>, mob: Entity<'_>, rt: &mut Runtime, v: EntityView) -> bool {
    let tick = tick_count();
    let ground = medium(world, v) == Medium::Ground;
    let Some(dash) = rt.rider.dash.as_mut() else {
        return false;
    };
    let age = tick.saturating_sub(dash.start);
    let duration = match dash.direction {
        0 => 25,
        1 => 13,
        _ => 12,
    };
    if age >= duration || !ground {
        rt.rider.dash = None;
        return false;
    }
    if dash.direction == 0 && age == 14 {
        let width = mob.body().map_or(2.0, |b| b.max[0] - b.min[0]);
        let center = add(
            v.position,
            look_dir(v.body_yaw, 0.0).map(|x| x * width * 0.75),
        );
        for_near(world, mob, center, 8.0, false, |t| hit(world, mob, t, 16.0));
    } else if dash.direction == 1 && age == 8 {
        for angle in [-12.0, 0.0, 12.0] {
            rt.shots.push(Shot {
                kind: ShotKind::DashSpine,
                pos: aim(mob, v),
                velocity: look_dir(v.body_yaw + angle, 0.0).map(|x| x * 2.9),
                age: 0,
                display: None,
                learning_token: 0,
            });
        }
    }
    motion(
        mob,
        [dash.velocity[0], v.motion[1].min(0.0), dash.velocity[2]],
    );
    let drag = match dash.direction {
        0 => 0.90,
        1 if age < 8 => 0.90,
        2 | 3 if age < 7 => 1.0,
        _ => 0.82,
    };
    dash.velocity = dash.velocity.map(|x| x * drag);
    true
}

/// DragonMeleeGeometry.bodyForwardAttack and sweep.
fn melee_sweep(v: EntityView, width: f64, height: f64, range: f64) -> ([f64; 3], [f64; 3]) {
    let forward = look_dir(v.body_yaw, 0.0);
    let origin = add(
        add(v.position, [0.0, height * 0.52, 0.0]),
        forward.map(|x| x * (width * 0.35 + 2.0)),
    );
    let end = add(origin, forward.map(|x| x * range));
    (
        std::array::from_fn(|i| origin[i].min(end[i]) - range),
        std::array::from_fn(|i| origin[i].max(end[i]) + range),
    )
}

fn rider_melee(world: World<'_>, mob: Entity<'_>, a: &mut Action, v: EntityView, age: u64) {
    let first = if a.kind == Attack::Claw { 3 } else { 5 };
    if age != first && !(a.kind == Attack::Gore && age == first + 1) {
        return;
    }
    let (width, height) = mob
        .body()
        .map_or((2.0, 2.0), |b| (b.max[0] - b.min[0], b.max[1] - b.min[1]));
    let range = if a.kind == Attack::Claw { 3.5 } else { 4.5 };
    let (min, max) = melee_sweep(v, width, height, range);
    let Some(entities) = ENTITIES.get() else {
        return;
    };
    for id in entities.entities_in(world, min, max, mob.id()) {
        entities.with_entity(world, id, |t| {
            if !enemy(world, mob, t) || !a.hit.insert(id) {
                return;
            }
            let damage = match a.kind {
                Attack::Bite => 12.0,
                Attack::Claw => 11.0,
                _ => 15.0,
            };
            hit(world, mob, t, damage * crate::hunger::melee_multiplier(mob));
            if let Some(tv) = t.view().filter(|_| a.kind != Attack::Claw) {
                let f = look_dir(v.yaw, v.pitch);
                let push = if a.kind == Attack::Gore { 1.4 } else { 0.3 };
                motion(
                    t,
                    [
                        tv.motion[0] + f[0] * push,
                        if a.kind == Attack::Gore {
                            tv.motion[1].max(0.35)
                        } else {
                            tv.motion[1] + 0.1
                        },
                        tv.motion[2] + f[2] * push,
                    ],
                );
            }
        });
    }
}

/// Tick the rider-controlled sections, sharing autonomous roar, burrow and projectile effects.
fn run_rider_action(
    world: World<'_>,
    mob: Entity<'_>,
    rt: &mut Runtime,
    a: &mut Action,
    v: EntityView,
    look: EntityView,
) -> bool {
    let age = tick_count().saturating_sub(a.start);
    match a.kind {
        Attack::Bite | Attack::Claw | Attack::Gore => rider_melee(world, mob, a, v, age),
        Attack::Orb => {
            if a.released && age >= 13 && a.duration == u64::MAX {
                a.duration = age + 8;
                sound(world, mob, "poison_ball_shoot", 2.0);
            }
            if age >= a.duration {
                launch_orb(
                    world,
                    mob,
                    rt,
                    aim(mob, v),
                    look_dir(look.yaw, look.pitch).map(|x| x * 3.5),
                );
                return false;
            }
        }
        Attack::Breath => {
            if age >= 17 {
                let mut s = saved(mob);
                if age == 17 {
                    s.energy = energy(&s, now(world));
                }
                s.energy = (s.energy - 1.0 / 240.0).max(0.0);
                s.energy_tick = now(world);
                if s.energy <= 0.01 {
                    s.depleted = true;
                }
                save(mob, &s);
                if s.depleted || age >= a.duration {
                    return false;
                }
                if age == 17 {
                    sound(world, mob, "breathing", 2.0);
                }
                if tick_count().is_multiple_of(2) {
                    let direction = look_dir(look.yaw, look.pitch);
                    rt.shots.push(Shot {
                        kind: if a.poison {
                            ShotKind::Poison
                        } else {
                            ShotKind::Water
                        },
                        pos: aim(mob, v),
                        velocity: direction.map(|x| x * 1.6),
                        age: 0,
                        display: None,
                        learning_token: 0,
                    });
                }
            }
        }
        Attack::Roar | Attack::Burrow => {
            return run_action(world, mob, rt, a, v, None, f64::INFINITY, medium(world, v));
        }
        Attack::Ultimate => {
            if let Some((impact, _)) = a.exit {
                return tick_count().saturating_sub(impact) < 20;
            }
            if age < 25 {
                motion(mob, [v.motion[0] * 0.78, 0.0, v.motion[2] * 0.78]);
            } else if v.on_ground {
                a.exit = Some((tick_count(), false));
                motion(mob, [0.0; 3]);
                sound(world, mob, "slammed", 1.9);
                particle(world, "minecraft:cloud", v.position, 3.0, 120);
                for_near(world, mob, v.position, 20.0, true, |t| {
                    if !poison_immune(t) {
                        hit(world, mob, t, 24.0);
                        t.add_effect("minecraft:poison", 600, 1, false, true);
                        stun(t, 40);
                        if let Some(tv) = t.view() {
                            let d = unit(offset(tv.position, v.position));
                            motion(t, add(tv.motion, [d[0], 0.45, d[2]]));
                        }
                    }
                });
                for wave in 0..3 {
                    spine_ring(
                        rt,
                        mob,
                        v,
                        10 + wave * 2,
                        u64::from(wave),
                        1.2 + f64::from(wave) * 0.25,
                        0.14 + f64::from(wave) * 0.07,
                        1.0 + f64::from(wave) * 0.35,
                    );
                }
            } else {
                motion(
                    mob,
                    [v.motion[0], -2.5 - 0.15 * (age - 25) as f64, v.motion[2]],
                );
            }
        }
        Attack::Flex => {}
    }
    age < a.duration
}

/// Called by the existing mounted movement goal. Does not compete with its motion writes.
pub fn ride(
    world: World<'_>,
    mob: Entity<'_>,
    player: Entity<'_>,
    v: EntityView,
    look: EntityView,
    input: [f64; 3],
    keys: i32,
) {
    let mut rt = with_runtime(mob, std::mem::take);
    reset_rider(&mut rt, player.id());
    if rt
        .rider
        .primary
        .is_some_and(|p| primary_held(p, Instant::now()))
    {
        rt.rider.primary = None;
        start_rider(world, mob, &mut rt, Attack::Orb, v);
    }
    if rt
        .rider
        .tertiary
        .is_some_and(|p| tertiary_held(p, tick_count()))
    {
        rt.rider.tertiary = None;
        start_rider(world, mob, &mut rt, Attack::Breath, v);
    }
    if let Some(mut a) = rt.rider.action.take() {
        let ongoing = run_rider_action(world, mob, &mut rt, &mut a, v, look);
        rt.rider.action = Some(a);
        if !ongoing {
            finish_rider(world, mob, &mut rt);
        }
    }
    let locked = rt
        .rider
        .action
        .as_ref()
        .is_some_and(|a| rider_locked(a, tick_count()));
    let ultimate = rt
        .rider
        .action
        .as_ref()
        .is_some_and(|a| a.kind == Attack::Ultimate);
    if update_dash(world, mob, &mut rt, v) {
        // Dash owns motion until its recovery completes.
    } else if locked {
        if !ultimate {
            motion(mob, [0.0, v.motion[1].min(0.0), 0.0]);
        }
    } else {
        let active = rt.rider.action.as_ref();
        let m = medium(world, v);
        let burrowing = active.is_some_and(|a| a.kind == Attack::Burrow);
        let takeoff_allowed = !burrowing
            && tick_count() >= rt.rider.takeoff_block
            && !active.is_some_and(|a| a.kind == Attack::Orb && a.released);
        let up = takeoff_allowed && (rt.rider.ascend || keys & PLAYER_INPUT_JUMP != 0);
        let down = !burrowing && rt.rider.descend;
        let sprint = !burrowing && (rt.rider.accelerate || keys & PLAYER_INPUT_SPRINT != 0);
        let yaw = f64::from(look.yaw).to_radians();
        let (sin, cos) = yaw.sin_cos();
        let pitch = if up || down {
            0.0
        } else {
            f64::from(look.pitch).to_radians()
        };
        let forward = input[2];
        let strafe = input[0];
        let speed = if burrowing {
            0.40
        } else if m == Medium::Water {
            1.42 * if sprint { 1.6 } else { 1.0 }
        } else if m == Medium::Air || up {
            0.60 * if sprint { 5.0 } else { 4.0 }
        } else if sprint {
            0.34
        } else {
            0.24
        };
        let f = if m == Medium::Ground {
            1.0
        } else {
            pitch.cos()
        };
        let desired = [
            (-sin * forward * f + cos * strafe * 0.4) * speed,
            -pitch.sin() * forward * speed,
            (cos * forward * f + sin * strafe * 0.4) * speed,
        ];
        let mut velocity = match m {
            Medium::Water => {
                let dir = [desired[0], desired[1] * 0.65, desired[2]];
                let desired = if forward.abs() > 0.01 || strafe.abs() > 0.01 {
                    unit(dir).map(|x| x * speed)
                } else {
                    [0.0, v.motion[1] * 0.9, 0.0]
                };
                let mut next = std::array::from_fn(|i| {
                    (v.motion[i] + (desired[i] - v.motion[i]) * 0.28)
                        * if i == 1 { 0.96 } else { 0.92 }
                });
                if up {
                    next[1] = (next[1] + 0.18).min(0.55);
                } else if down {
                    next[1] = (next[1] - 0.20).max(-0.55);
                }
                next
            }
            Medium::Air => std::array::from_fn(|i| v.motion[i] + (desired[i] - v.motion[i]) * 0.35),
            Medium::Ground => [desired[0], v.motion[1].min(0.0), desired[2]],
        };
        if m != Medium::Water {
            if up {
                velocity[1] = (v.motion[1] + 0.45).min(1.5);
            } else if down && m == Medium::Air {
                velocity[1] = (v.motion[1] - 0.85).max(-1.5);
            }
        }
        mob.change(&EntityChange {
            mask: SET_YAW | SET_BODY_YAW | SET_HEAD_YAW | SET_MOTION,
            yaw: look.yaw,
            body_yaw: look.yaw,
            head_yaw: look.yaw,
            motion: velocity,
            ..Default::default()
        });
    }
    mob.reset_fall_distance();
    player.reset_fall_distance();
    with_runtime(mob, |s| *s = rt);
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
                if enemy(world, mob, t)
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
            display: None,
            learning_token: 0,
        });
    }
}
fn impact_orb(world: World<'_>, mob: Entity<'_>, p: [f64; 3]) {
    particle(world, "minecraft:witch", p, 1.8, 80);
    crate::vfx::burst(
        world,
        p,
        2.5,
        "volitans/poison/poison_explosion/poison_explosion",
        17,
        false,
    );
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
                    if !enemy(world, mob, t)
                        || !t.body().is_some_and(|b| {
                            (0..3).all(|axis| {
                                (b.min[axis] - radius..=b.max[axis] + radius).contains(&p[axis])
                            })
                        })
                    {
                        return;
                    }
                    touched = true;
                    if shot.kind == ShotKind::DashSpine && !poison_immune(t) {
                        hit(world, mob, t, 5.0);
                        t.add_effect("minecraft:poison", 100, 0, false, true);
                    }
                    if breath
                        && breath_hits
                            .get(&id)
                            .is_none_or(|last| tick_count().saturating_sub(*last) >= 2)
                    {
                        breath_hits.insert(id, tick_count());
                        let mut damaged = false;
                        if shot.kind == ShotKind::Poison {
                            if !poison_immune(t) {
                                damaged = MOB_AI
                                    .get()
                                    .is_some_and(|ai| ai.attack(world, mob, t.id(), 1.4));
                                t.add_effect("minecraft:poison", 80, 0, false, true);
                            }
                        } else {
                            damaged = MOB_AI
                                .get()
                                .is_some_and(|ai| ai.attack(world, mob, t.id(), 1.8));
                            if let Some(v) = t.view() {
                                let d = unit(offset(v.position, p));
                                motion(t, add(v.motion, [d[0] * 0.14, 0.04, d[2] * 0.14]));
                            }
                        }
                        if shot.learning_token != 0
                            && let Some(uuid) = MOBS.get().and_then(|m| m.uuid(t))
                        {
                            learning::contact_token(mob, shot.learning_token, uuid, damaged);
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
        ShotKind::Spine | ShotKind::DashSpine => ("minecraft:crit", 40, 0.99),
    };
    particle(world, visual, next, if breath { 0.15 } else { 0.12 }, 2);
    if breath {
        if shot.age % 5 == 1 {
            let element = if shot.kind == ShotKind::Poison {
                crate::vfx::Element::Poison
            } else {
                crate::vfx::Element::Water
            };
            crate::vfx::breath(world, shot.pos, next, element, radius.min(3.0) as f32);
        }
    } else {
        let (model, scale) = match shot.kind {
            ShotKind::Orb(_) => (crate::vfx::POISON_ORB, 0.9),
            _ => (crate::vfx::SPINE, 1.0),
        };
        crate::vfx::projectile(world, &mut shot.display, model, next, shot.velocity, scale);
    }
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
    if matches!(shot.kind, ShotKind::Spine | ShotKind::DashSpine) {
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
    learning::expire(mob);
    // Drop pressed keys and unfinished abilities when the controlling passenger leaves.
    let rider = MOBS.get().and_then(|m| m.passengers(mob).first().copied());
    let stale = with_runtime(mob, |rt| rt.rider.id.is_some() && rt.rider.id != rider);
    if stale {
        let mut rt = with_runtime(mob, std::mem::take);
        finish_rider(world, mob, &mut rt);
        rt.rider = RiderRuntime::default();
        with_runtime(mob, |s| *s = rt);
    }
    let (mut shots, mut hits, clip) = with_runtime(mob, |rt| {
        let clip = rt
            .rider
            .action
            .as_ref()
            .or(rt.action.as_ref())
            .map(|a| match a.kind {
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
                Attack::Ultimate => {
                    if a.exit.is_some() {
                        "slammed"
                    } else {
                        "slamming"
                    }
                }
                Attack::Flex => "roar",
            })
            .or_else(|| rt.rider.dash.as_ref().map(|d| DASH_CLIPS[d.direction]));
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
    fn dual_key_thresholds_match_release() {
        let start = Instant::now();
        assert!(!primary_held(start, start + Duration::from_millis(179)));
        assert!(primary_held(start, start + Duration::from_millis(180)));
        assert!(!tertiary_held(10, 14));
        assert!(tertiary_held(10, 15));
    }
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
