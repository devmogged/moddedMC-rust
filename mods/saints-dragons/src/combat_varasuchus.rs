//! Varasuchus combat tracks and brain from Saint's Dragons 0.9.88.
use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
};

use moddedmc_api::{
    Pos,
    content::World,
    effects::SOUND_NEUTRAL,
    entities::{Entity, EntityChange, SET_BODY_YAW, SET_HEAD_YAW, SET_MOTION, SET_YAW},
    mob_ai::goal_phase,
    mobs::PLAYER_INPUT_JUMP,
    player_actions::{ACTION_PRESSED, ACTION_RELEASED},
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
    #[serde(rename = "MeleeMode")]
    horn_mode: bool,
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
    /// Bite, horn and claw abilities scale from the live attack attribute.
    fn scaled_damage(self, phase_two: bool, hunger: f32, attribute: Option<f64>) -> f32 {
        let mut damage = self.damage(phase_two);
        if let Some(value) = attribute.filter(|v| *v > 0.0) {
            match self {
                Self::Bite | Self::Bite2 | Self::Horn => damage *= (value / 10.0) as f32,
                Self::Claw => damage = (value * 1.2) as f32,
                _ => {}
            }
        }
        damage * hunger
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Guard {
    Startup(u64),
    Hold(u64),
    Cancel(u64),
    Parry(u64),
}
impl Guard {
    fn release(self, now: u64) -> Self {
        match self {
            Self::Startup(_) | Self::Hold(_) => Self::Cancel(now + 8),
            _ => self,
        }
    }
    fn advance(self, now: u64) -> Option<Self> {
        match self {
            Self::Startup(end) if now >= end => Some(Self::Hold(end + 383)),
            Self::Hold(end) if now >= end => Some(Self::Cancel(now + 8)),
            Self::Cancel(end) | Self::Parry(end) if now >= end => None,
            _ => Some(self),
        }
    }
    fn clip(self) -> &'static str {
        match self {
            Self::Startup(_) => "tailguard",
            Self::Hold(_) => "tailguard_hold",
            Self::Cancel(_) => "tailguard_cancel",
            Self::Parry(_) => "tailguard_parry",
        }
    }
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
    rider: Option<i32>,
    guard: Option<Guard>,
    guard_ready: u64,
    flex_end: u64,
    flex_ready: u64,
    ascend: bool,
    descend: bool,
    accelerate: bool,
    dash: Option<Dash>,
    dash_ready: u64,
    right_dash: bool,
}
#[derive(Clone, Copy)]
struct Dash {
    start: u64,
    end: u64,
    direction: [f64; 3],
    speed: f64,
    phase_two: bool,
    clip: &'static str,
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
fn begin_ride(mob: Entity<'_>, rider: Entity<'_>) {
    state(mob, |s| {
        if s.rider != Some(rider.id()) {
            s.rider = Some(rider.id());
            s.active = None;
            s.guard = None;
            s.ascend = false;
            s.descend = false;
            s.accelerate = false;
            s.dash = None;
            s.flex_end = 0;
        }
    });
}
fn end_ride(mob: Entity<'_>) {
    let now = tick_count();
    state(mob, |s| {
        if s.rider.take().is_some() {
            s.active = None;
            if let Some(guard) = s.guard.take() {
                s.guard_ready = now
                    + if matches!(guard, Guard::Parry(_)) {
                        300
                    } else {
                        40
                    };
            }
            s.dash = None;
            s.flex_end = 0;
        }
    });
}
fn sound(world: World<'_>, mob: Entity<'_>, name: &str, volume: f32) {
    if let (Some(effects), Some(view)) = (EFFECTS.get(), mob.view()) {
        let name = if cfg!(feature = "original-assets") {
            name
        } else {
            "entity.ender_dragon.growl"
        };
        effects.play_sound(world, view.position, name, SOUND_NEUTRAL, volume, 1.0);
    }
}
fn attack_for(action: &str, saved: Saved, in_water: bool) -> Option<Attack> {
    match action {
        "attack" => Some(if saved.horn_mode {
            Attack::Horn
        } else if saved.phase_two {
            Attack::Bite2
        } else {
            Attack::Bite
        }),
        "ability_primary" if saved.phase_two => Some(Attack::Barrage),
        "ability_secondary" => Some(Attack::Shift),
        "ability_tertiary" if saved.phase_two => Some(Attack::Claw),
        "ability_tertiary" if !in_water => Some(Attack::Tail),
        _ => None,
    }
}
/// Varasuchus rider bindings. Only the controlling, tame owner may issue them.
pub fn rider_action(world: World<'_>, mob: Entity<'_>, rider: Entity<'_>, action: &str, phase: u8) {
    let (Some(mobs), Some(view)) = (MOBS.get(), mob.view()) else {
        return;
    };
    if !taming::is_owner(mob, rider)
        || crate::eggs::is_juvenile(mob)
        || mobs.passengers(mob).first() != Some(&rider.id())
    {
        return;
    }
    begin_ride(mob, rider);
    let action = match action {
        "moddedmc:mounted_attack" => "attack",
        "moddedmc:double_tap/forward" => "double_tap_forward",
        _ => action.strip_prefix("saintsdragons:").unwrap_or(action),
    };
    let now = tick_count();
    let saved = Saved::load(mob);
    let in_water = wet(world, view.position);
    if matches!(action, "ascend" | "descend" | "accelerate") {
        let down = phase != ACTION_RELEASED;
        state(mob, |s| match action {
            "ascend" => s.ascend = down,
            "descend" => s.descend = down,
            "accelerate" => s.accelerate = down,
            _ => {}
        });
        return;
    }
    if action == "ability_primary" && phase == ACTION_RELEASED {
        state(mob, |s| s.guard = s.guard.map(|g| g.release(now)));
        return;
    }
    if phase != ACTION_PRESSED {
        return;
    }
    let s = snapshot(mob);
    if s.guard.is_some()
        || s.active.is_some_and(|r| now < r.end)
        || now < s.flex_end
        || s.dash.is_some()
    {
        return;
    }
    match action {
        "toggle_melee" => {
            Saved {
                horn_mode: !saved.horn_mode,
                ..saved
            }
            .save(mob);
        }
        "ability_primary" if !saved.phase_two && now >= s.guard_ready => {
            state(mob, |s| s.guard = Some(Guard::Startup(now + 17)));
            sound(world, mob, "saintsdragons:varasuchus_guarding", 1.2);
        }
        "flex" if view.on_ground && !in_water && now >= s.flex_ready => {
            let end = now + if saved.phase_two { 140 } else { 70 };
            state(mob, |s| {
                s.flex_end = end;
                s.flex_ready = now + 60;
            });
            sound(
                world,
                mob,
                if saved.phase_two {
                    "saintsdragons:varasuchus_flex2"
                } else {
                    "saintsdragons:varasuchus_flex"
                },
                2.0,
            );
        }
        "double_tap_forward" => start_dash(world, mob, saved.phase_two, in_water),
        _ => {
            if let Some(attack) = attack_for(action, saved, in_water)
                && now >= s.ready[attack as usize]
                && (attack != Attack::Shift || view.on_ground || in_water)
            {
                let phase_two = if attack == Attack::Shift {
                    !saved.phase_two
                } else {
                    saved.phase_two
                };
                start(world, mob, attack, phase_two, in_water);
                if attack == Attack::Claw && !in_water && s.accelerate {
                    state(mob, |s| {
                        if let Some(r) = &mut s.active {
                            r.clip = if s.left_claw {
                                "run_and_claw_left"
                            } else {
                                "run_and_claw_right"
                            };
                        }
                    });
                }
            }
        }
    }
}
fn start_dash(world: World<'_>, mob: Entity<'_>, phase_two: bool, in_water: bool) {
    let Some(view) = mob.view() else {
        return;
    };
    let now = tick_count();
    let s = snapshot(mob);
    if in_water
        || s.dash.is_some()
        || now < s.dash_ready
        || s.guard.is_some()
        || now < s.flex_end
        || s.active.is_some_and(|r| now < r.end)
    {
        return;
    }
    let duration = if phase_two { 33 } else { 47 };
    let speed = 26.0 * (1.0 - 0.92) / (1.0 - 0.92_f64.powi(duration as i32));
    let (sin, cos) = f64::from(view.yaw).to_radians().sin_cos();
    state(mob, |s| {
        s.right_dash = !s.right_dash;
        s.dash = Some(Dash {
            start: now,
            end: now + duration,
            direction: [-sin, 0.0, cos],
            speed,
            phase_two,
            clip: if !phase_two {
                "tail_swipe_left"
            } else if s.right_dash {
                "phase2_dash_right"
            } else {
                "phase2_dash_left"
            },
        });
        s.dash_ready = now + duration + if phase_two { 38 } else { 60 };
    });
    sound(
        world,
        mob,
        if phase_two {
            "saintsdragons:varasuchus_phase2_dash"
        } else {
            "saintsdragons:varasuchus_tail_swipe"
        },
        1.0,
    );
}
fn dash_hit(world: World<'_>, mob: Entity<'_>, phase_two: bool) {
    let (Some(ai), Some(entities), Some(view), Some(body)) =
        (MOB_AI.get(), ENTITIES.get(), mob.view(), mob.body())
    else {
        return;
    };
    let center: [f64; 3] = std::array::from_fn(|i| (body.min[i] + body.max[i]) * 0.5);
    for id in entities.entities_in(
        world,
        center.map(|x| x - 8.0),
        center.map(|x| x + 8.0),
        mob.id(),
    ) {
        entities.with_entity(world, id, |e| {
            if !enemy(world, mob, e) {
                return;
            }
            ai.attack(world, mob, id, if phase_two { 15.0 } else { 10.0 });
            if !phase_two && let Some(other) = e.view() {
                let d = flight::offset(other.position, view.position);
                let n = flight::length_squared(d).sqrt().max(0.0001);
                e.change(&EntityChange {
                    mask: SET_MOTION,
                    motion: [
                        other.motion[0] + 2.0 * d[0] / n,
                        other.motion[1] + 0.8,
                        other.motion[2] + 2.0 * d[2] / n,
                    ],
                    ..EntityChange::default()
                });
            }
        });
    }
}
/// VarasuchusTailguardAbility.tryParry: the causing living entity must be within
/// eight blocks. Change the state before retaliating, since attack callbacks may reenter.
pub fn parry(world: World<'_>, mob: Entity<'_>) -> bool {
    let (Some(ai), Some(entities), Some(view), Some(body)) =
        (MOB_AI.get(), ENTITIES.get(), mob.view(), mob.body())
    else {
        return false;
    };
    let now = tick_count();
    let s = snapshot(mob);
    if !matches!(s.guard, Some(Guard::Hold(end)) if now < end) {
        return false;
    }
    let Some((attacker, _)) = ai.damage_source() else {
        return false;
    };
    if !entities
        .with_entity(world, attacker, |e| {
            enemy(world, mob, e)
                && e.view()
                    .is_some_and(|v| dist(v.position, view.position) <= 8.0)
        })
        .unwrap_or(false)
    {
        return false;
    }
    state(mob, |s| s.guard = Some(Guard::Parry(now + 17)));
    sound(world, mob, "saintsdragons:varasuchus_parry", 1.4);
    let ids = if s.rider.is_some() {
        entities.entities_in(
            world,
            [body.min[0] - 6.5, body.min[1] - 3.0, body.min[2] - 6.5],
            [body.max[0] + 6.5, body.max[1] + 3.0, body.max[2] + 6.5],
            mob.id(),
        )
    } else {
        vec![attacker]
    };
    for id in ids {
        entities.with_entity(world, id, |e| {
            if !enemy(world, mob, e) {
                return;
            }
            let Some(other) = e.view() else {
                return;
            };
            ai.attack(world, mob, id, 10.0 * hunger::melee_multiplier(mob));
            let mut delta = flight::offset(other.position, view.position);
            if flight::length_squared(delta) < 0.0001 {
                let (sin, cos) = f64::from(view.yaw).to_radians().sin_cos();
                delta = [-sin, 0.0, cos];
            }
            let n = flight::length_squared(delta).sqrt().max(0.0001);
            let current = e.view().unwrap_or(other);
            e.change(&EntityChange {
                mask: SET_MOTION,
                motion: [
                    current.motion[0] + delta[0] / n * 1.5,
                    current.motion[1] + 0.35,
                    current.motion[2] + delta[2] / n * 1.5,
                ],
                ..EntityChange::default()
            });
        });
    }
    true
}
/// Rider movement and ability ticks run in the existing ride-or-sit goal.
pub fn ride(world: World<'_>, mob: Entity<'_>, rider: Entity<'_>) {
    let (Some(mobs), Some(view), Some(look), Some(input)) =
        (MOBS.get(), mob.view(), rider.view(), rider.body())
    else {
        return;
    };
    begin_ride(mob, rider);
    let now = tick_count();
    let in_water = wet(world, view.position);
    let saved = Saved::load(mob);
    let mut s = snapshot(mob);
    if let Some(g) = s.guard {
        state(mob, |s| {
            s.guard = g.advance(now);
            if s.guard.is_none() {
                s.guard_ready = now
                    + if matches!(g, Guard::Parry(_)) {
                        300
                    } else {
                        40
                    };
            }
        });
    }
    if let Some(r) = s.active {
        if now >= r.end {
            state(mob, |s| s.active = None);
        } else if r.attack.hit(now.saturating_sub(r.start)) {
            hit(
                world,
                mob,
                None,
                r.attack,
                saved.phase_two,
                in_water,
                Some(rider),
            );
        }
    }
    s = snapshot(mob);
    let locked = s.guard.is_some()
        || now < s.flex_end
        || s.active
            .is_some_and(|r| matches!(r.attack, Attack::Tail | Attack::Shift));
    let yaw = f64::from(look.yaw).to_radians();
    let forward = input.input[2];
    let strafe = input.input[0];
    state(mob, |s| {
        if s.dash.is_some_and(|d| now >= d.end) {
            s.dash = None;
        }
    });
    s = snapshot(mob);
    let accelerating = s.accelerate && !s.active.is_some_and(|r| r.attack == Attack::Barrage);
    let happiness = taming::happiness(mob).unwrap_or(100);
    let multiplier = match happiness {
        61.. => 1.0,
        ..=30 => 0.5,
        _ => 0.5 + 0.5 * f64::from(happiness - 30) / 30.0,
    };
    let motion = if let Some(dash) = s.dash {
        let elapsed = now.saturating_sub(dash.start);
        if elapsed == if dash.phase_two { 18 } else { 32 } {
            dash_hit(world, mob, dash.phase_two);
        }
        let speed = dash.speed * 0.92_f64.powi(elapsed as i32);
        [
            dash.direction[0] * speed,
            view.motion[1],
            dash.direction[2] * speed,
        ]
    } else if locked {
        [
            0.0,
            if in_water {
                0.0
            } else {
                view.motion[1].min(0.0)
            },
            0.0,
        ]
    } else if in_water {
        let speed = 1.45 * if accelerating { 1.6 } else { 1.0 };
        let pitch = f64::from(look.pitch).to_radians();
        let f = forward * if forward < 0.0 { 0.5 } else { 1.0 };
        let desired = [
            -yaw.sin() * pitch.cos() * f + yaw.cos() * strafe * 0.3,
            -pitch.sin() * f * 1.2,
            yaw.cos() * pitch.cos() * f + yaw.sin() * strafe * 0.3,
        ];
        let len = flight::length_squared(desired).sqrt();
        let desired = if len > 0.01 {
            desired.map(|d| d / len * speed)
        } else {
            [0.0, view.motion[1] * 0.9, 0.0]
        };
        let mut result = std::array::from_fn(|i| {
            (view.motion[i] + (desired[i] - view.motion[i]) * 0.28)
                * if i == 1 { 0.96 } else { 0.94 }
        });
        if s.ascend {
            result[1] = (result[1] + 0.12 * speed).min(speed * 0.8);
        } else if s.descend {
            result[1] = (result[1] - 0.12 * speed).max(-speed * 0.8);
        }
        result
    } else {
        let speed = if accelerating { 0.30 } else { 0.15 } * multiplier;
        let jump = (s.ascend || mobs.player_input(rider).unwrap_or(0) & PLAYER_INPUT_JUMP != 0)
            && view.on_ground;
        let f = forward * if forward < 0.0 { 0.5 } else { 1.0 };
        [
            (strafe * 0.5 * yaw.cos() - f * yaw.sin()) * speed
                - if jump && f > 0.0 {
                    yaw.sin() * 0.4
                } else {
                    0.0
                },
            if jump { 1.0 } else { view.motion[1] },
            (strafe * 0.5 * yaw.sin() + f * yaw.cos()) * speed
                + if jump && f > 0.0 {
                    yaw.cos() * 0.4
                } else {
                    0.0
                },
        ]
    };
    mob.change(&EntityChange {
        mask: SET_MOTION | SET_YAW | SET_BODY_YAW | SET_HEAD_YAW,
        motion,
        yaw: if locked || s.dash.is_some() {
            view.yaw
        } else {
            look.yaw
        },
        body_yaw: if locked || s.dash.is_some() {
            view.yaw
        } else {
            look.yaw
        },
        head_yaw: if locked || s.dash.is_some() {
            view.yaw
        } else {
            look.yaw
        },
        ..EntityChange::default()
    });
    rider.reset_fall_distance();
}
pub fn forget(id: i32) {
    if let Some(states) = STATES
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_mut()
    {
        states.remove(&id);
    }
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
fn enemy(world: World<'_>, mob: Entity<'_>, other: Entity<'_>) -> bool {
    abilities::enemy(world, mob, other)
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
        if !enemy(world, mob, other) {
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
                if !enemy(world, mob, e) || !predicate(e) {
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
        if mobs.passengers(mob).is_empty() {
            end_ride(mob);
        }
        return false;
    }
    end_ride(mob);
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
        let mut saved = Saved::load(mob);
        saved.phase_two = phase_two;
        saved.save(mob);
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
    let s = snapshot(mob);
    if let Some(guard) = s.guard {
        return Some(guard.clip());
    }
    if tick_count() < s.flex_end {
        return Some(if Saved::load(mob).phase_two {
            "flex2"
        } else {
            "flex"
        });
    }
    if let Some(dash) = s.dash {
        return Some(dash.clip);
    }
    s.active.filter(|r| tick_count() < r.end).map(|r| r.clip)
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
                if s.rider.is_none() && s.active.is_some_and(|r| r.attack != Attack::Shift) {
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
                        hit(world, mob, Some(id), r.attack, phase_two, in_water, None);
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
    primary: Option<i32>,
    attack: Attack,
    phase_two: bool,
    wet: bool,
    rider: Option<Entity<'_>>,
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
    let targets = if let Some(rider) = rider {
        ridden_targets(world, mob, rider, attack)
    } else if attack == Attack::Tail {
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
        primary.into_iter().collect()
    };
    for id in targets {
        entities.with_entity(world, id, |e| {
            if !enemy(world, mob, e) {
                return;
            }
            let Some(other) = e.view() else { return };
            let d = flight::offset(other.position, view.position);
            let distance = dist(other.position, view.position);
            if rider.is_none()
                && attack == Attack::Tail
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
            let mut damage = attack.scaled_damage(
                phase_two,
                hunger::melee_multiplier(mob),
                mob.attribute_value("minecraft:attack_damage"),
            );
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
    if attack == Attack::Claw
        && let Some(rider) = rider
    {
        claw_blocks(world, mob, rider);
    }
}

fn ridden_geometry(mob: Entity<'_>, rider: Entity<'_>) -> Option<([f64; 3], [f64; 3])> {
    let (view, body, look) = (mob.view()?, mob.body()?, rider.view()?);
    let (sin, cos) = f64::from(look.yaw).to_radians().sin_cos();
    let (ps, pc) = f64::from(look.pitch).to_radians().sin_cos();
    let width = body.max[0] - body.min[0];
    Some((
        [
            view.position[0] - sin * width * 0.35,
            view.position[1] + (body.max[1] - body.min[1]) * 0.52,
            view.position[2] + cos * width * 0.35,
        ],
        [-sin * pc, -ps, cos * pc],
    ))
}

/// VarasuchusClawAbility.canBreakBlock: waterlogged solid blocks are eligible;
/// only air, liquid blocks and hardness outside 0..=30 are excluded.
fn claw_breakable(selector: &str, hardness: f32) -> bool {
    let id = selector.split('[').next().unwrap_or(selector);
    !matches!(
        id,
        "minecraft:air"
            | "minecraft:cave_air"
            | "minecraft:void_air"
            | "minecraft:water"
            | "minecraft:lava"
    ) && (0.0..=30.0).contains(&hardness)
}

/// VarasuchusClawAbility.breakBlocksInPath.
// shortcut: the vanilla mob griefing rule is exposed, but not the dragon config
// or destroyBlock's causing-entity argument.
fn claw_blocks(world: World<'_>, mob: Entity<'_>, rider: Entity<'_>) {
    if !MOBS
        .get()
        .is_some_and(|m| m.boolean_gamerule(world, "minecraft:mob_griefing") == Some(true))
    {
        return;
    }
    let Some((origin, direction)) = ridden_geometry(mob, rider) else {
        return;
    };
    let Some(content) = crate::CONTENT.get() else {
        return;
    };
    let end: [f64; 3] = std::array::from_fn(|i| origin[i] + direction[i] * 6.0);
    let min = [
        (origin[0].min(end[0]) - 3.0).floor() as i32,
        origin[1].min(end[1]).floor() as i32,
        (origin[2].min(end[2]) - 3.0).floor() as i32,
    ];
    let max = [
        (origin[0].max(end[0]) + 3.0).ceil() as i32,
        (origin[1].max(end[1]) + 6.0).ceil() as i32,
        (origin[2].max(end[2]) + 3.0).ceil() as i32,
    ];
    for x in min[0]..=max[0] {
        for y in min[1]..=max[1] {
            for z in min[2]..=max[2] {
                let delta = flight::offset(
                    [f64::from(x) + 0.5, f64::from(y) + 0.5, f64::from(z) + 0.5],
                    origin,
                );
                let forward = delta.iter().zip(direction).map(|(a, b)| a * b).sum::<f64>();
                let lateral = (flight::length_squared(delta) - forward * forward).max(0.0);
                if !(0.0..=6.5).contains(&forward)
                    || lateral > 3.5 * 3.5
                    || !(-1.0..=6.5).contains(&delta[1])
                {
                    continue;
                }
                let pos = Pos { x, y, z };
                let mut state = [moddedmc_api::content::UNLOADED_STATE];
                if world.read(&[pos], &mut state)
                    && let Some(selector) = content.describe_state(state[0])
                    && world
                        .destroy_speed(pos)
                        .is_some_and(|h| claw_breakable(&selector, h))
                {
                    world.destroy(pos, true);
                }
            }
        }
    }
}

/// DragonMeleeGeometry.findForwardTargets uses the rider's look vector and
/// distance to the nearest point on each target's bounding box.
fn ridden_targets(
    world: World<'_>,
    mob: Entity<'_>,
    rider: Entity<'_>,
    attack: Attack,
) -> Vec<i32> {
    let (Some(entities), Some((origin, direction))) = (ENTITIES.get(), ridden_geometry(mob, rider))
    else {
        return Vec::new();
    };
    let (range, horizontal, vertical, close) = match attack {
        Attack::Bite => (6.0, 3.0, 1.35, 2.75),
        Attack::Bite2 => (5.0, 2.4, 1.55, 2.5),
        Attack::Horn => (5.0, 5.0, 5.0, 3.0),
        Attack::Claw | Attack::Barrage => (6.5, 3.0, 4.0, 2.6),
        Attack::Tail => (9.8, 4.0, 7.0, 3.92),
        Attack::Shift => return Vec::new(),
    };
    let end: [f64; 3] = std::array::from_fn(|i| origin[i] + direction[i] * range);
    let inflate = [horizontal, vertical, horizontal];
    let min = std::array::from_fn(|i| origin[i].min(end[i]) - inflate[i]);
    let max = std::array::from_fn(|i| origin[i].max(end[i]) + inflate[i]);
    let mut ids: Vec<_> = entities
        .entities_in(world, min, max, mob.id())
        .into_iter()
        .filter_map(|id| {
            entities
                .with_entity(world, id, |e| {
                    if !enemy(world, mob, e) {
                        return None;
                    }
                    let b = e.body()?;
                    let toward =
                        std::array::from_fn(|i| origin[i].clamp(b.min[i], b.max[i]) - origin[i]);
                    let gap = flight::length_squared(toward).sqrt();
                    // All released angles are at least 90 degrees; the explicit
                    // positive-dot requirement is stricter than their cosine limits.
                    (gap <= range + horizontal
                        && (gap <= close
                            || toward
                                .iter()
                                .zip(direction)
                                .map(|(a, b)| a * b)
                                .sum::<f64>()
                                > 0.0))
                        .then_some((gap, id))
                })
                .flatten()
        })
        .collect();
    ids.sort_by(|a, b| a.0.total_cmp(&b.0));
    ids.into_iter().map(|(_, id)| id).collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tailguard_startup_hold_release_and_cooldown_tracks() {
        let guard = Guard::Startup(17);
        assert_eq!(guard.advance(16), Some(guard));
        assert_eq!(guard.advance(17), Some(Guard::Hold(400)));
        assert_eq!(guard.release(5), Guard::Cancel(13));
        assert_eq!(Guard::Hold(400).advance(400), Some(Guard::Cancel(408)));
        assert_eq!(Guard::Cancel(408).advance(408), None);
        assert_eq!(Guard::Parry(100).release(90), Guard::Parry(100));
    }
    #[test]
    fn phase_specific_rider_bindings() {
        let phase_one = Saved::default();
        let phase_two = Saved {
            phase_two: true,
            ..phase_one
        };
        assert_eq!(
            attack_for("ability_tertiary", phase_one, false),
            Some(Attack::Tail)
        );
        assert_eq!(attack_for("ability_tertiary", phase_one, true), None);
        assert_eq!(
            attack_for("ability_tertiary", phase_two, true),
            Some(Attack::Claw)
        );
        assert_eq!(
            attack_for("ability_primary", phase_two, false),
            Some(Attack::Barrage)
        );
        assert_eq!(attack_for("attack", phase_two, false), Some(Attack::Bite2));
        assert_eq!(
            attack_for(
                "attack",
                Saved {
                    horn_mode: true,
                    ..phase_one
                },
                false
            ),
            Some(Attack::Horn)
        );
    }
    #[test]
    fn live_attack_attribute_and_hunger_scale_released_abilities() {
        assert_eq!(Attack::Horn.scaled_damage(false, 0.25, Some(10.0)), 2.0);
        assert_eq!(Attack::Bite.scaled_damage(false, 0.5, Some(20.0)), 15.0);
        assert_eq!(Attack::Bite2.scaled_damage(true, 0.5, Some(20.0)), 25.0);
        assert_eq!(Attack::Claw.scaled_damage(true, 0.5, Some(20.0)), 12.0);
        assert_eq!(Attack::Tail.scaled_damage(false, 0.5, Some(20.0)), 3.5);
        assert_eq!(Attack::Horn.scaled_damage(false, 0.5, Some(0.0)), 4.0);
    }
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
    #[test]
    fn claw_breaks_waterlogged_solids_but_not_liquids_or_unbreakable_blocks() {
        assert!(claw_breakable("minecraft:oak_fence[waterlogged=true]", 2.0));
        assert!(claw_breakable("minecraft:stone", 30.0));
        assert!(!claw_breakable("minecraft:water[level=2]", 0.0));
        assert!(!claw_breakable("minecraft:lava[level=0]", 0.0));
        assert!(!claw_breakable("minecraft:cave_air", 0.0));
        assert!(!claw_breakable("minecraft:bedrock", -1.0));
        assert!(!claw_breakable("minecraft:obsidian", 50.0));
    }
}
