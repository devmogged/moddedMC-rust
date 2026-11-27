//! Atroxiia's 0.9.88 ground and water combat.
use moddedmc_api::{
    Pos, RegistrationError, Registry,
    content::{BlockUpdate, UPDATE_ALL, World},
    effects::SOUND_NEUTRAL,
    entities::{
        Entity, EntityBody, EntityChange, EntityView, SET_BODY_YAW, SET_HEAD_YAW, SET_MOTION,
        SET_YAW,
    },
    mob_ai::{Goal, goal_control, goal_phase},
    tick_count,
};
use serde::{Deserialize, Serialize};

use crate::{CONTENT, EFFECTS, ENTITIES, MOB_AI, MOBS, abilities::enemy, taming};

pub const TARGETING: u64 = 100;
pub const COMBAT: u64 = 101;
const DATA: &str = "saintsdragons:atroxiia_combat";
const KIND: &str = "saintsdragons:atroxiia";
const FOLLOW: f64 = 32.0;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[repr(usize)]
enum Attack {
    Sweep,
    Quake,
    Gungnir,
    Precise,
    Slam,
    Swipe,
    Bite,
}

impl Attack {
    fn pacing(self) -> (i64, i64, i64, i64) {
        // duration, cadence, per-ability cooldown, major cooldown
        match self {
            Self::Sweep => (26, 32, 240, 120),
            Self::Quake => (39, 50, 600, 240),
            Self::Gungnir => (37, 45, 400, 200),
            Self::Precise => (109, 110, 360, 180),
            Self::Slam => (25, 28, 32, 0),
            Self::Swipe => (20, 22, 24, 0),
            Self::Bite => (19, 24, 24, 0),
        }
    }
    fn clip(self, right: bool, elapsed: i64) -> &'static str {
        match self {
            Self::Sweep => "devastating_sweep",
            Self::Quake if elapsed >= 22 => "helheim_quake2",
            Self::Quake => "helheim_quake1",
            Self::Gungnir => "gungnir_stab",
            Self::Precise => "precise_strike",
            Self::Slam if right => "slam_right",
            Self::Slam => "slam_left",
            Self::Swipe if right => "swipe_right",
            Self::Swipe => "swipe_left",
            Self::Bite => "underwater_bite",
        }
    }
    fn cue(self, elapsed: i64) -> &'static str {
        match self {
            Self::Slam => "slam",
            Self::Swipe => "swipe",
            _ => self.clip(false, elapsed),
        }
    }
}

#[derive(Default, Deserialize, Serialize)]
#[serde(default)]
struct State {
    #[serde(skip_serializing_if = "Option::is_none")]
    active: Option<Attack>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last: Option<Attack>,
    started: i64,
    cadence: i64,
    major: i64,
    cooldown: [i64; 7],
    decision: i64,
    kite: i32,
    right: bool,
    yaw: f32,
    last_hurt: i32,
    owner_hurt: i32,
}

fn now() -> i64 {
    tick_count() as i64
}
impl State {
    fn load(mob: Entity<'_>) -> Self {
        let mut s: Self = mob
            .data(DATA)
            .and_then(|v| fastnbt::from_bytes(&v).ok())
            .unwrap_or_default();
        // The host's tick counter starts again at boot; transient abilities must not resume.
        if s.started > now() || s.decision > now() + 6 {
            s = Self {
                last: s.last,
                ..Self::default()
            };
        }
        s
    }
    fn save(&self, mob: Entity<'_>) {
        if let Ok(bytes) = fastnbt::to_bytes(self) {
            mob.set_data(DATA, &bytes);
        }
    }
    fn ready(&self, attack: Attack, time: i64) -> bool {
        time >= self.cadence
            && time >= self.cooldown[attack as usize]
            && (attack.pacing().3 == 0 || time >= self.major)
    }
    fn begin(&mut self, attack: Attack, time: i64, yaw: f32) {
        let (_, cadence, cooldown, major) = attack.pacing();
        self.active = Some(attack);
        self.last = Some(attack);
        self.started = time;
        self.cadence = time + cadence;
        self.cooldown[attack as usize] = time + cooldown;
        if major != 0 {
            self.major = time + major;
        }
        self.right = !self.right;
        self.yaw = yaw;
    }
}

pub fn register(registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
    let ai = MOB_AI.get().ok_or(RegistrationError::InvalidDefinition)?;
    for (key, priority, controls) in [
        (TARGETING, 1, 0),
        (COMBAT, 1, goal_control::MOVE | goal_control::LOOK),
    ] {
        ai.add_goal(
            registry,
            &Goal {
                kind: KIND,
                key,
                priority,
                controls,
            },
        )?;
    }
    Ok(())
}

fn eligible(mob: Entity<'_>) -> bool {
    !crate::eggs::is_juvenile(mob)
        && taming::order_name(mob) != "stay"
        && mob.vehicle().is_none()
        && MOBS.get().is_some_and(|m| {
            m.passengers(mob).is_empty()
                && (taming::is_tamed(mob) || m.health(mob).is_some_and(|(hp, _)| hp > 60.0))
        })
}
fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|i| a[i] - b[i])
}
fn length(v: [f64; 3]) -> f64 {
    v[0].hypot(v[1]).hypot(v[2])
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3).map(|i| a[i] * b[i]).sum()
}
fn forward(yaw: f32) -> [f64; 3] {
    let (s, c) = f64::from(yaw).to_radians().sin_cos();
    [-s, 0.0, c]
}
fn horizontal(v: [f64; 3], fallback: [f64; 3]) -> [f64; 3] {
    let n = v[0].hypot(v[2]);
    if n < 1e-6 {
        fallback
    } else {
        [v[0] / n, 0.0, v[2] / n]
    }
}
fn width(b: EntityBody) -> f64 {
    b.max[0] - b.min[0]
}
fn gap(a: EntityView, b: EntityView, aw: f64, bw: f64) -> f64 {
    (length(sub(a.position, b.position)) - (aw + bw) * 0.5).max(0.0)
}
fn wet(world: World<'_>, position: [f64; 3]) -> bool {
    crate::eggs::water_at(world, crate::flight::cell(position))
}
fn locate(
    world: World<'_>,
    mob: Entity<'_>,
    id: i32,
) -> Option<(EntityView, EntityBody, (f32, f32))> {
    ENTITIES.get()?.with_entity(world, id, |e| {
        if !hostile(mob, e) {
            return None;
        }
        Some((e.view()?, e.body()?, MOBS.get()?.health(e)?))
    })?
}

/// DragonEntity.isAlly: village defenders and companions of the same owner.
fn hostile(mob: Entity<'_>, target: Entity<'_>) -> bool {
    enemy(mob, target)
        && !matches!(
            target.type_id().trim_start_matches("minecraft:"),
            "villager" | "wandering_trader" | "iron_golem"
        )
        && !taming::owner(mob).is_some_and(|owner| taming::owner(target) == Some(owner))
}

/// AtroxiiaTargetingBehaviour: peaceful by default, then owner defense and retaliation.
pub fn targeting(phase: u32, world: World<'_>, mob: Entity<'_>) -> bool {
    if phase != goal_phase::TICK {
        return matches!(phase, goal_phase::CAN_START | goal_phase::CONTINUE);
    }
    let (Some(ai), Some(mobs), Some(entities), Some(view)) =
        (MOB_AI.get(), MOBS.get(), ENTITIES.get(), mob.view())
    else {
        return false;
    };
    if !eligible(mob) {
        if ai.target(mob).is_some() {
            ai.set_target(world, mob, None);
        }
        return false;
    }
    let usable = |id| {
        locate(world, mob, id)
            .is_some_and(|(v, _, _)| length(sub(v.position, view.position)) <= FOLLOW)
    };
    if ai.target(mob).is_some_and(|id| !usable(id)) {
        ai.set_target(world, mob, None);
    }
    let mut state = State::load(mob);
    let owner_threat = taming::owner(mob)
        .and_then(|u| mobs.by_uuid(world, &u))
        .and_then(|id| {
            entities
                .with_entity(world, id, |owner| ai.last_hurt_by(owner))
                .flatten()
        });
    if let Some((id, stamp)) = owner_threat
        && stamp != state.owner_hurt
        && usable(id)
    {
        state.owner_hurt = stamp;
        state.save(mob);
        ai.set_target(world, mob, Some(id));
    } else if let Some((id, stamp)) = ai.last_hurt_by(mob)
        && stamp != state.last_hurt
        && usable(id)
    {
        state.last_hurt = stamp;
        state.save(mob);
        ai.set_target(world, mob, Some(id));
    }
    // shortcut: host has no last-attacked-entity query for owner offense.
    false
}

struct Snapshot {
    gap: f64,
    facing: f64,
    enemies: usize,
    health: f32,
    ratio: f32,
    kite: i32,
    retreat: f64,
    lateral: f64,
    grounded: bool,
    clear_lane: bool,
}

/// AtroxiiaGroundCombatBehaviour.selectAction.
fn choose(s: &State, t: i64, x: &Snapshot) -> Option<Attack> {
    let mut best = None;
    let mut score = 0.0;
    let mut offer = |a, condition, value| {
        if condition && s.ready(a, t) && value > score {
            best = Some(a);
            score = value;
        }
    };
    let g = x.gap;
    let k = f64::from(x.kite);
    let n = x.enemies as f64;
    let quake_available =
        (x.enemies >= 3 || x.kite >= 50) && g <= 18.0 && s.ready(Attack::Quake, t);
    offer(Attack::Sweep, g <= 2.5, 150.0 + n * 12.0 + (2.5 - g) * 10.0);
    offer(
        Attack::Quake,
        quake_available,
        125.0 + n * 10.0 + (k * 0.4).min(30.0),
    );
    offer(
        Attack::Gungnir,
        !quake_available
            && x.enemies == 1
            && x.clear_lane
            && x.health >= 12.0
            && x.grounded
            && g > 4.0
            && g <= 8.0
            && (x.retreat >= 0.04 || x.kite >= 12)
            && x.lateral <= 0.16
            && x.facing >= 0.82,
        112.0 + (k * 0.45).min(18.0) + x.retreat.max(0.0) * 40.0,
    );
    offer(
        Attack::Precise,
        x.enemies == 1
            && x.health >= 18.0
            && x.ratio >= 0.35
            && x.facing >= 0.42
            && (3.0..=6.5).contains(&g),
        96.0 + f64::from(x.ratio) * 20.0,
    );
    offer(
        Attack::Slam,
        x.facing >= 0.42 && g <= 2.75,
        82.0 + (2.75 - g) * 8.0,
    );
    offer(
        Attack::Swipe,
        x.facing >= 0.42 && g <= 6.0,
        58.0 + (6.0 - g) * 2.0,
    );
    best
}

fn sound(world: World<'_>, mob: Entity<'_>, attack: Attack, elapsed: i64) {
    if let (Some(effects), Some(view)) = (EFFECTS.get(), mob.view()) {
        let cue = if cfg!(feature = "original-assets") {
            format!("saintsdragons:atroxiia_{}", attack.cue(elapsed))
        } else {
            "minecraft:entity.ravager.attack".into()
        };
        effects.play_sound(
            world,
            view.position,
            &cue,
            SOUND_NEUTRAL,
            if attack == Attack::Quake { 1.5 } else { 1.0 },
            1.0,
        );
    }
}

pub fn clip(mob: Entity<'_>) -> Option<&'static str> {
    let s = State::load(mob);
    s.active.map(|a| a.clip(s.right, now() - s.started))
}

fn turn(mob: Entity<'_>, view: EntityView, delta: [f64; 3], degrees: f32) -> f32 {
    if delta[0].hypot(delta[2]) < 1e-6 {
        return view.yaw;
    }
    let wanted = delta[2].atan2(delta[0]).to_degrees() as f32 - 90.0;
    let yaw =
        view.yaw + ((wanted - view.yaw + 180.0).rem_euclid(360.0) - 180.0).clamp(-degrees, degrees);
    mob.change(&EntityChange {
        mask: SET_YAW | SET_BODY_YAW | SET_HEAD_YAW,
        yaw,
        body_yaw: yaw,
        head_yaw: yaw,
        ..Default::default()
    });
    yaw
}

pub fn goal(phase: u32, world: World<'_>, mob: Entity<'_>) -> bool {
    let (Some(ai), Some(view), Some(body)) = (MOB_AI.get(), mob.view(), mob.body()) else {
        return false;
    };
    let target = ai
        .target(mob)
        .and_then(|id| locate(world, mob, id).map(|t| (id, t)));
    let mut s = State::load(mob);
    match phase {
        goal_phase::CAN_START | goal_phase::CONTINUE => {
            eligible(mob) && (target.is_some() || s.active.is_some())
        }
        goal_phase::STOP => {
            s.active = None;
            s.kite = 0;
            s.save(mob);
            ai.stop(mob);
            false
        }
        goal_phase::TICK => {
            let t = now();
            let swimming = wet(world, view.position);
            if let Some(a) = s.active {
                let elapsed = t - s.started;
                if elapsed >= a.pacing().0 || swimming != (a == Attack::Bite) {
                    s.active = None;
                    s.save(mob);
                } else {
                    if a != Attack::Bite {
                        ai.stop(mob);
                    }
                    if a == Attack::Precise
                        && let Some((_, (v, _, _))) = target
                    {
                        turn(mob, view, sub(v.position, view.position), 10.0);
                    }
                    execute(world, mob, a, elapsed, target.map(|(id, _)| id), s.yaw);
                    return false;
                }
            }
            let Some((id, (at, target_body, (health, max)))) = target else {
                return false;
            };
            let delta = sub(at.position, view.position);
            let g = gap(view, at, width(body), width(target_body));
            ai.look_at(mob, at.position);
            let yaw = turn(mob, view, delta, if swimming { 8.0 } else { 12.0 });
            if swimming {
                ai.stop(mob);
                let norm = length(delta).max(0.01);
                mob.change(&EntityChange {
                    mask: SET_MOTION,
                    motion: delta.map(|v| v / norm * 0.30),
                    ..Default::default()
                });
            } else if g > 6.0 {
                if t % 5 == 0 || !ai.navigating(mob) {
                    ai.walk_to(mob, at.position, 1.45);
                }
            } else {
                ai.stop(mob);
            }
            s.kite = if g > 7.5 {
                (s.kite + 1).min(100)
            } else {
                (s.kite - 2).max(0)
            };
            if t < s.decision || (!swimming && !view.on_ground) {
                s.save(mob);
                return false;
            }
            s.decision = t + 6;
            let dir = horizontal(delta, forward(yaw));
            let retreat = dot(dir, at.motion);
            let lateral = (at.motion[0] - dir[0] * retreat).hypot(at.motion[2] - dir[2] * retreat);
            let facing = dot(dir, forward(yaw));
            let attack = if swimming {
                (g <= 5.0 && s.ready(Attack::Bite, t)).then_some(Attack::Bite)
            } else {
                let enemies = engaged(world, mob, id, body);
                // shortcut: native frozen ticks and LOS are not exposed; the frozen-target combo
                // needs those calls. The block collision sweep approximates the Java lane check.
                choose(
                    &s,
                    t,
                    &Snapshot {
                        gap: g,
                        facing,
                        enemies,
                        health,
                        ratio: health / max.max(1.0),
                        kite: s.kite,
                        retreat,
                        lateral,
                        grounded: at.on_ground && !wet(world, at.position) && at.motion[1] <= 0.08,
                        clear_lane: (1..=((g - 0.35).clamp(0.0, 8.0) as i32)).all(|step| {
                            crate::flight::clear_body(world, body, dir.map(|v| v * f64::from(step)))
                        }),
                    },
                )
            };
            if let Some(a) = attack {
                s.begin(a, t, yaw);
                sound(world, mob, a, 0);
                if a != Attack::Bite {
                    ai.stop(mob);
                }
            }
            s.save(mob);
            false
        }
        _ => false,
    }
}

fn engaged(world: World<'_>, mob: Entity<'_>, primary: i32, body: EntityBody) -> usize {
    let (Some(entities), Some(ai)) = (ENTITIES.get(), MOB_AI.get()) else {
        return 1;
    };
    entities
        .entities_in(
            world,
            inflate(body.min, [-18.0, -6.0, -18.0]),
            inflate(body.max, [18.0, 6.0, 18.0]),
            mob.id(),
        )
        .into_iter()
        .filter(|&id| {
            entities
                .with_entity(world, id, |e| {
                    hostile(mob, e)
                        && (id == primary
                            || ai.target(e) == Some(mob.id())
                            || ai
                                .last_hurt_by(e)
                                .is_some_and(|(attacker, _)| attacker == mob.id()))
                })
                .unwrap_or(false)
        })
        .count()
        .max(1)
}
fn inflate(v: [f64; 3], by: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|i| v[i] + by[i])
}

fn push(target: Entity<'_>, direction: [f64; 3], strength: f64, lift: f64) {
    if let Some(v) = target.view() {
        target.change(&EntityChange {
            mask: SET_MOTION,
            motion: inflate(
                v.motion,
                [direction[0] * strength, lift, direction[2] * strength],
            ),
            ..Default::default()
        });
    }
}

/// AtroxiiaFrostImpact.apply. Native frozen ticks need host support.
fn frost(world: World<'_>, target: Entity<'_>, ticks: i32, particles: bool) {
    target.add_effect("minecraft:slowness", ticks, 6, false, true);
    target.add_effect("minecraft:weakness", ticks, 1, false, true);
    if particles && let (Some(e), Some(v)) = (EFFECTS.get(), target.view()) {
        e.particles(
            world,
            "minecraft:snowflake",
            inflate(v.position, [0.0, 1.0, 0.0]),
            [0.4; 3],
            0.2,
            28,
        );
    }
    if MOBS.get().and_then(|m| m.kind(target)) == Some(crate::CINDERVANE) {
        crate::combat::suppress_fire(target);
    }
}

fn forward_hit(
    origin: [f64; 3],
    dir: [f64; 3],
    body: EntityBody,
    range: f64,
    inflate: f64,
    close: f64,
    angle: f64,
) -> bool {
    let nearest = std::array::from_fn(|i| origin[i].clamp(body.min[i], body.max[i]));
    let delta = sub(nearest, origin);
    let dist = length(delta);
    if dist > range + inflate {
        return false;
    }
    if dist <= close {
        return true;
    }
    let cosine = dot(delta, dir) / dist;
    cosine > 0.0 && (cosine >= angle.to_radians().cos() || dist < range * 0.35)
}

#[derive(Clone, Copy)]
struct Hit {
    range: f64,
    horizontal: f64,
    vertical: f64,
    angle: f64,
    damage: f32,
    push: f64,
    lift: f64,
    stun: i32,
}
fn hit(world: World<'_>, mob: Entity<'_>, attack: Attack, spec: Hit, pull: bool) {
    let (Some(entities), Some(ai), Some(v), Some(body)) =
        (ENTITIES.get(), MOB_AI.get(), mob.view(), mob.body())
    else {
        return;
    };
    let radial = matches!(attack, Attack::Sweep | Attack::Quake);
    let dir = forward(v.yaw);
    let offset = width(body) * 0.35 + if attack == Attack::Gungnir { 1.0 } else { 0.0 };
    let origin = inflate(
        v.position,
        [
            dir[0] * offset,
            (body.max[1] - body.min[1]) * 0.52,
            dir[2] * offset,
        ],
    );
    let end = inflate(origin, dir.map(|d| d * spec.range));
    let (min, max) = if radial {
        (
            inflate(body.min, [-spec.range, -spec.vertical, -spec.range]),
            inflate(body.max, [spec.range, spec.vertical, spec.range]),
        )
    } else {
        (
            std::array::from_fn(|i| {
                origin[i].min(end[i])
                    - if i == 1 {
                        spec.vertical
                    } else {
                        spec.horizontal
                    }
            }),
            std::array::from_fn(|i| {
                origin[i].max(end[i])
                    + if i == 1 {
                        spec.vertical
                    } else {
                        spec.horizontal
                    }
            }),
        )
    };
    for id in entities.entities_in(world, min, max, mob.id()) {
        entities.with_entity(world, id, |target| {
            let (Some(at), Some(b)) = (target.view(), target.body()) else {
                return;
            };
            if !hostile(mob, target) {
                return;
            }
            let delta = sub(at.position, v.position);
            let valid = match attack {
                Attack::Sweep => delta[0].hypot(delta[2]) <= spec.range,
                Attack::Quake => length(delta) <= spec.range,
                Attack::Gungnir => true,
                _ => forward_hit(
                    origin,
                    dir,
                    b,
                    spec.range,
                    spec.horizontal,
                    spec.range * 0.45,
                    spec.angle,
                ),
            };
            if !valid {
                return;
            }
            if pull {
                push(
                    target,
                    horizontal(sub(v.position, at.position), dir),
                    1.0,
                    0.12,
                );
                return;
            }
            let damaged = if attack == Attack::Gungnir {
                target.hurt("minecraft:magic", spec.damage)
            } else {
                ai.attack(
                    world,
                    mob,
                    id,
                    spec.damage * crate::hunger::melee_multiplier(mob),
                )
            };
            if attack == Attack::Quake && !damaged {
                return;
            }
            if spec.stun > 0 {
                frost(world, target, spec.stun, attack != Attack::Quake);
            }
            if spec.push != 0.0 {
                push(
                    target,
                    if radial { horizontal(delta, dir) } else { dir },
                    spec.push,
                    spec.lift,
                );
            }
        });
    }
}

/// AtroxiiaFrostWalker.freezeNearbyWater: source water exposed to air within radius 12,
/// scanning from the dragon's feet up to ten blocks down.
fn freeze_water(world: World<'_>, at: [f64; 3]) {
    let Some(content) = CONTENT.get() else { return };
    let (Some(water), Some(air), Some(ice)) = (
        content.state("minecraft:water[level=0]"),
        content.state("minecraft:air"),
        content.state("minecraft:ice"),
    ) else {
        return;
    };
    let c = crate::flight::cell(at);
    let mut positions = Vec::new();
    for x in -12..=12 {
        for z in -12..=12 {
            if x * x + z * z > 144 {
                continue;
            }
            for y in -10..=1 {
                positions.push(Pos {
                    x: c.x + x,
                    y: c.y + y,
                    z: c.z + z,
                });
            }
        }
    }
    let mut states = vec![0; positions.len()];
    if !world.read(&positions, &mut states) {
        return;
    }
    let mut updates = Vec::new();
    for (ps, ss) in positions
        .as_chunks::<12>()
        .0
        .iter()
        .zip(states.as_chunks::<12>().0)
    {
        if let Some(i) = (0..11).rev().find(|&i| ss[i] == water && ss[i + 1] == air) {
            updates.push(BlockUpdate {
                position: ps[i],
                state: ice,
                flags: UPDATE_ALL,
            });
        }
    }
    world.write(&updates);
}

fn nudge(mob: Entity<'_>, yaw: f32, speed: f64) {
    if let Some(v) = mob.view() {
        let d = forward(yaw);
        mob.change(&EntityChange {
            mask: SET_MOTION,
            motion: [d[0] * speed, v.motion[1], d[2] * speed],
            ..Default::default()
        });
    }
}

/// Ability tracks from the seven Atroxiia ability classes. Slither is rider-only.
fn execute(world: World<'_>, mob: Entity<'_>, a: Attack, t: i64, target: Option<i32>, yaw: f32) {
    let spec = match (a, t) {
        (Attack::Slam, 10) => Hit {
            range: 5.5,
            horizontal: 4.0,
            vertical: 4.0,
            angle: 100.0,
            damage: 16.0,
            push: 0.45,
            lift: 0.18,
            stun: 0,
        },
        (Attack::Swipe, 13) => Hit {
            range: 6.0,
            horizontal: 4.5,
            vertical: 3.5,
            angle: 115.0,
            damage: 12.0,
            push: 0.3,
            lift: 0.1,
            stun: 0,
        },
        (Attack::Sweep, 10) => Hit {
            range: 12.0,
            horizontal: 12.0,
            vertical: 6.0,
            angle: 180.0,
            damage: 13.0,
            push: 1.65,
            lift: 0.32,
            stun: 0,
        },
        (Attack::Precise, 11 | 25 | 45 | 64) => Hit {
            range: 8.5,
            horizontal: 10.5,
            vertical: 10.5,
            angle: 100.0,
            damage: 9.0,
            push: 0.75,
            lift: 0.16,
            stun: (148 - t) as i32,
        },
        (Attack::Gungnir, 17) => Hit {
            range: 20.0,
            horizontal: 3.0,
            vertical: 4.0,
            angle: 180.0,
            damage: 40.0,
            push: 0.0,
            lift: 0.0,
            stun: 60,
        },
        (Attack::Quake, 18 | 36) => Hit {
            range: 20.0,
            horizontal: 20.0,
            vertical: 6.0,
            angle: 180.0,
            damage: 25.0,
            push: if t == 18 { 0.75 } else { 1.8 },
            lift: if t == 18 { 0.2 } else { 0.5 },
            stun: 100,
        },
        (Attack::Bite, 9) => {
            if let (Some(id), Some(ai), Some(v), Some(b)) =
                (target, MOB_AI.get(), mob.view(), mob.body())
                && locate(world, mob, id)
                    .is_some_and(|(at, body, _)| gap(v, at, width(b), width(body)) <= 5.0)
            {
                ai.attack(world, mob, id, 10.0 * crate::hunger::melee_multiplier(mob));
                if let Some(entities) = ENTITIES.get() {
                    entities.with_entity(world, id, |e| push(e, forward(v.yaw), 0.2, 0.05));
                }
            }
            return;
        }
        (Attack::Quake, 22) => {
            sound(world, mob, a, 22);
            return;
        }
        (Attack::Gungnir, 12..=15) => {
            nudge(mob, yaw, 0.5);
            return;
        }
        (Attack::Gungnir, 18..=23) => {
            nudge(mob, yaw, 8.0 / 7.0);
            return;
        }
        (Attack::Precise, 12..=15 | 39..=43 | 53..=57) => {
            if let Some(v) = mob.view() {
                nudge(mob, v.yaw, 1.0);
            }
            return;
        }
        _ => return,
    };
    if a == Attack::Quake
        && t == 18
        && let Some(v) = mob.view()
    {
        freeze_water(world, v.position);
    }
    if a == Attack::Gungnir
        && let Some(v) = mob.view()
    {
        mob.change(&EntityChange {
            mask: SET_YAW | SET_BODY_YAW,
            yaw,
            body_yaw: yaw,
            ..Default::default()
        });
        nudge(mob, v.yaw, 8.0 / 7.0);
    }
    if a == Attack::Precise
        && t == 11
        && let Some(v) = mob.view()
    {
        nudge(mob, v.yaw, 1.0);
    }
    hit(world, mob, a, spec, a == Attack::Precise && t == 25);
}

#[cfg(test)]
mod tests {
    use super::*;
    fn snap(gap: f64) -> Snapshot {
        Snapshot {
            gap,
            facing: 1.0,
            enemies: 1,
            health: 20.0,
            ratio: 1.0,
            kite: 0,
            retreat: 0.0,
            lateral: 0.0,
            grounded: true,
            clear_lane: true,
        }
    }
    #[test]
    fn scores_and_cooldowns() {
        let mut s = State::default();
        assert_eq!(choose(&s, 0, &snap(2.0)), Some(Attack::Sweep));
        assert_eq!(choose(&s, 0, &snap(4.0)), Some(Attack::Precise));
        let mut pressure = snap(10.0);
        pressure.kite = 50;
        assert_eq!(choose(&s, 0, &pressure), Some(Attack::Quake));
        let mut retreat = snap(7.0);
        retreat.retreat = 0.1;
        assert_eq!(choose(&s, 0, &retreat), Some(Attack::Gungnir));
        retreat.clear_lane = false;
        assert_eq!(choose(&s, 0, &retreat), None);
        s.begin(Attack::Sweep, 0, 0.0);
        assert_eq!(choose(&s, 31, &snap(2.0)), None);
        assert_eq!(choose(&s, 32, &snap(2.0)), Some(Attack::Slam));
        assert_eq!(choose(&s, 240, &snap(2.0)), Some(Attack::Sweep));
    }
    #[test]
    fn forward_geometry_rejects_behind() {
        let body = |z| EntityBody {
            min: [-0.5, -0.5, z],
            max: [0.5, 0.5, z + 1.0],
            ..Default::default()
        };
        assert!(forward_hit(
            [0.0; 3],
            [0.0, 0.0, 1.0],
            body(4.0),
            5.5,
            4.0,
            2.475,
            100.0
        ));
        assert!(!forward_hit(
            [0.0; 3],
            [0.0, 0.0, 1.0],
            body(-6.0),
            5.5,
            4.0,
            2.475,
            100.0
        ));
    }
    #[test]
    fn nbt_roundtrip() {
        let empty: State =
            fastnbt::from_bytes(&fastnbt::to_bytes(&State::default()).unwrap()).unwrap();
        assert_eq!(empty.active, None);
        let mut s = State::default();
        s.begin(Attack::Sweep, 12, 0.0);
        let d: State = fastnbt::from_bytes(&fastnbt::to_bytes(&s).unwrap()).unwrap();
        assert_eq!(d.active, Some(Attack::Sweep));
        assert_eq!(d.cooldown[0], 252);
    }
}
