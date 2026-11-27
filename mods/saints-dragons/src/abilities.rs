//! Cindervane's double bite, slash grab, Magma Volley and Fire Body.

use moddedmc_api::{
    content::World,
    entities::{ENTITY_ALIVE, ENTITY_LIVING, Entity, EntityBody, EntityChange, SET_MOTION},
    tick_count,
};

use crate::{
    EFFECTS, ENTITIES, MOB_AI, MOBS,
    flight::{add, length_squared, offset},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Attack {
    Bite,
    DoubleBite,
    SlashGrab,
    MagmaVolley,
}

impl Attack {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Bite => "bite",
            Self::DoubleBite => "double-bite",
            Self::SlashGrab => "slash-grab",
            Self::MagmaVolley => "magma-volley",
        }
    }

    pub const fn timers(self) -> (u64, u64, u64) {
        match self {
            Self::Bite => (12, 20, 24),
            Self::DoubleBite => (16, 28, 80),
            Self::SlashGrab => (35, 40, 140),
            Self::MagmaVolley => (29, 55, 400),
        }
    }
}

const ATTACKS: [Attack; 4] = [
    Attack::Bite,
    Attack::DoubleBite,
    Attack::SlashGrab,
    Attack::MagmaVolley,
];

#[derive(Default)]
pub struct Abilities {
    running: Option<Running>,
    cooldown: [u64; 4],
    cadence: u64,
    fire_until: u64,
    fire_cooldown: u64,
    rider_fire: Option<i32>,
    crash_height: Option<f64>,
    shots: Vec<Shot>,
    pub last: Option<Attack>,
}

struct Running {
    kind: Attack,
    target: i32,
    strike_target: Option<i32>,
    start: u64,
    held: bool,
    rider: Option<i32>,
    first_hits: Vec<i32>,
}

pub struct Shot {
    position: [f64; 3],
    motion: [f64; 3],
    age: u16,
    rider: bool,
}

pub fn enemy(world: World<'_>, mob: Entity<'_>, target: Entity<'_>) -> bool {
    let (Some(mobs), Some(view)) = (MOBS.get(), target.view()) else {
        return false;
    };
    target.id() != mob.id()
        && view.kind & ENTITY_LIVING != 0
        && target.flags() & ENTITY_ALIVE != 0
        && !matches!(mobs.game_mode(target), Some(1 | 3))
        && !crate::allies::is_ally(world, mob, target)
        && !mob.has_indirect_passenger(target.id())
}

impl Abilities {
    pub const fn busy(&self) -> bool {
        self.running.is_some()
    }

    pub fn active(&self) -> Option<Attack> {
        self.running.as_ref().map(|action| action.kind)
    }

    pub const fn holding(&self, id: i32) -> bool {
        matches!(&self.running, Some(action) if action.held && action.target == id)
    }

    pub fn fire_active(&self) -> bool {
        tick_count() < self.fire_until
    }

    pub fn suppress_fire(&mut self) {
        self.fire_until = 0;
        self.rider_fire = None;
        self.crash_height = None;
        self.fire_cooldown = self.fire_cooldown.max(tick_count() + 100);
    }

    pub fn choose(&mut self, now: u64, target: i32, gap: f64, facing: bool) -> bool {
        if gap <= 3.25 && now >= self.fire_cooldown {
            self.fire_until = now + 100;
            self.fire_cooldown = now + 240;
        }
        if !facing || now < self.cadence {
            return false;
        }
        let index = ATTACKS
            .into_iter()
            .enumerate()
            .filter_map(|(i, kind)| {
                if now < self.cooldown[i] {
                    return None;
                }
                let score = match kind {
                    Attack::Bite if gap <= 4.5 => 56.0 + (4.5 - gap) * 3.0,
                    Attack::DoubleBite if gap <= 4.75 => 92.0 + (4.75 - gap) * 6.0,
                    Attack::SlashGrab if (2.0..=6.25).contains(&gap) => {
                        84.0 + if gap >= 3.0 { 8.0 } else { -8.0 }
                    }
                    Attack::MagmaVolley if (8.0..=32.0).contains(&gap) => {
                        68.0 + (gap - 8.0).mul_add(0.8, 0.0).min(18.0)
                    }
                    _ => return None,
                };
                Some((i, score - if self.last == Some(kind) { 26.0 } else { 0.0 }))
            })
            .max_by(|(_, a), (_, b)| a.total_cmp(b))
            .map(|(i, _)| i);
        let Some(i) = index else { return false };
        let kind = ATTACKS[i];
        let (_, cadence, cooldown) = kind.timers();
        self.running = Some(Running {
            kind,
            target,
            strike_target: None,
            start: now,
            held: false,
            rider: None,
            first_hits: Vec::new(),
        });
        self.cadence = now + cadence;
        self.cooldown[i] = now + cooldown;
        self.last = Some(kind);
        true
    }

    pub fn cancel(&mut self, world: World<'_>, mob: Entity<'_>) {
        if let Some(action) = self.running.take()
            && action.held
            && action.rider.is_none()
        {
            release(world, mob, action.target, false);
        }
        if self.rider_fire.is_none() {
            self.fire_until = 0;
        }
    }

    /// The release starts its cooldown when the ability ends.
    pub fn start_rider(&mut self, now: u64, rider: i32, kind: Attack) -> bool {
        let index = ATTACKS.iter().position(|&a| a == kind).unwrap();
        if self.busy() || now < self.cooldown[index] {
            return false;
        }
        let (duration, cooldown) = rider_timers(kind);
        self.cooldown[index] = now + duration + cooldown;
        self.running = Some(Running {
            kind,
            target: -1,
            strike_target: None,
            start: now,
            held: false,
            rider: Some(rider),
            first_hits: Vec::new(),
        });
        self.last = Some(kind);
        true
    }

    pub fn rider_attack(&self) -> bool {
        self.running.as_ref().is_some_and(|a| a.rider.is_some())
    }

    pub fn start_fire(&mut self, now: u64, rider: i32) -> bool {
        if now < self.fire_until || now < self.fire_cooldown {
            return false;
        }
        self.rider_fire = Some(rider);
        self.fire_until = now + 1001;
        true
    }

    pub fn stop_fire(&mut self, now: u64) {
        if self.rider_fire.take().is_some() {
            self.fire_until = 0;
            self.fire_cooldown = now + 40;
            self.crash_height = None;
        }
    }

    pub fn rider_tick(&mut self, world: World<'_>, mob: Entity<'_>, rider: Option<i32>) {
        let now = tick_count();
        if self.rider_attack() {
            if self.running.as_ref().and_then(|a| a.rider) == rider {
                self.tick_attack(world, mob);
            } else {
                self.running = None;
            }
        }
        if let Some(owner) = self.rider_fire {
            let Some(view) = mob.view() else { return };
            if rider != Some(owner)
                || now >= self.fire_until
                || world
                    .fluid_id(crate::flight::cell(view.position))
                    .is_some_and(|f| f.ends_with("water"))
            {
                self.stop_fire(now);
                return;
            }
            rider_fire_aura(world, mob);
            let forward_crash = if !view.on_ground {
                let horizontal = view.motion[0].hypot(view.motion[2]);
                let (sin, cos) = f64::from(view.yaw).to_radians().sin_cos();
                let direction = if horizontal > 0.01 {
                    [
                        view.motion[0] / horizontal,
                        0.0,
                        view.motion[2] / horizontal,
                    ]
                } else {
                    [-sin, 0.0, cos]
                };
                let origin = add(view.position, [0.0, 1.375, 0.0]);
                let reach = (2.5 * 0.75 + horizontal * 2.0 + 0.75).clamp(1.5, 4.0);
                world
                    .raycast(
                        origin,
                        add(origin, direction.map(|v| v * reach)),
                        Some(mob.id()),
                    )
                    .and_then(|(block, _)| block)
                    .map(|b| {
                        [
                            f64::from(b.x) + 0.5,
                            f64::from(b.y) + 0.5,
                            f64::from(b.z) + 0.5,
                        ]
                    })
            } else {
                None
            };
            if let Some(impact) = forward_crash {
                fire_crash(world, mob, impact);
                self.stop_fire(now);
                self.running = None;
            } else if !view.on_ground {
                self.crash_height = Some(
                    self.crash_height
                        .map_or(view.position[1], |h| h.max(view.position[1])),
                );
            } else if self
                .crash_height
                .take()
                .is_some_and(|h| h - view.position[1] >= 7.0)
            {
                fire_crash(world, mob, view.position);
                self.stop_fire(now);
                self.running = None;
            }
        }
    }

    pub fn tick_attack(&mut self, world: World<'_>, mob: Entity<'_>) {
        let Some(action) = self.running.as_mut() else {
            return;
        };
        let age = tick_count().saturating_sub(action.start);
        if let Some(rider) = action.rider {
            match (action.kind, age) {
                (Attack::Bite, 5) => {
                    rider_bite(world, mob, 12.0, &mut action.first_hits, false);
                }
                (Attack::DoubleBite, 4 | 11) => {
                    rider_bite(world, mob, 15.0, &mut action.first_hits, age == 11)
                }
                (Attack::SlashGrab, 16) => {
                    action.target = rider_grab_candidate(world, mob).unwrap_or(-1);
                    rider_slash(world, mob, action.target, 5.0);
                }
                (Attack::SlashGrab, 26) => {
                    rider_slash(world, mob, action.target, 7.0);
                    if let Some(entities) = ENTITIES.get() {
                        entities.with_entity(world, action.target, |target| {
                            target.change(&EntityChange {
                                mask: SET_MOTION,
                                motion: [0.0, -0.65, 0.0],
                                ..EntityChange::default()
                            });
                        });
                    }
                }
                (Attack::SlashGrab, 17..=25) => hold_rider_grab(world, mob, action.target),
                (Attack::MagmaVolley, 3 | 13 | 23) => {
                    self.shots.extend(rider_volley(world, mob, rider));
                    crate::audio::volley(world, mob);
                }
                _ => {}
            }
            if age >= rider_timers(action.kind).0 {
                self.running = None;
            }
            return;
        }
        if action.kind == Attack::SlashGrab && age <= 26 {
            // The release retains its first living strike candidate throughout startup,
            // even if it leaves the slash box before either hit.
            if action
                .strike_target
                .is_none_or(|id| !strike_alive(world, mob, id))
            {
                action.strike_target = strike_candidate(world, mob);
            }
        }
        match (action.kind, age) {
            (Attack::Bite, 5) => melee(world, mob, action.target, 12.0, 4.5, false),
            (Attack::DoubleBite, 4 | 11) => melee(world, mob, action.target, 15.0, 4.75, age == 11),
            (Attack::SlashGrab, 16) => {
                let grab = rider_grab_candidate(world, mob);
                action.target = grab.or(action.strike_target).unwrap_or(-1);
                action.held = slash_primary(world, mob, action.target, 5.0, grab.is_some(), false);
                slash(world, mob, action.target, 5.0, false);
            }
            (Attack::SlashGrab, 26) => {
                let excluded = if action.held {
                    release(world, mob, action.target, true);
                    action.target
                } else {
                    let id = action.strike_target.unwrap_or(-1);
                    slash_primary(world, mob, id, 7.0, false, true);
                    id
                };
                slash(world, mob, excluded, 7.0, true);
                action.held = false;
            }
            (Attack::MagmaVolley, 3 | 13 | 23) => {
                let shots = volley(world, mob, action.target);
                if !shots.is_empty() {
                    crate::audio::volley(world, mob);
                    self.shots.extend(shots);
                }
            }
            _ => {}
        }
        if age >= action.kind.timers().0 {
            self.running = None;
        }
    }

    pub fn take_effects(&mut self) -> (Vec<Shot>, bool) {
        (
            std::mem::take(&mut self.shots),
            self.fire_active() && self.rider_fire.is_none(),
        )
    }

    pub fn keep_shots(&mut self, shots: Vec<Shot>) {
        self.shots = shots;
    }
}

fn rider_timers(kind: Attack) -> (u64, u64) {
    match kind {
        Attack::Bite => (12, 15),
        Attack::DoubleBite => (20, 20),
        Attack::SlashGrab => (40, 7),
        Attack::MagmaVolley => (29, 400),
    }
}

fn rider_bite(
    world: World<'_>,
    mob: Entity<'_>,
    damage: f32,
    first_hits: &mut Vec<i32>,
    repeat: bool,
) {
    let (Some(entities), Some(ai), Some(mobs), Some(view)) =
        (ENTITIES.get(), MOB_AI.get(), MOBS.get(), mob.view())
    else {
        return;
    };
    let (sin, cos) = f64::from(view.body_yaw).to_radians().sin_cos();
    let range = if view.on_ground { 4.0 } else { 4.6 };
    // CindervaneBiteAbility.selectTargets: body-forward swept box, using the released width.
    let origin = add(view.position, [-sin * 5.875, 1.3, cos * 5.875]);
    let end = add(origin, [-sin * range, 0.0, cos * range]);
    let min = std::array::from_fn(|i| origin[i].min(end[i]) - range);
    let max = std::array::from_fn(|i| origin[i].max(end[i]) + range);
    for id in entities.entities_in(world, min, max, mob.id()) {
        entities.with_entity(world, id, |target| {
            if !enemy(world, mob, target) {
                return;
            }
            if repeat && first_hits.contains(&id) {
                mobs.reset_hurt_cooldown(target);
            }
            if ai.attack(
                world,
                mob,
                id,
                damage * crate::hunger::melee_multiplier(mob),
            ) && !repeat
            {
                first_hits.push(id);
            }
            if let Some(at) = target.view() {
                target.change(&EntityChange {
                    mask: SET_MOTION,
                    motion: add(
                        at.motion,
                        [
                            -sin * 0.3,
                            if view.on_ground { 0.05 } else { 0.15 },
                            cos * 0.3,
                        ],
                    ),
                    ..EntityChange::default()
                });
            }
        });
    }
}

fn rider_grab_candidate(world: World<'_>, mob: Entity<'_>) -> Option<i32> {
    let (entities, mobs, view) = (ENTITIES.get()?, MOBS.get()?, mob.view()?);
    let (sin, cos) = f64::from(view.yaw).to_radians().sin_cos();
    let center = add(
        view.position,
        [-sin * 5.25 - cos * 1.6, 1.15, cos * 5.25 - sin * 1.6],
    );
    entities
        .entities_in(
            world,
            add(center, [-3.0; 3]),
            add(center, [3.0; 3]),
            mob.id(),
        )
        .into_iter()
        .filter_map(|id| {
            entities
                .with_entity(world, id, |target| {
                    let body = target.body()?;
                    let at = target.view()?;
                    (enemy(world, mob, target)
                        && target.vehicle().is_none()
                        && mobs.passengers(target).is_empty()
                        && !mobs
                            .kind(target)
                            .is_some_and(|k| k.starts_with("saintsdragons:"))
                        && (0..3).all(|i| body.max[i] - body.min[i] <= 4.3))
                    .then_some((id, length_squared(offset(at.position, center))))
                })
                .flatten()
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(id, _)| id)
}

fn hold_rider_grab(world: World<'_>, mob: Entity<'_>, id: i32) {
    let (Some(entities), Some(view)) = (ENTITIES.get(), mob.view()) else {
        return;
    };
    let (sin, cos) = f64::from(view.yaw).to_radians().sin_cos();
    let hold = add(view.position, [-cos * 1.2 - sin, 1.0, -sin * 1.2 + cos]);
    entities.with_entity(world, id, |target| {
        if let Some(at) = target.view() {
            target.change(&EntityChange {
                mask: SET_MOTION,
                motion: offset(hold, at.position),
                ..EntityChange::default()
            });
            target.reset_fall_distance();
        }
    });
}

fn rider_slash(world: World<'_>, mob: Entity<'_>, primary: i32, damage: f32) {
    if let (Some(ai), Some(entities)) = (MOB_AI.get(), ENTITIES.get()) {
        entities.with_entity(world, primary, |target| {
            if enemy(world, mob, target) {
                ai.attack(
                    world,
                    mob,
                    primary,
                    damage * crate::hunger::melee_multiplier(mob),
                );
            }
        });
    }
    slash(world, mob, primary, damage, true);
}

fn rider_volley(world: World<'_>, mob: Entity<'_>, rider: i32) -> Vec<Shot> {
    let (Some(entities), Some(view)) = (ENTITIES.get(), mob.view()) else {
        return Vec::new();
    };
    let look = entities
        .with_entity(world, rider, |p| p.view())
        .flatten()
        .unwrap_or(view);
    let direction = |yaw: f64, pitch: f64| {
        let (s, c) = yaw.to_radians().sin_cos();
        let (sp, cp) = pitch.to_radians().sin_cos();
        [-s * cp, -sp, c * cp]
    };
    let forward = direction(f64::from(look.yaw), f64::from(look.pitch));
    let origin = add(
        add(view.position, [0.0, 2.75, 0.0]),
        forward.map(|v| v * 1.875),
    );
    [-9.5, 0.0, 9.5]
        .into_iter()
        .map(|spread| {
            let jitter = |range: f64| (crate::ai::random(1001) as f64 / 1000.0 - 0.5) * range;
            let d = direction(
                f64::from(look.yaw) + spread + jitter(6.0),
                f64::from(look.pitch) + jitter(4.0),
            );
            Shot {
                position: add(origin, d.map(|v| v * 5.0)),
                motion: add(d.map(|v| v * 0.55), [0.0, -0.15, 0.0]),
                age: 0,
                rider: true,
            }
        })
        .collect()
}

fn rider_fire_aura(world: World<'_>, mob: Entity<'_>) {
    let (Some(entities), Some(mobs), Some(view)) = (ENTITIES.get(), MOBS.get(), mob.view()) else {
        return;
    };
    for id in entities.entities_in(
        world,
        add(view.position, [-6.5, -3.25, -6.5]),
        add(view.position, [6.5, 6.0, 6.5]),
        mob.id(),
    ) {
        entities.with_entity(world, id, |target| {
            if crate::allies::is_ally(world, mob, target) || mob.has_indirect_passenger(id) {
                target.add_effect("minecraft:fire_resistance", 60, 0, true, false);
                target.add_effect("minecraft:resistance", 40, 4, true, false);
                mobs.set_on_fire(target, 0);
            }
        });
    }
    for id in entities.entities_in(
        world,
        add(view.position, [-4.75, -2.5, -4.75]),
        add(view.position, [4.75, 5.0, 4.75]),
        mob.id(),
    ) {
        entities.with_entity(world, id, |target| {
            if !enemy(world, mob, target) || fire_immune(target) {
                return;
            }
            target.hurt("minecraft:dragon_breath", 3.0);
            mobs.set_on_fire(target, 80);
            if let Some(at) = target.view() {
                let delta = offset(at.position, add(view.position, [0.0, 1.25, 0.0]));
                let length = length_squared(delta).sqrt();
                if length > 0.01 {
                    target.change(&EntityChange {
                        mask: SET_MOTION,
                        motion: add(
                            at.motion,
                            [delta[0] / length * 0.15, 0.05, delta[2] / length * 0.15],
                        ),
                        ..EntityChange::default()
                    });
                }
            }
        });
    }
    if tick_count().is_multiple_of(5)
        && let Some(effects) = EFFECTS.get()
    {
        effects.particles(
            world,
            "minecraft:flame",
            add(view.position, [0.0, 1.0, 0.0]),
            [1.3, 0.8, 1.3],
            0.06,
            12,
        );
    }
}

fn fire_immune(entity: Entity<'_>) -> bool {
    crate::TAGS.get().is_some_and(|tags| {
        tags.contains(
            "entity_type",
            "saintsdragons:immune_to/fire",
            MOBS.get()
                .and_then(|m| m.kind(entity))
                .unwrap_or_else(|| entity.type_id()),
        )
    })
}

fn fire_crash(world: World<'_>, mob: Entity<'_>, pos: [f64; 3]) {
    // shortcut: entity blast only; the release's terrain imprint and griefing-dependent explosion are pending.
    if let (Some(entities), Some(mobs)) = (ENTITIES.get(), MOBS.get()) {
        for id in entities.entities_in(world, add(pos, [-17.5; 3]), add(pos, [17.5; 3]), mob.id()) {
            entities.with_entity(world, id, |target| {
                if enemy(world, mob, target)
                    && !fire_immune(target)
                    && target
                        .hurt_by(mob.id(), "minecraft:player_explosion", 200.0)
                        .unwrap_or(false)
                {
                    mobs.set_on_fire(target, 160);
                }
            });
        }
    }
    mob.hurt_by(mob.id(), "minecraft:player_explosion", 40.0);
    if let Some(effects) = EFFECTS.get() {
        effects.particles(world, "minecraft:explosion", pos, [2.0; 3], 0.0, 30);
    }
}

fn melee(world: World<'_>, mob: Entity<'_>, id: i32, damage: f32, reach: f64, repeat: bool) {
    let (Some(ai), Some(mobs), Some(entities), Some(view)) =
        (MOB_AI.get(), MOBS.get(), ENTITIES.get(), mob.view())
    else {
        return;
    };
    entities.with_entity(world, id, |target| {
        let (Some(at), Some(body)) = (target.view(), target.body()) else {
            return;
        };
        let gap = length_squared(offset(at.position, view.position)).sqrt()
            - (2.5 + body.max[0] - body.min[0]) * 0.5;
        if !enemy(world, mob, target) || gap > reach {
            return;
        }
        if repeat {
            mobs.reset_hurt_cooldown(target);
        }
        if ai.attack(
            world,
            mob,
            id,
            damage * crate::hunger::melee_multiplier(mob),
        ) {
            let angle = f64::from(view.yaw).to_radians();
            target.change(&EntityChange {
                mask: SET_MOTION,
                motion: add(at.motion, [-angle.sin() * 0.3, 0.05, angle.cos() * 0.3]),
                ..EntityChange::default()
            });
        }
    });
}

fn strike_alive(world: World<'_>, mob: Entity<'_>, id: i32) -> bool {
    ENTITIES.get().is_some_and(|entities| {
        entities.with_entity(world, id, |target| {
            enemy(world, mob, target) && target.vehicle().is_none()
        }) == Some(true)
    })
}

fn strike_candidate(world: World<'_>, mob: Entity<'_>) -> Option<i32> {
    let (entities, view) = (ENTITIES.get()?, mob.view()?);
    let (sin, cos) = f64::from(view.yaw).to_radians().sin_cos();
    let center = add(
        view.position,
        [-sin * 5.25 - cos * 1.6, 1.15, cos * 5.25 - sin * 1.6],
    );
    entities
        .entities_in(
            world,
            add(center, [-3.0; 3]),
            add(center, [3.0; 3]),
            mob.id(),
        )
        .into_iter()
        .filter_map(|id| {
            entities
                .with_entity(world, id, |target| {
                    let at = target.view()?;
                    (enemy(world, mob, target) && target.vehicle().is_none())
                        .then_some((id, length_squared(offset(at.position, center))))
                })
                .flatten()
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(id, _)| id)
}

fn slash_primary(
    world: World<'_>,
    mob: Entity<'_>,
    id: i32,
    damage: f32,
    grab: bool,
    repeat: bool,
) -> bool {
    let (Some(ai), Some(entities), Some(mobs)) = (MOB_AI.get(), ENTITIES.get(), MOBS.get()) else {
        return false;
    };
    entities
        .with_entity(world, id, |target| {
            if !enemy(world, mob, target) || target.vehicle().is_some() {
                return false;
            }
            if repeat {
                mobs.reset_hurt_cooldown(target);
            }
            let hit = ai.attack(
                world,
                mob,
                id,
                damage * crate::hunger::melee_multiplier(mob),
            );
            if grab && hit
                && target.flags() & ENTITY_ALIVE != 0
                && mobs.passengers(mob).is_empty()
                && mobs.passengers(target).is_empty()
            {
                crate::taming::configure_seats(mob, crate::CINDERVANE, true);
                target.start_riding(mob.id(), true)
            } else {
                false
            }
        })
        .unwrap_or(false)
}

fn slash(world: World<'_>, mob: Entity<'_>, primary: i32, damage: f32, repeat: bool) {
    let (Some(ai), Some(entities), Some(mobs), Some(view)) =
        (MOB_AI.get(), ENTITIES.get(), MOBS.get(), mob.view())
    else {
        return;
    };
    let (sin, cos) = f64::from(view.yaw).to_radians().sin_cos();
    let center = add(
        view.position,
        [-sin * 5.25 - cos * 1.6, 1.15, cos * 5.25 - sin * 1.6],
    );
    for id in entities.entities_in(
        world,
        add(center, [-3.0; 3]),
        add(center, [3.0; 3]),
        mob.id(),
    ) {
        if id == primary {
            continue;
        }
        entities.with_entity(world, id, |target| {
            if !enemy(world, mob, target) || target.vehicle().is_some() {
                return;
            }
            if repeat {
                mobs.reset_hurt_cooldown(target);
            }
            ai.attack(
                world,
                mob,
                id,
                damage * crate::hunger::melee_multiplier(mob),
            );
        });
    }
}

fn release(world: World<'_>, mob: Entity<'_>, id: i32, damage: bool) {
    let (Some(ai), Some(entities), Some(mobs)) = (MOB_AI.get(), ENTITIES.get(), MOBS.get()) else {
        return;
    };
    entities.with_entity(world, id, |target| {
        if target.vehicle() == Some(mob.id())
            && target.stop_riding()
            && damage
            && target.vehicle() != Some(mob.id())
            && !crate::allies::is_ally(world, mob, target)
        {
            mobs.reset_hurt_cooldown(target);
            ai.attack(world, mob, id, 7.0);
            target.change(&EntityChange {
                mask: SET_MOTION,
                motion: [0.0, -0.65, 0.0],
                ..EntityChange::default()
            });
        }
    });
}

fn volley(world: World<'_>, mob: Entity<'_>, id: i32) -> Vec<Shot> {
    let (Some(entities), Some(view)) = (ENTITIES.get(), mob.view()) else {
        return Vec::new();
    };
    let Some(at) = entities.with_entity(world, id, |e| e.view()).flatten() else {
        return Vec::new();
    };
    let [dx, dy, dz] = offset(at.position, view.position);
    let angle = dz.atan2(dx);
    // shortcut: a ballistic lead from the current position. The Java AI takes off first.
    let duration = (dx.hypot(dz) / 0.50).clamp(4.0, 70.0);
    let vertical = ((dy - 1.5) / duration + 0.022 * duration).clamp(-0.4, 1.4);
    [-0.18, 0.0, 0.18]
        .into_iter()
        .map(|spread| {
            let (sin, cos) = (angle + spread).sin_cos();
            Shot {
                position: add(view.position, [cos * 2.0, 1.5, sin * 2.0]),
                motion: [cos * 0.55, vertical, sin * 0.55],
                age: 0,
                rider: false,
            }
        })
        .collect()
}

fn area(
    world: World<'_>,
    mob: Entity<'_>,
    center: [f64; 3],
    radius: f64,
    vertical: f64,
    damage: f32,
    fire: u32,
) {
    let (Some(ai), Some(entities), Some(mobs)) = (MOB_AI.get(), ENTITIES.get(), MOBS.get()) else {
        return;
    };
    for id in entities.entities_in(
        world,
        add(center, [-radius, -vertical, -radius]),
        add(center, [radius, vertical, radius]),
        mob.id(),
    ) {
        entities.with_entity(world, id, |entity| {
            if enemy(world, mob, entity)
                && entity.view().is_some_and(|view| {
                    let [x, y, z] = offset(view.position, center);
                    x.hypot(z) <= radius && y.abs() <= vertical
                })
            {
                ai.attack(
                    world,
                    mob,
                    id,
                    damage * crate::hunger::melee_multiplier(mob),
                );
                if fire > 0 {
                    mobs.set_on_fire(entity, fire);
                }
            }
        });
    }
}

pub fn effects(world: World<'_>, mob: Entity<'_>, shots: &mut Vec<Shot>, fire: bool) {
    let Some(visuals) = EFFECTS.get() else { return };
    if fire
        && tick_count().is_multiple_of(5)
        && let Some(view) = mob.view()
        && !world
            .fluid_id(crate::flight::cell(view.position))
            .is_some_and(|fluid| fluid.ends_with("water"))
    {
        visuals.particles(
            world,
            "minecraft:flame",
            add(view.position, [0.0, 1.0, 0.0]),
            [1.3, 0.8, 1.3],
            0.06,
            12,
        );
        area(world, mob, view.position, 3.5, 2.5, 3.0, 80);
    }
    shots.retain_mut(|shot| {
        shot.age += 1;
        shot.motion[1] -= 0.04;
        let next = add(shot.position, shot.motion);
        let lower = std::array::from_fn(|i| shot.position[i].min(next[i]) - 0.3);
        let upper = std::array::from_fn(|i| shot.position[i].max(next[i]) + 0.3);
        let targets: Vec<EntityBody> = ENTITIES.get().map_or_else(Vec::new, |entities| {
            entities
                .entities_in(world, lower, upper, mob.id())
                .into_iter()
                .filter_map(|id| {
                    entities
                        .with_entity(world, id, |entity| {
                            if enemy(world, mob, entity) {
                                entity.body()
                            } else {
                                None
                            }
                        })
                        .flatten()
                })
                .collect()
        });
        let steps = ((length_squared(shot.motion).sqrt() * 4.0).ceil() as u32).max(1);
        for i in 1..=steps {
            let point = add(
                shot.position,
                shot.motion.map(|v| v * f64::from(i) / f64::from(steps)),
            );
            let Some(shape) = world.collision_shape(crate::flight::cell(point)) else {
                return false;
            };
            if !shape.is_empty()
                || shot.age >= 200
                || targets.iter().any(|body| {
                    (0..3).all(|axis| {
                        (body.min[axis] - 0.3..=body.max[axis] + 0.3).contains(&point[axis])
                    })
                })
            {
                visuals.particles(world, "minecraft:lava", point, [1.0, 0.45, 1.0], 0.0, 12);
                if shot.rider {
                    if let (Some(entities), Some(mobs)) = (ENTITIES.get(), MOBS.get()) {
                        for id in entities.entities_in(
                            world,
                            add(point, [-7.0; 3]),
                            add(point, [7.0; 3]),
                            mob.id(),
                        ) {
                            entities.with_entity(world, id, |target| {
                                if enemy(world, mob, target) && !fire_immune(target) {
                                    target.hurt_by(mob.id(), "minecraft:player_explosion", 20.0);
                                    mobs.set_on_fire(target, 80);
                                }
                            });
                        }
                    }
                } else {
                    area(world, mob, point, 7.0, 3.5, 20.0, 0);
                }
                return false;
            }
        }
        visuals.particles(world, "minecraft:flame", next, [0.2; 3], 0.03, 4);
        shot.position = next;
        shot.motion = shot.motion.map(|v| v * 0.98);
        true
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rider_cooldowns_begin_at_ability_end() {
        for (kind, duration, cooldown) in [
            (Attack::Bite, 12, 15),
            (Attack::DoubleBite, 20, 20),
            (Attack::SlashGrab, 40, 7),
            (Attack::MagmaVolley, 29, 400),
        ] {
            let mut state = Abilities::default();
            assert!(state.start_rider(10, 1, kind));
            assert!(state.rider_attack());
            assert!(!state.start_rider(11, 1, kind));
            state.running = None;
            assert!(!state.start_rider(10 + duration + cooldown - 1, 1, kind));
            assert!(state.start_rider(10 + duration + cooldown, 1, kind));
        }
    }

    #[test]
    fn rider_fire_is_hold_with_cooldown_on_release() {
        let mut state = Abilities::default();
        assert!(state.start_fire(10, 1));
        assert_eq!(state.fire_until, 1011);
        assert!(!state.start_fire(11, 1));
        state.stop_fire(20);
        assert_eq!(state.fire_until, 0);
        assert!(!state.start_fire(59, 1));
        assert!(state.start_fire(60, 1));
    }

    #[test]
    fn range_selects_all_primary_attacks_with_cooldown() {
        for (gap, expected) in [
            (1.0, Attack::DoubleBite),
            (5.0, Attack::SlashGrab),
            (12.0, Attack::MagmaVolley),
        ] {
            let mut state = Abilities::default();
            assert!(state.choose(0, 1, gap, true));
            assert_eq!(state.last, Some(expected));
            assert!(!state.choose(0, 1, gap, true));
        }
        let mut state = Abilities::default();
        assert!(state.choose(0, 1, 1.0, true));
        assert!(state.choose(28, 1, 1.0, true));
        assert_eq!(state.last, Some(Attack::Bite));
    }
}
