//! Transient hearing, scent, target memory and falling-owner rescue.
use crate::{ENTITIES, MOB_AI, MOBS, abilities, ai, flight, hunger, hunt, roost, taming};
use moddedmc_api::{
    abi::{GAME_EVENT_SOURCE_SILENT, GAME_EVENT_SOURCE_SNEAKING, GAME_EVENT_SOURCE_SPECTATOR},
    content::World,
    entities::{ENTITY_ALIVE, ENTITY_PLAYER, Entity, EntityChange, SET_MOTION, SET_ON_GROUND},
    mob_ai::goal_phase,
    tick_count,
};
use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
};

pub const INVESTIGATE: u64 = 210;
pub const RESCUE: u64 = 211;

#[derive(Clone, Copy)]
struct Track {
    source: Option<[u8; 16]>,
    at: [f64; 3],
    expires: u64,
    confidence: f64,
    hostile: bool,
    assess_until: u64,
}

#[derive(Default)]
struct Memory {
    cursor: u64,
    seen: u64,
    heard: Option<(u64, f64)>,
    track: Option<Track>,
    disturbance: f64,
    last_disturbance: Option<u64>,
    scent_next: u64,
    owner_hurt: Option<([u8; 16], i64, i32)>,
    owner_attack: Option<([u8; 16], i64, i32)>,
    rescue: Option<[u8; 16]>,
}

static MEMORIES: Mutex<Option<HashMap<i32, Memory>>> = Mutex::new(None);

fn memory<T>(id: i32, f: impl FnOnce(&mut Memory) -> T) -> T {
    let mut lock = MEMORIES.lock().unwrap_or_else(PoisonError::into_inner);
    f(lock.get_or_insert_default().entry(id).or_default())
}

pub fn forget(id: i32) {
    if let Some(all) = MEMORIES
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_mut()
    {
        all.remove(&id);
    }
}

fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    flight::length_squared(flight::offset(a, b)).sqrt()
}

fn eye(entity: Entity<'_>) -> Option<[f64; 3]> {
    let mut at = entity.view()?.position;
    let body = entity.body()?;
    at[1] = body.min[1] + (body.max[1] - body.min[1]) * 0.85;
    Some(at)
}

pub(crate) fn visible(world: World<'_>, mob: Entity<'_>, target: Entity<'_>) -> bool {
    eye(mob).zip(eye(target)).is_some_and(|(from, to)| {
        world
            .raycast(from, to, Some(mob.id()))
            .is_some_and(|(block, _)| block.is_none())
    })
}

fn aggressive(name: &str, mob: Entity<'_>) -> bool {
    !taming::is_tamed(mob)
        && !crate::eggs::is_juvenile(mob)
        && matches!(name, "volitans" | "varasuchus")
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Sound {
    Explosion,
    Combat,
    Projectile,
    Roar,
    Impact,
    Step,
    Block,
    Teleport,
    Splash,
}

impl Sound {
    fn read(kind: &str) -> Option<(Self, f64, bool)> {
        Some(match kind.trim_start_matches("minecraft:") {
            "explode" | "lightning_strike" => (Self::Explosion, 1.0, true),
            "entity_roar" | "shriek" => (Self::Roar, 0.95, true),
            "entity_damage" | "entity_die" => (Self::Combat, 0.85, true),
            "hit_ground" => (Self::Impact, 0.7, false),
            "projectile_land" | "projectile_shoot" => (Self::Projectile, 0.75, true),
            "block_destroy" | "block_place" => (Self::Block, 0.65, true),
            "block_open" | "block_close" => (Self::Block, 0.65, false),
            "teleport" => (Self::Teleport, 0.7, true),
            "splash" | "swim" => (Self::Splash, 0.5, false),
            "step" | "flap" | "elytra_glide" => (Self::Step, 0.3, false),
            _ => return None,
        })
    }

    fn dangerous(self) -> bool {
        matches!(
            self,
            Self::Explosion | Self::Combat | Self::Projectile | Self::Roar
        )
    }

    fn disturbance(self, c: f64, wet: bool) -> f64 {
        match self {
            Self::Impact => 7.0 + c * 18.0,
            Self::Step if c >= 0.18 => 0.75 + c * 2.5,
            Self::Step => 0.0,
            Self::Splash if wet => 4.0 + c * 14.0,
            Self::Splash => 2.0 + c * 7.0,
            Self::Block => 3.0 + c * 9.0,
            Self::Teleport => 5.0 + c * 12.0,
            _ => 4.0 + c * 10.0,
        }
    }
}

fn confidence(loudness: f64, range: f64, distance: f64, sneaking: bool, obstructed: bool) -> f64 {
    let effective = range * (0.35 + loudness * 0.65);
    (loudness * (1.0 - distance / effective)).max(0.0)
        * if sneaking { 0.35 } else { 1.0 }
        * if obstructed { 0.45 } else { 1.0 }
}

fn random_fraction() -> f64 {
    ai::random(10_000) as f64 / 10_000.0
}

fn uncertain(mut at: [f64; 3], radius: f64) -> [f64; 3] {
    let angle = random_fraction() * std::f64::consts::TAU;
    let radial = random_fraction().sqrt() * radius;
    at[0] += angle.cos() * radial;
    at[2] += angle.sin() * radial;
    at[1] += (random_fraction() - 0.5) * radius * 0.25;
    at
}

fn free(mob: Entity<'_>) -> bool {
    mob.flags() & ENTITY_ALIVE != 0
        && !crate::eggs::is_juvenile(mob)
        && mob.vehicle().is_none()
        && MOBS.get().is_some_and(|m| m.passengers(mob).is_empty())
        && taming::order_name(mob) != "stay"
        && roost::sleep_clip(mob.id()).is_none()
}

fn can_investigate(mob: Entity<'_>) -> bool {
    free(mob)
        && (!taming::is_tamed(mob) || taming::order_name(mob) == "wander")
        && !hunt::active(mob.id())
        && !memory(mob.id(), |m| m.rescue.is_some())
}

fn target_candidate(
    world: World<'_>,
    mob: Entity<'_>,
    id: i32,
    valid: &impl Fn(i32) -> bool,
) -> bool {
    valid(id)
        && ENTITIES.get().is_some_and(|entities| {
            entities
                .with_entity(world, id, |target| abilities::enemy(world, mob, target))
                .unwrap_or(false)
        })
}

/// Separate hurt and attack timestamps, matching owner-defense priority in Java.
pub fn owner_target(
    world: World<'_>,
    mob: Entity<'_>,
    hurt: bool,
    valid: impl Fn(i32) -> bool,
) -> Option<i32> {
    let (mobs, entities) = (MOBS.get()?, ENTITIES.get()?);
    let owner = taming::owner(mob)?;
    let id = mobs.by_uuid(world, &owner)?;
    let (target, stamp) = entities.with_entity(world, id, |e| {
        if hurt {
            mobs.last_hurt_by_mob(e)
        } else {
            mobs.last_hurt_mob(e)
        }
    })??;
    if !target_candidate(world, mob, target, &valid) {
        return None;
    }
    memory(mob.id(), |m| {
        let slot = if hurt {
            &mut m.owner_hurt
        } else {
            &mut m.owner_attack
        };
        let event = (owner, stamp, target);
        if *slot == Some(event) {
            None
        } else {
            *slot = Some(event);
            Some(target)
        }
    })
}

pub fn owner_attack(world: World<'_>, mob: Entity<'_>, valid: impl Fn(i32) -> bool) -> Option<i32> {
    owner_target(world, mob, false, valid)
}

pub fn tick(world: World<'_>, mob: Entity<'_>, name: &str) {
    if !hunger::DRAGONS.contains(&name) {
        return;
    }
    let (Some(entities), Some(mobs), Some(ai_api), Some(view), Some(ear)) = (
        ENTITIES.get(),
        MOBS.get(),
        MOB_AI.get(),
        mob.view(),
        eye(mob),
    ) else {
        return;
    };
    let now = tick_count();
    let cursor = memory(mob.id(), |m| {
        m.seen = now;
        m.cursor
    });
    if now.is_multiple_of(200)
        && let Some(all) = MEMORIES
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_mut()
    {
        all.retain(|_, m| now.saturating_sub(m.seen) < 400);
    }
    let range = (mob
        .attribute_value("minecraft:follow_range")
        .unwrap_or(32.0)
        * 0.9)
        .round()
        .clamp(24.0, 48.0);
    let sleeping = matches!(roost::sleep_clip(mob.id()), Some("sleep" | "fall_asleep"));
    memory(mob.id(), |m| {
        m.disturbance = (m.disturbance - if sleeping { 0.05 } else { 0.2 }).max(0.0)
    });
    if let Some((next, events)) = entities.game_events(world, mob, range, cursor) {
        memory(mob.id(), |m| m.cursor = next);
        for event in events {
            let Some((sound, loudness, investigate)) = Sound::read(&event.kind) else {
                continue;
            };
            if event.flags & (GAME_EVENT_SOURCE_SILENT | GAME_EVENT_SOURCE_SPECTATOR) != 0
                || event.source.is_some_and(|(id, _)| {
                    id == mob.id() || mob.has_indirect_passenger(id) || mob.vehicle() == Some(id)
                })
            {
                continue;
            }
            let source = event
                .source
                .and_then(|(id, uuid)| {
                    entities.with_entity(world, id, |e| {
                        let resolved = e.projectile_owner().and_then(|owner| {
                            entities
                                .with_entity(world, owner, |owner| {
                                    (owner.id(), mobs.uuid(owner)?).into()
                                })
                                .flatten()
                        });
                        resolved.unwrap_or((id, uuid))
                    })
                })
                .or(event.source);
            if source.is_some_and(|(_, uuid)| Some(uuid) == taming::owner(mob)) {
                continue;
            }
            let (player, ignored, recognized) = source
                .and_then(|(id, _)| {
                    entities.with_entity(world, id, |e| {
                        (
                            e.view().is_some_and(|v| v.kind & ENTITY_PLAYER != 0),
                            matches!(mobs.game_mode(e), Some(1 | 3)),
                            visible(world, mob, e),
                        )
                    })
                })
                .unwrap_or((false, false, false));
            if ignored {
                continue;
            }
            let blocked = world
                .raycast(ear, event.position, Some(mob.id()))
                .is_none_or(|(block, _)| block.is_some());
            let d = distance(ear, event.position);
            let c = confidence(
                loudness,
                range,
                d,
                sound == Sound::Step && event.flags & GAME_EVENT_SOURCE_SNEAKING != 0,
                blocked,
            );
            if c < 0.08 {
                continue;
            }
            if !memory(mob.id(), |m| {
                if m.heard
                    .is_some_and(|(at, old)| now.saturating_sub(at) < 10 && old > c)
                {
                    return false;
                }
                m.heard = Some((now, c));
                true
            }) {
                continue;
            }
            let wet = mob.flags() & moddedmc_api::abi::ENTITY_IN_WATER != 0;
            let alert = if !player {
                sound.dangerous() && c >= 0.12
            } else if !aggressive(name, mob) {
                sound.dangerous() && c >= 0.18
            } else {
                d <= 24.0
                    && c >= match sound {
                        Sound::Splash if wet => 0.09,
                        Sound::Step | Sound::Splash => 0.16,
                        Sound::Block | Sound::Teleport => 0.14,
                        _ => 0.10,
                    }
            };
            let disturbed = sleeping
                && memory(mob.id(), |m| {
                    if m.last_disturbance
                        .is_some_and(|at| now.saturating_sub(at) < 10)
                    {
                        return false;
                    }
                    m.last_disturbance = Some(now);
                    m.disturbance = (m.disturbance + sound.disturbance(c, wet)).min(100.0);
                    m.disturbance >= 40.0
                });
            if alert || disturbed {
                roost::wake(mob, false, 400);
            }
            let source_uuid = source
                .filter(|_| sound != Sound::Projectile || recognized)
                .map(|(_, uuid)| uuid);
            let at = uncertain(event.position, (1.0 - c) * 5.0);
            let hostile = source.is_some_and(|(id, _)| ai_api.target(mob) == Some(id))
                || (player && aggressive(name, mob));
            memory(mob.id(), |m| {
                if let Some(track) = m
                    .track
                    .as_mut()
                    .filter(|t| source_uuid.is_some() && t.source == source_uuid)
                {
                    track.at = at;
                    track.confidence = track.confidence.max(c);
                    if !track.hostile {
                        track.expires = now + 240;
                    }
                } else if investigate
                    && c >= 0.15
                    && ai_api.target(mob).is_none()
                    && (!recognized || sound == Sound::Projectile)
                    && !(sound == Sound::Block && source_uuid.is_none())
                {
                    let ttl = ((d - 2.0).max(0.0) * 8.0 + 100.0)
                        .ceil()
                        .clamp(160.0, 400.0) as u64;
                    if m.track
                        .is_none_or(|t| t.expires <= now || (!t.hostile && c >= t.confidence))
                    {
                        m.track = Some(Track {
                            source: source_uuid,
                            at,
                            expires: now + ttl,
                            confidence: c,
                            hostile,
                            assess_until: now,
                        });
                    }
                }
            });
        }
    }
    if let Some(id) = ai_api.target(mob) {
        let observation = entities
            .with_entity(world, id, |target| {
                Some((
                    mobs.uuid(target)?,
                    target.view()?.position,
                    visible(world, mob, target),
                ))
            })
            .flatten();
        if let Some((uuid, at, true)) = observation {
            memory(mob.id(), |m| {
                m.track = Some(Track {
                    source: Some(uuid),
                    at,
                    expires: now + 240,
                    confidence: 1.0,
                    hostile: true,
                    assess_until: now,
                })
            });
        } else if observation.is_some_and(|(uuid, _, _)| {
            memory(mob.id(), |m| {
                m.track.is_some_and(|t| t.hostile && t.source == Some(uuid))
            })
        }) {
            ai_api.set_target(world, mob, None);
        }
    }
    memory(mob.id(), |m| {
        if m.track.is_some_and(|t| t.expires <= now) {
            m.track = None;
        }
    });
    if ai_api.target(mob).is_none()
        && free(mob)
        && let Some(track) = memory(mob.id(), |m| m.track)
        && track.hostile
        && let Some(id) = track.source.and_then(|uuid| mobs.by_uuid(world, &uuid))
        && entities
            .with_entity(world, id, |e| {
                abilities::enemy(world, mob, e) && visible(world, mob, e)
            })
            .unwrap_or(false)
    {
        ai_api.set_target(world, mob, Some(id));
    }
    if ai_api.target(mob).is_none()
        && free(mob)
        && now.is_multiple_of(10)
        && matches!(
            name,
            "cindervane" | "raevyx" | "ignivorus" | "volitans" | "varasuchus" | "stegonaut"
        )
        && mobs.active_raid(world, mob) == Some(true)
    {
        let range = mob
            .attribute_value("minecraft:follow_range")
            .unwrap_or(32.0);
        let candidate = entities
            .entities_matching(
                world,
                view.position.map(|v| v - range),
                view.position.map(|v| v + range),
                mob.id(),
                0,
            )
            .into_iter()
            .filter_map(|id| {
                entities
                    .with_entity(world, id, |e| {
                        if mobs.raid_member(world, e) != Some(true)
                            || !abilities::enemy(world, mob, e)
                            || !visible(world, mob, e)
                        {
                            return None;
                        }
                        let d = distance(view.position, e.view()?.position);
                        (d <= range).then_some((id, d))
                    })
                    .flatten()
            })
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((id, _)) = candidate {
            ai_api.set_target(world, mob, Some(id));
        }
    }
    if ai_api.target(mob).is_some()
        || !can_investigate(mob)
        || !memory(mob.id(), |m| m.track.is_none() && now >= m.scent_next)
    {
        return;
    }
    memory(mob.id(), |m| m.scent_next = now + 20);
    let Some(body) = mob.body() else {
        return;
    };
    let horizontal = (18.0 + (body.max[0] - body.min[0]) * 2.0).clamp(20.0, 30.0);
    let vertical = (6.0 + (body.max[1] - body.min[1]) * 0.75).clamp(8.0, 14.0);
    let extent = [horizontal, vertical, horizontal];
    let candidate = entities
        .entities_matching(
            world,
            std::array::from_fn(|i| view.position[i] - extent[i]),
            std::array::from_fn(|i| view.position[i] + extent[i]),
            mob.id(),
            0,
        )
        .into_iter()
        .filter_map(|id| {
            entities
                .with_entity(world, id, |e| {
                    let v = e.view()?;
                    let player = v.kind & ENTITY_PLAYER != 0;
                    let prey = hunger::level(mob) <= 60 && hunt::prey_type(name, e.type_id());
                    let d = distance(view.position, v.position);
                    (d <= horizontal
                        && (prey || (player && aggressive(name, mob)))
                        && abilities::enemy(world, mob, e)
                        && !visible(world, mob, e))
                    .then(|| (mobs.uuid(e), v.position, d, player))
                    .and_then(|(uuid, at, d, hostile)| Some((uuid?, at, d, hostile)))
                })
                .flatten()
        })
        .min_by(|a, b| a.2.total_cmp(&b.2));
    if let Some((uuid, at, d, hostile)) = candidate {
        let fraction = (d / horizontal).clamp(0.0, 1.0);
        let assess_until = now + 24 + ai::random(13);
        memory(mob.id(), |m| {
            m.scent_next = assess_until + 240 + ai::random(161);
            m.track = Some(Track {
                source: Some(uuid),
                at: uncertain(at, 1.5 + fraction * 4.5),
                expires: assess_until + 240,
                confidence: 0.85 - fraction * 0.5,
                hostile,
                assess_until,
            });
        });
    }
}

pub fn investigate(phase: u32, world: World<'_>, mob: Entity<'_>) -> bool {
    let (Some(ai), Some(mobs), Some(entities), Some(view)) =
        (MOB_AI.get(), MOBS.get(), ENTITIES.get(), mob.view())
    else {
        return false;
    };
    let track = memory(mob.id(), |m| m.track);
    let now = tick_count();
    match phase {
        goal_phase::CAN_START | goal_phase::CONTINUE => {
            ai.target(mob).is_none()
                && free(mob)
                && !memory(mob.id(), |m| m.rescue.is_some())
                && track.is_some_and(|t| t.expires > now && (t.hostile || can_investigate(mob)))
        }
        goal_phase::TICK => {
            if let Some(track) = track {
                if track.hostile
                    && let Some(id) = track.source.and_then(|uuid| mobs.by_uuid(world, &uuid))
                    && entities
                        .with_entity(world, id, |e| {
                            abilities::enemy(world, mob, e) && visible(world, mob, e)
                        })
                        .unwrap_or(false)
                {
                    ai.set_target(world, mob, Some(id));
                    return false;
                }
                ai.look_at(mob, track.at);
                if now < track.assess_until || distance(view.position, track.at) <= 2.0 {
                    ai.stop(mob);
                } else if now.is_multiple_of(10) {
                    ai.walk_to(mob, track.at, 0.85);
                }
            }
            false
        }
        goal_phase::STOP => {
            ai.stop(mob);
            false
        }
        _ => false,
    }
}

fn rescue_profile(name: &str) -> Option<(f64, f64, f64)> {
    Some(match name {
        "raevyx" => (4.5, 1.5, 1.75),
        "cindervane" => (5.0, 1.5, 1.75),
        "ignivorus" => (3.8, 2.5, 3.0),
        "volitans" => (4.2, 2.0, 2.5),
        _ => return None,
    })
}

pub fn rescue_supported(name: &str) -> bool {
    rescue_profile(name).is_some()
}

fn rescue_eligible(mob: Entity<'_>) -> bool {
    free(mob)
        && taming::rescue_eligible(mob)
        && mob.flags() & moddedmc_api::abi::ENTITY_IN_WATER == 0
        && MOB_AI.get().is_some_and(|ai| ai.target(mob).is_none())
}

fn dangerous_fall(world: World<'_>, player: Entity<'_>) -> bool {
    let (Some(view), Some(state)) = (player.view(), player.player_fall_state()) else {
        return false;
    };
    if !state.falling || state.distance < 1.5 || state.movement[1] > -0.12 {
        return false;
    }
    let [x, y, z] = view.position;
    // Scan the vertical landing column, including water which the block ray ignores.
    for step in 1..=96 {
        let cell = flight::cell([x, y - f64::from(step), z]);
        if matches!(
            world.fluid_id(cell).as_deref(),
            Some("minecraft:water" | "minecraft:flowing_water")
        ) {
            return false;
        }
        let Some((block, _)) = world.raycast(
            [x, y - f64::from(step - 1), z],
            [x, y - f64::from(step), z],
            Some(player.id()),
        ) else {
            return false;
        };
        if block.is_some() {
            return step >= 20;
        }
    }
    true
}

pub fn rescue(phase: u32, world: World<'_>, mob: Entity<'_>) -> bool {
    let (Some(mobs), Some(entities), Some(ai), Some(view)) =
        (MOBS.get(), ENTITIES.get(), MOB_AI.get(), mob.view())
    else {
        return false;
    };
    let Some((speed, horizontal, vertical)) = mobs
        .kind(mob)
        .and_then(|k| rescue_profile(k.trim_start_matches("saintsdragons:")))
    else {
        return false;
    };
    if phase == goal_phase::STOP {
        memory(mob.id(), |m| m.rescue = None);
        if mobs.passengers(mob).is_empty() {
            mobs.set_no_gravity(mob, false);
        }
        return false;
    }
    let Some(owner) = taming::owner(mob) else {
        return false;
    };
    let Some(id) = mobs.by_uuid(world, &owner) else {
        return false;
    };
    entities
        .with_entity(world, id, |player| {
            let (Some(p), Some(state)) = (player.view(), player.player_fall_state()) else {
                return false;
            };
            let d = distance(view.position, p.position);
            if phase == goal_phase::CAN_START {
                if !rescue_eligible(mob) || d > 256.0 || !dangerous_fall(world, player) {
                    return false;
                }
                let better = entities
                    .entities_matching(
                        world,
                        p.position.map(|v| v - 256.0),
                        p.position.map(|v| v + 256.0),
                        mob.id(),
                        0,
                    )
                    .into_iter()
                    .any(|other| {
                        entities
                            .with_entity(world, other, |e| {
                                if taming::owner(e) != Some(owner)
                                    || !rescue_eligible(e)
                                    || !mobs.kind(e).is_some_and(|k| {
                                        rescue_supported(k.trim_start_matches("saintsdragons:"))
                                    })
                                {
                                    return false;
                                }
                                e.view().is_some_and(|v| {
                                    let gap = distance(v.position, p.position);
                                    gap < d - 0.0001
                                        || ((gap - d).abs() <= 0.0001 && e.id() < mob.id())
                                })
                            })
                            .unwrap_or(false)
                    });
                return !better;
            }
            if phase == goal_phase::CONTINUE {
                return state.falling
                    && d <= 256.0
                    && rescue_eligible(mob)
                    && memory(mob.id(), |m| m.rescue == Some(owner));
            }
            if phase == goal_phase::START {
                memory(mob.id(), |m| m.rescue = Some(owner));
                roost::wake(mob, true, 400);
                ai.stop(mob);
                mobs.set_no_gravity(mob, true);
            } else if phase == goal_phase::TICK && state.falling {
                let (Some(body), Some(rider)) = (mob.body(), player.body()) else {
                    return false;
                };
                let margin = [horizontal, vertical, horizontal];
                let intersects = (0..3).all(|i| {
                    body.max[i] + margin[i] >= rider.min[i]
                        && body.min[i] - margin[i] <= rider.max[i]
                });
                if intersects
                    && world
                        .raycast(view.position, p.position, Some(mob.id()))
                        .is_some_and(|(block, _)| block.is_none())
                    && player.start_riding(mob.id(), true)
                {
                    player.reset_fall_distance();
                    player.change(&EntityChange {
                        mask: SET_MOTION,
                        motion: [0.0; 3],
                        ..Default::default()
                    });
                    mob.reset_fall_distance();
                    mob.change(&EntityChange {
                        mask: SET_MOTION,
                        motion: [0.0; 3],
                        ..Default::default()
                    });
                    memory(mob.id(), |m| m.rescue = None);
                    return false;
                }
                let lead = (d / 2.0).clamp(1.5, 12.0);
                let mut intercept =
                    std::array::from_fn(|i| p.position[i] + state.movement[i] * lead);
                intercept[1] -= 0.04 * lead * lead + (body.max[1] - body.min[1]) * 0.65;
                let waypoint =
                    crate::flight_route::waypoint(world, mob, view.position, intercept, false);
                let delta = flight::offset(waypoint, view.position);
                // The Java speed multiplier is applied by the flight controller; cap each
                // actual movement step and check collision before moving the carrier.
                let step = (speed
                    * mob
                        .attribute_value("minecraft:flying_speed")
                        .unwrap_or(0.25))
                .clamp(0.2, 1.4);
                let scale = step / flight::length_squared(delta).sqrt().max(step);
                let motion = delta.map(|v| v * scale);
                ai.look_at(mob, p.position);
                mob.reset_fall_distance();
                mob.change(&EntityChange {
                    mask: SET_MOTION | SET_ON_GROUND,
                    on_ground: false,
                    motion: if mob.can_move_by(motion) && flight::clear_body(world, body, motion) {
                        motion
                    } else {
                        [0.0; 3]
                    },
                    ..Default::default()
                });
            }
            false
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hearing_attenuates_with_distance_and_cover() {
        let clear = confidence(0.3, 24.0, 3.0, false, false);
        assert!(clear > 0.18);
        assert!(confidence(0.3, 24.0, 3.0, true, true) < 0.08);
        assert_eq!(confidence(0.3, 24.0, 24.0, false, false), 0.0);
        assert!((confidence(0.3, 24.0, 3.0, false, true) - clear * 0.45).abs() < 1e-10);
    }

    #[test]
    fn disturbance_distinguishes_steps_and_impact() {
        assert_eq!(Sound::Step.disturbance(0.17, false), 0.0);
        assert!(Sound::Impact.disturbance(0.7, false) > Sound::Step.disturbance(0.3, false));
        assert!(Sound::Splash.disturbance(0.5, true) > Sound::Splash.disturbance(0.5, false));
    }
}
