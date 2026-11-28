use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
};

use moddedmc_api::{content::World, entities::Entity, mob_ai::goal_phase, tick_count};
use serde::{Deserialize, Serialize};

use crate::{ENTITIES, MOB_AI, MOBS, flight, taming};

pub const GOAL: u64 = 11;
pub const DEPTH: u64 = 520;
const DATA: &str = "saintsdragons:roost";
const ANCHOR_TAG: &str = "saintsdragons.sleep_anchor";
const CHECK_INTERVAL: u64 = 19;
const SETTLE_TICKS: u64 = 60;
const PRESSURE_DATA: &str = "saintsdragons:sleep";
const READY_PRESSURE: f32 = 65.0;
const CRITICAL_PRESSURE: f32 = 92.0;
const RESTED_PRESSURE: f32 = 15.0;

#[derive(Clone, Copy, Deserialize, Serialize)]
struct Pressure {
    #[serde(rename = "SleepPressure")]
    value: f32,
    age: i64,
    delay: i64,
}

impl Pressure {
    fn initial(preferred: bool) -> Self {
        Self {
            value: if preferred {
                READY_PRESSURE + crate::ai::random(10_000) as f32 / 1000.0
            } else {
                crate::ai::random(20_000) as f32 / 1000.0
            },
            age: 0,
            delay: if preferred {
                60 + crate::ai::random(21) as i64
            } else {
                100 + crate::ai::random(201) as i64
            },
        }
    }

    fn read(mob: Entity<'_>) -> Option<Self> {
        fastnbt::from_bytes(&mob.data(PRESSURE_DATA)?).ok()
    }

    fn tick(&mut self, clip: Option<&str>, preferred: bool, hunger: i32, health: f32) {
        self.age = self.age.saturating_add(1);
        self.delay = (self.delay - 1).max(0);
        if matches!(clip, Some("sleep" | "fall_asleep")) {
            self.value = (self.value - 100.0 / (20.0 * 60.0 * 6.0)).max(0.0);
        } else if clip != Some("wake_up") {
            let mut rate = 100.0 / (20.0 * 60.0 * 15.0);
            if preferred {
                rate *= 2.0;
            }
            if health < 0.5 {
                rate *= 1.0 + (0.5 - health);
            }
            if hunger < 35 {
                rate *= 0.5;
            }
            self.value = (self.value + rate).min(100.0);
        }
    }

    fn ready(self, preferred: bool, hunger: i32, health: f32) -> bool {
        let threshold = READY_PRESSURE + if hunger < 35 { 15.0 } else { 0.0 }
            - if health < 0.35 { 10.0 } else { 0.0 };
        self.delay == 0
            && self.value >= threshold.clamp(45.0, CRITICAL_PRESSURE)
            && (preferred || self.value >= CRITICAL_PRESSURE)
    }
}

fn preferred_time(name: &str, night: bool) -> bool {
    if matches!(name, "raevyx" | "volitans") {
        !night
    } else {
        night
    }
}

fn health_ratio(mob: Entity<'_>) -> f32 {
    MOBS.get().and_then(|m| m.health(mob)).map_or(
        1.0,
        |(hp, max)| {
            if max <= 0.0 { 1.0 } else { hp / max }
        },
    )
}

fn depth_at(world: World<'_>, at: [f64; 3], height: f64) -> bool {
    let top = at[1] + height;
    let mut pos = flight::cell([at[0], top, at[2]]);
    let last = (top + 10.0).ceil() as i32;
    while pos.y < last {
        if !crate::eggs::water_at(world, pos) {
            return false;
        }
        pos.y += 1;
    }
    true
}

fn deep_water(world: World<'_>, mob: Entity<'_>) -> bool {
    let (Some(view), Some(body)) = (mob.view(), mob.body()) else {
        return false;
    };
    depth_at(world, view.position, body.max[1] - view.position[1])
}

#[derive(Clone, Copy, Default)]
struct Depth {
    target: Option<[f64; 3]>,
    retry: u64,
    seen: u64,
}
static DEPTHS: Mutex<Option<HashMap<i32, Depth>>> = Mutex::new(None);

fn depth_column(
    world: World<'_>,
    origin: moddedmc_api::Pos,
    height: f64,
    width: f64,
) -> Option<[f64; 3]> {
    let water = |x, y, z| crate::eggs::water_at(world, moddedmc_api::Pos { x, y, z });
    let clear = |x, y, z| {
        world
            .collision_shape(moddedmc_api::Pos { x, y, z })
            .is_some_and(|shape| shape.is_empty())
    };
    let (x, z, y) = (origin.x, origin.z, origin.y);
    if !water(x, y, z) {
        return None;
    }
    let surface = (y..=y + 8).find(|&cy| !water(x, cy, z)).unwrap_or(y + 9);
    let bottom = (y - 24..=y)
        .rev()
        .find(|&cy| !water(x, cy, z) || !clear(x, cy, z))
        .map_or(y - 24, |cy| cy + 1);
    let h = (height.ceil() as i32).max(1);
    let radius = ((width * 0.5 - 0.25).ceil() as i32).max(0);
    for cy in (bottom + 2..=(y - 1).min(surface - h - 10)).rev() {
        let at = [f64::from(x) + 0.5, f64::from(cy) + 0.5, f64::from(z) + 0.5];
        if depth_at(world, at, height)
            && (0..h).all(|dy| {
                (-radius..=radius).all(|dx| {
                    (-radius..=radius).all(|dz| {
                        dx * dx + dz * dz > radius * radius
                            || (water(x + dx, cy + dy, z + dz) && clear(x + dx, cy + dy, z + dz))
                    })
                })
            })
        {
            return Some(at);
        }
    }
    None
}

pub fn depth_goal(phase: u32, world: World<'_>, mob: Entity<'_>) -> bool {
    let (Some(ai), Some(mobs), Some(view), Some(body)) =
        (MOB_AI.get(), MOBS.get(), mob.view(), mob.body())
    else {
        return false;
    };
    let now = tick_count();
    let mut all = DEPTHS.lock().unwrap_or_else(PoisonError::into_inner);
    let state = all.get_or_insert_default().entry(mob.id()).or_default();
    state.seen = now;
    if phase == goal_phase::STOP {
        state.target = None;
        state.retry = now + 20;
        ai.stop(mob);
        return false;
    }
    let eligible = !taming::is_tamed(mob)
        && !crate::eggs::is_juvenile(mob)
        && !flight::active(mob.id())
        && ai.target(mob).is_none()
        && !crate::hunt::active(mob.id())
        && !suppressed(mob.id(), now)
        && mobs.passengers(mob).is_empty()
        && mob.vehicle().is_none()
        && crate::eggs::water_at(world, flight::cell(view.position))
        && Pressure::read(mob).is_some_and(|p| {
            p.age >= 600 && p.ready(!night(world), crate::hunger::level(mob), health_ratio(mob))
        })
        && !deep_water(world, mob);
    if !eligible {
        return false;
    }
    let find = || {
        let origin = flight::cell(view.position);
        let column = |pos| {
            depth_column(
                world,
                pos,
                body.max[1] - body.min[1],
                body.max[0] - body.min[0],
            )
        };
        column(origin).or_else(|| {
            (0..24).find_map(|_| {
                column(moddedmc_api::Pos {
                    x: origin.x + crate::ai::random(29) as i32 - 14,
                    z: origin.z + crate::ai::random(29) as i32 - 14,
                    ..origin
                })
            })
        })
    };
    match phase {
        goal_phase::CAN_START => {
            if now < state.retry {
                return false;
            }
            state.target = find();
            if state.target.is_none() {
                state.retry = now + 80;
            }
            state.target.is_some()
        }
        goal_phase::CONTINUE => state
            .target
            .is_some_and(|at| flight::length_squared(flight::offset(at, view.position)) > 9.0),
        goal_phase::START => {
            ai.stop(mob);
            false
        }
        goal_phase::TICK => {
            if let Some(at) = state.target {
                let delta = flight::offset(at, view.position);
                let motion =
                    delta.map(|v| v * 0.16 / flight::length_squared(delta).sqrt().max(0.16));
                if mob.can_move_by(motion)
                    && flight::clear_body(world, body, motion)
                    && crate::eggs::water_at(
                        world,
                        flight::cell(flight::add(view.position, motion)),
                    )
                {
                    ai.look_at(mob, at);
                    mob.change(&moddedmc_api::entities::EntityChange {
                        mask: moddedmc_api::entities::SET_MOTION,
                        motion,
                        ..Default::default()
                    });
                } else {
                    state.target = find();
                }
            }
            false
        }
        _ => false,
    }
}

static ACTIVE: Mutex<Option<HashMap<i32, Return>>> = Mutex::new(None);
static SUPPRESSED: Mutex<Option<HashMap<i32, u64>>> = Mutex::new(None);
static OWNER_SLEEP: Mutex<Option<HashMap<i32, OwnerSleep>>> = Mutex::new(None);

struct OwnerSleep {
    since: u64,
    fall: u64,
    wake: u64,
    waking: Option<u64>,
    seen: u64,
    exhaustion: bool,
}

fn owner_durations(name: &str) -> Option<(u64, u64)> {
    Some(match name {
        "cindervane" => (33, 33),
        "raevyx" => (50, 53),
        "ignivorus" => (38, 38),
        "varasuchus" => (42, 129),
        "volitans" => (51, 43),
        "stegonaut" => (38, 38),
        "atroxiia" => (38, 42),
        _ => return None,
    })
}

fn owner_clip(id: i32) -> Option<&'static str> {
    let lock = OWNER_SLEEP.lock().unwrap_or_else(PoisonError::into_inner);
    let state = lock.as_ref()?.get(&id)?;
    Some(if state.waking.is_some() {
        "wake_up"
    } else if tick_count().saturating_sub(state.since) < state.fall {
        "fall_asleep"
    } else {
        "sleep"
    })
}

/// DragonSleepComponent.tickBrainDecisions. Roost residents use the same saved pressure,
/// but their movement and settling remain in the roost goal.
pub fn owner_tick(world: World<'_>, mob: Entity<'_>, name: &str) {
    let Some((fall, wake)) = owner_durations(name) else {
        return;
    };
    let (Some(mobs), Some(ai), Some(entities), Some(view)) =
        (MOBS.get(), MOB_AI.get(), ENTITIES.get(), mob.view())
    else {
        return;
    };
    let now = tick_count();
    let is_night = night(world);
    let preferred = preferred_time(name, is_night);
    let health = health_ratio(mob);
    let hunger = crate::hunger::level(mob);
    let mut pressure = Pressure::read(mob).unwrap_or_else(|| Pressure::initial(preferred));
    pressure.tick(sleep_clip(mob.id()), preferred, hunger, health);
    if let Ok(nbt) = fastnbt::to_bytes(&pressure) {
        mob.set_data(PRESSURE_DATA, &nbt);
    }
    let tamed = taming::is_tamed(mob);
    if !tamed && mob.data(DATA).is_some() && Profile::for_mob(mob).is_some() {
        return;
    }
    let owner_asleep = taming::owner(mob)
        .and_then(|uuid| mobs.by_uuid(world, &uuid))
        .and_then(|id| {
            entities.with_entity(world, id, |owner| {
                owner.flags() & moddedmc_api::entities::ENTITY_ALIVE != 0
                    && mobs.player_sleeping(owner) == Some(true)
            })
        })
        == Some(true);
    let fluid = world.fluid_id(flight::cell(view.position));
    let wet = matches!(
        fluid.as_deref(),
        Some("minecraft:water" | "minecraft:flowing_water")
    );
    let current = owner_clip(mob.id());
    let recent_combat = world.game_time().is_some_and(|now| {
        mobs.last_hurt_by_mob(mob)
            .is_some_and(|(_, stamp)| (0..600).contains(&now.saturating_sub(stamp)))
    });
    let can_sleep = ai.target(mob).is_none()
        && !recent_combat
        && !crate::hunt::active(mob.id())
        && !suppressed(mob.id(), now)
        && mobs.passengers(mob).is_empty()
        && mob.vehicle().is_none()
        && (current.is_some() || !flight::active(mob.id()))
        && (!wet || name == "volitans")
        && (name != "volitans" || (pressure.age >= 600 && (!wet || deep_water(world, mob))))
        && (!matches!(name, "atroxiia" | "stegonaut" | "ignivorus") || is_night)
        && !matches!(
            fluid.as_deref(),
            Some("minecraft:lava" | "minecraft:flowing_lava")
        )
        && (current.is_some()
            || view.on_ground
            || (name == "volitans" && wet)
            || (tamed && owner_asleep && taming::order_name(mob) == "stay"));
    let wants_sleep = if tamed {
        is_night && owner_asleep
    } else {
        pressure.ready(preferred, hunger, health)
    };
    let mut lock = OWNER_SLEEP.lock().unwrap_or_else(PoisonError::into_inner);
    let all = lock.get_or_insert_default();
    if can_sleep && wants_sleep && !all.contains_key(&mob.id()) {
        all.insert(
            mob.id(),
            OwnerSleep {
                since: now,
                fall,
                wake,
                waking: None,
                seen: now,
                exhaustion: !preferred && pressure.value >= CRITICAL_PRESSURE,
            },
        );
    }
    if let Some(state) = all.get_mut(&mob.id()) {
        state.seen = now;
        if !can_sleep
            || (tamed && !wants_sleep)
            || (!tamed
                && (pressure.value <= RESTED_PRESSURE
                    || (!preferred && !state.exhaustion && pressure.value < CRITICAL_PRESSURE)))
        {
            state.waking.get_or_insert(now);
        }
        if state
            .waking
            .is_some_and(|since| now.saturating_sub(since) >= state.wake)
        {
            all.remove(&mob.id());
            pressure.delay = 40 + crate::ai::random(21) as i64;
            if let Ok(nbt) = fastnbt::to_bytes(&pressure) {
                mob.set_data(PRESSURE_DATA, &nbt);
            }
        } else {
            ai.stop(mob);
            mob.change(&moddedmc_api::entities::EntityChange {
                mask: moddedmc_api::entities::SET_MOTION,
                motion: [0.0; 3],
                ..Default::default()
            });
        }
    }
}

#[derive(Clone, Copy, Deserialize, Serialize)]
struct Roost {
    home: [i32; 3],
    anchor: Option<[i32; 3]>,
}

impl Roost {
    fn save(self, mob: Entity<'_>) {
        if let Ok(nbt) = fastnbt::to_bytes(&self) {
            mob.set_data(DATA, &nbt);
        }
    }

    fn resolve_anchor(&mut self, world: World<'_>, mob: Entity<'_>) {
        let Some(entities) = ENTITIES.get() else {
            return;
        };
        let center = self.home.map(|coordinate| f64::from(coordinate) + 0.5);
        // shortcut: both released anchors are within two blocks of their resident's spawn.
        // Custom templates can fall back to the spawn block if they move the anchor farther away.
        let nearest = entities
            .entities_in(
                world,
                center.map(|coordinate| coordinate - 2.0),
                center.map(|coordinate| coordinate + 2.0),
                mob.id(),
            )
            .into_iter()
            .filter_map(|id| {
                entities.with_entity(world, id, |entity| {
                    if entity.type_id() != "marker" || !entity.has_scoreboard_tag(ANCHOR_TAG) {
                        return None;
                    }
                    let position = entity.view()?.position;
                    let distance = flight::length_squared(flight::offset(position, center));
                    Some((distance, id, block(position)))
                })?
            })
            .min_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
        if let Some((_, _, anchor)) = nearest {
            self.anchor = Some(anchor);
            self.save(mob);
        }
    }
}

#[derive(Clone, Copy)]
struct Profile {
    territory: f64,
    return_radius: f64,
    sleep_radius: f64,
    fall_asleep: u64,
    wake_up: u64,
}

impl Profile {
    fn for_mob(mob: Entity<'_>) -> Option<Self> {
        Some(match MOBS.get()?.kind(mob)? {
            "saintsdragons:ignivorus" => Self {
                territory: 64.0,
                return_radius: 48.0,
                sleep_radius: 6.0,
                fall_asleep: 38,
                wake_up: 38,
            },
            "saintsdragons:varasuchus" => Self {
                territory: 48.0,
                return_radius: 32.0,
                sleep_radius: 3.0,
                fall_asleep: 42,
                wake_up: 129,
            },
            _ => return None,
        })
    }
}

struct Return {
    roost: Roost,
    profile: Profile,
    seen: u64,
    next_path: u64,
    settled_since: Option<u64>,
    sleep_since: Option<u64>,
    waking_since: Option<u64>,
}

impl Return {
    fn wake(&mut self, now: u64, immediate: bool) {
        if immediate {
            self.settled_since = None;
            self.sleep_since = None;
            self.waking_since = None;
        } else if self.clip(now).is_some() {
            self.waking_since.get_or_insert(now);
        } else {
            self.settled_since = None;
            self.sleep_since = None;
        }
    }

    fn clip(&self, now: u64) -> Option<&'static str> {
        if self.waking_since.is_some() {
            return Some("wake_up");
        }
        let elapsed = now.saturating_sub(self.sleep_since?);
        match elapsed {
            _ if elapsed < self.profile.fall_asleep => Some("fall_asleep"),
            _ => Some("sleep"),
        }
    }
}

fn block(position: [f64; 3]) -> [i32; 3] {
    let block = flight::cell(position);
    [block.x, block.y, block.z]
}

fn horizontal_distance(position: [f64; 3], home: [i32; 3]) -> f64 {
    (position[0] - (f64::from(home[0]) + 0.5)).hypot(position[2] - (f64::from(home[2]) + 0.5))
}

fn night(world: World<'_>) -> bool {
    world
        .day_time()
        .is_some_and(|time| !time.fixed_time && time.ticks.rem_euclid(24000) >= 12000)
}

pub fn set_home(mob: Entity<'_>, position: [f64; 3]) {
    Roost {
        home: block(position),
        anchor: None,
    }
    .save(mob);
}

pub fn sleep_clip(id: i32) -> Option<&'static str> {
    if let Some(clip) = owner_clip(id) {
        return Some(clip);
    }
    let active = ACTIVE.lock().unwrap_or_else(PoisonError::into_inner);
    active.as_ref()?.get(&id)?.clip(tick_count())
}

/// DragonSleepComponent.startSleepExit / wakeUpImmediately and suppressSleep.
pub fn wake(mob: Entity<'_>, immediate: bool, suppression: u64) {
    if MOBS
        .get()
        .and_then(|m| m.kind(mob))
        .and_then(|kind| kind.strip_prefix("saintsdragons:"))
        .and_then(owner_durations)
        .is_none()
    {
        return;
    }
    let now = tick_count();
    {
        let mut lock = OWNER_SLEEP.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(all) = lock.as_mut() {
            if immediate {
                all.remove(&mob.id());
            } else if let Some(state) = all.get_mut(&mob.id()) {
                state.waking.get_or_insert(now);
            }
        }
    }
    let mut active = ACTIVE.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(state) = active.as_mut().and_then(|all| all.get_mut(&mob.id())) {
        state.wake(now, immediate);
    }
    let mut suppressed = SUPPRESSED.lock().unwrap_or_else(PoisonError::into_inner);
    let until = suppressed
        .get_or_insert_default()
        .entry(mob.id())
        .or_default();
    *until = (*until).max(now + suppression);
}

fn suppressed(id: i32, now: u64) -> bool {
    SUPPRESSED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .and_then(|all| all.get(&id))
        .is_some_and(|until| now < *until)
}

pub fn forget(id: i32) {
    if let Some(all) = DEPTHS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_mut()
    {
        all.remove(&id);
    }
    if let Some(all) = OWNER_SLEEP
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_mut()
    {
        all.remove(&id);
    }
    if let Some(active) = ACTIVE
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_mut()
    {
        active.remove(&id);
    }
    if let Some(suppressed) = SUPPRESSED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_mut()
    {
        suppressed.remove(&id);
    }
}

pub fn sweep() {
    let now = tick_count();
    if now.is_multiple_of(200)
        && let Some(all) = DEPTHS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_mut()
    {
        all.retain(|_, state| now.saturating_sub(state.seen) < 400);
    }
    if now.is_multiple_of(200)
        && let Some(all) = OWNER_SLEEP
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_mut()
    {
        all.retain(|_, state| now.saturating_sub(state.seen) < 400);
    }
    if now.is_multiple_of(200)
        && let Some(active) = ACTIVE
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_mut()
    {
        active.retain(|_, state| now.saturating_sub(state.seen) < 400);
    }
    if now.is_multiple_of(200)
        && let Some(all) = SUPPRESSED
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_mut()
    {
        all.retain(|_, until| *until > now);
    }
}

pub fn goal(phase: u32, world: World<'_>, mob: Entity<'_>) -> bool {
    let Some(ai) = MOB_AI.get() else { return false };
    let id = mob.id();
    let now = tick_count();
    if phase == goal_phase::CAN_START && !(now + id as u64).is_multiple_of(CHECK_INTERVAL) {
        return false;
    }

    let mut active = ACTIVE.lock().unwrap_or_else(PoisonError::into_inner);
    let active = active.get_or_insert_default();
    if phase == goal_phase::STOP {
        active.remove(&id);
        ai.stop(mob);
        return false;
    }

    let Some(view) = mob.view() else { return false };
    if matches!(phase, goal_phase::CAN_START | goal_phase::CONTINUE)
        && (world.dimension_id() != Some("minecraft:overworld") || taming::is_tamed(mob))
    {
        return false;
    }

    let is_night = night(world);
    if phase == goal_phase::CAN_START {
        if ai.target(mob).is_some() || suppressed(id, now) {
            return false;
        }
        let Some((mut roost, profile)) = mob
            .data(DATA)
            .and_then(|nbt| fastnbt::from_bytes::<Roost>(&nbt).ok())
            .zip(Profile::for_mob(mob))
        else {
            return false;
        };
        if !is_night && horizontal_distance(view.position, roost.home) <= profile.territory {
            return false;
        }
        if roost.anchor.is_none() {
            roost.resolve_anchor(world, mob);
        }
        active.insert(
            id,
            Return {
                roost,
                profile,
                seen: now,
                next_path: now,
                settled_since: None,
                sleep_since: None,
                waking_since: None,
            },
        );
        return true;
    }

    let Some(state) = active.get_mut(&id) else {
        return false;
    };
    match phase {
        goal_phase::CONTINUE => {
            let target = ai.target(mob).is_some();
            let wet = matches!(
                world.fluid_id(flight::cell(view.position)).as_deref(),
                Some("minecraft:water" | "minecraft:flowing_water")
            );
            let rested = Pressure::read(mob).is_some_and(|p| p.value <= RESTED_PRESSURE);
            if state.clip(now).is_some() && (target || wet || !is_night || rested) {
                state.wake(now, wet && !target);
                if target || wet {
                    let mut all = SUPPRESSED.lock().unwrap_or_else(PoisonError::into_inner);
                    let until = all.get_or_insert_default().entry(id).or_default();
                    *until = (*until).max(now + if target { 600 } else { 200 });
                }
            }
            if let Some(wake) = state.waking_since {
                if now.saturating_sub(wake) < state.profile.wake_up {
                    return true;
                }
                state.waking_since = None;
                state.settled_since = None;
                state.sleep_since = None;
            }
            if target || suppressed(id, now) {
                return false;
            }
            is_night
                || horizontal_distance(view.position, state.roost.home)
                    > state.profile.return_radius
        }
        goal_phase::START => {
            ai.stop(mob);
            false
        }
        goal_phase::TICK => {
            state.seen = now;
            if state.waking_since.is_some() {
                ai.stop(mob);
                return false;
            }
            let sleep_site = state.roost.anchor.unwrap_or(state.roost.home);
            let distance = flight::length_squared(flight::offset(
                block(view.position).map(f64::from),
                sleep_site.map(f64::from),
            ));
            if is_night && distance <= state.profile.sleep_radius.powi(2) {
                if ai.navigating(mob) {
                    ai.stop(mob);
                }
                if view.on_ground
                    && world.fluid_id(flight::cell(view.position)).as_deref()
                        == Some("minecraft:empty")
                {
                    let settled = *state.settled_since.get_or_insert(now);
                    if now.saturating_sub(settled) >= SETTLE_TICKS
                        && !crate::hunt::active(id)
                        && Pressure::read(mob).is_some_and(|p| {
                            p.ready(is_night, crate::hunger::level(mob), health_ratio(mob))
                        })
                    {
                        state.sleep_since.get_or_insert(now);
                    }
                } else {
                    state.settled_since = None;
                    state.sleep_since = None;
                }
                return false;
            }

            state.settled_since = None;
            state.sleep_since = None;
            if now >= state.next_path {
                state.next_path = now + 20;
                if is_night && state.roost.anchor.is_none() {
                    state.roost.resolve_anchor(world, mob);
                }
                let [x, y, z] = if is_night {
                    state.roost.anchor.unwrap_or(state.roost.home)
                } else {
                    state.roost.home
                }
                .map(f64::from);
                ai.walk_to(mob, [x + 0.5, y, z + 0.5], 1.0);
            }
            false
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varasuchus_sleep_refusal_covers_both_transitions() {
        let mut state = Return {
            roost: Roost {
                home: [0; 3],
                anchor: None,
            },
            profile: Profile {
                territory: 48.0,
                return_radius: 32.0,
                sleep_radius: 3.0,
                fall_asleep: 42,
                wake_up: 129,
            },
            seen: 0,
            next_path: 0,
            settled_since: Some(100),
            sleep_since: None,
            waking_since: None,
        };
        assert_eq!(state.clip(159), None);
        state.sleep_since = Some(160);
        assert_eq!(state.clip(160), Some("fall_asleep"));
        assert_eq!(state.clip(201), Some("fall_asleep"));
        assert_eq!(state.clip(202), Some("sleep"));
        state.wake(300, false);
        assert_eq!(state.clip(300), Some("wake_up"));
        state.wake(350, false);
        assert_eq!(state.waking_since, Some(300));
        assert_eq!(state.clip(428), Some("wake_up"));
        state.wake(429, true);
        assert_eq!(state.clip(429), None);
    }

    #[test]
    fn pressure_matches_release_thresholds_and_rates() {
        let mut p = Pressure {
            value: 65.0,
            age: 0,
            delay: 0,
        };
        assert!(p.ready(true, 100, 1.0));
        assert!(!p.ready(false, 100, 1.0));
        assert!(!p.ready(true, 34, 1.0));
        p.value = 55.0;
        assert!(p.ready(true, 100, 0.34));
        p.value = 92.0;
        assert!(p.ready(false, 100, 1.0));
        let before = p.value;
        p.tick(None, true, 100, 1.0);
        assert!((p.value - before - 100.0 / 9000.0).abs() < 0.00001);
        let before = p.value;
        p.tick(Some("sleep"), true, 100, 1.0);
        assert!((before - p.value - 100.0 / 7200.0).abs() < 0.00001);
        let bytes = fastnbt::to_bytes(&p).unwrap();
        let saved: Pressure = fastnbt::from_bytes(&bytes).unwrap();
        assert_eq!(saved.value, p.value);
    }
}
