//! Saved nonliving combat effects. The host owns chunk load/unload and entity NBT.
use crate::{EFFECTS, ENTITIES, MOBS, pillars};
use moddedmc_api::{
    InterfaceUnavailable, RegistrationError, Registry,
    content::World,
    entities::{ENTITY_ALIVE, ENTITY_LIVING, Entity, EntityBody},
    mobs::ProjectileStep,
    native_entity::{self, ActorKind, Actors, Event, Phase, Spawn},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    hash::{BuildHasher, Hasher, RandomState},
    sync::{LazyLock, Mutex, OnceLock},
};

static ACTORS: OnceLock<Actors> = OnceLock::new();
static KIND: OnceLock<ActorKind> = OnceLock::new();

#[derive(Serialize, Deserialize)]
pub(crate) enum Effect {
    Fissure(Fissure),
    Pillar(pillars::Pillar),
    Fireball(Fireball),
}

pub fn require() -> Result<(), InterfaceUnavailable> {
    let _ = ACTORS.set(native_entity::require()?);
    Ok(())
}

pub fn register(reg: &mut Registry<'_>) -> Result<(), RegistrationError> {
    let actors = ACTORS.get().ok_or(RegistrationError::InvalidDefinition)?;
    let _ = KIND.set(actors.register(reg, event)?);
    Ok(())
}

fn uuid() -> [u8; 16] {
    let mut bytes = [0; 16];
    for chunk in bytes.chunks_exact_mut(8) {
        chunk.copy_from_slice(&RandomState::new().build_hasher().finish().to_le_bytes());
    }
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    bytes
}

pub(crate) fn spawn(world: World<'_>, at: [f64; 3], width: f32, height: f32, effect: Effect) {
    if let (Some(actors), Some(kind), Ok(data)) =
        (ACTORS.get(), KIND.get(), serde_json::to_vec(&effect))
    {
        actors.spawn(
            &world,
            *kind,
            &Spawn {
                uuid: uuid(),
                position: at,
                width,
                height,
                physics: false,
                data: &data,
            },
        );
    }
}

fn event(event: Event<'_>) -> bool {
    let Some(mut effect) = event
        .actor
        .data()
        .and_then(|bytes| serde_json::from_slice::<Effect>(&bytes).ok())
    else {
        return false;
    };
    match &mut effect {
        Effect::Fissure(fissure) => {
            if event.phase == Phase::Tick && !fissure.tick(&event) {
                event.actor.entity.discard();
                return false;
            }
        }
        Effect::Pillar(pillar) => {
            if !pillar.event(&event) {
                return false;
            }
        }
        Effect::Fireball(shot) => {
            if !shot.event(&event) {
                return false;
            }
        }
    }
    if event.phase == Phase::Tick
        && let Ok(data) = serde_json::to_vec(&effect)
    {
        event.actor.set_data(&data);
    }
    false
}

#[derive(Serialize, Deserialize)]
pub(crate) struct Fireball {
    owner: [u8; 16],
    motion: [f64; 3],
    age: u16,
}

static FIREBALL_DISPLAYS: LazyLock<Mutex<HashMap<[u8; 16], i32>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn fireball(world: World<'_>, mob: Entity<'_>, at: [f64; 3], motion: [f64; 3]) {
    let Some(owner) = MOBS.get().and_then(|m| m.uuid(mob)) else {
        return;
    };
    spawn(
        world,
        at,
        0.98,
        0.98,
        Effect::Fireball(Fireball {
            owner,
            motion,
            age: 0,
        }),
    );
}

impl Fireball {
    fn eligible(&self, world: World<'_>, target: Entity<'_>, owner: Option<i32>) -> bool {
        let (Some(mobs), Some(entities)) = (MOBS.get(), ENTITIES.get()) else {
            return false;
        };
        if !target.view().is_some_and(|v| v.kind & ENTITY_LIVING != 0)
            || target.flags() & ENTITY_ALIVE == 0
            || mobs.uuid(target) == Some(self.owner)
        {
            return false;
        }
        owner.is_none_or(|id| {
            entities
                .with_entity(world, id, |mob| !crate::allies::is_ally(world, mob, target))
                .unwrap_or(true)
        })
    }

    fn event(&mut self, event: &Event<'_>) -> bool {
        let mut display = FIREBALL_DISPLAYS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&event.actor.uuid);
        if matches!(event.phase, Phase::Removed | Phase::Unloaded) {
            if let Some(id) = display {
                crate::vfx::remove(event.world, id);
            }
            return false;
        }
        if event.phase == Phase::Used {
            if let Some(id) = display {
                FIREBALL_DISPLAYS
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert(event.actor.uuid, id);
            }
            return false;
        }
        let world = event.world;
        let Some(mobs) = MOBS.get() else { return false };
        let Some(view) = event.actor.entity.view() else {
            return false;
        };
        let mut position = view.position;
        if event.phase == Phase::Tick {
            self.age += 1;
            self.motion[1] -= 0.04;
            let owner = mobs.by_uuid(world, &self.owner);
            let step =
                mobs.step_effect_projectile(event.actor.entity, self.motion, 0.3, true, |target| {
                    self.eligible(world, target, owner)
                });
            match step {
                Some(ProjectileStep::Miss(at)) if self.age <= 200 => {
                    position = at;
                    self.motion = self.motion.map(|v| v * 0.98);
                    if let Some(effects) = EFFECTS.get() {
                        effects.particles(world, "minecraft:flame", at, [0.2; 3], 0.03, 4);
                    }
                }
                Some(
                    ProjectileStep::Miss(at)
                    | ProjectileStep::Block(at)
                    | ProjectileStep::Entity(at),
                ) => {
                    self.explode(event, at, owner);
                    if let Some(id) = display {
                        crate::vfx::remove(world, id);
                    }
                    event.actor.entity.discard();
                    return false;
                }
                None => {
                    if let Some(id) = display {
                        crate::vfx::remove(world, id);
                    }
                    event.actor.entity.discard();
                    return false;
                }
            }
        }
        crate::vfx::projectile(
            world,
            &mut display,
            crate::vfx::FIREBALL,
            position,
            self.motion,
            1.0,
        );
        if let Some(id) = display {
            FIREBALL_DISPLAYS
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(event.actor.uuid, id);
        }
        true
    }

    fn explode(&self, event: &Event<'_>, point: [f64; 3], owner: Option<i32>) {
        let world = event.world;
        let (Some(mobs), Some(entities)) = (MOBS.get(), ENTITIES.get()) else {
            return;
        };
        if let Some(effects) = EFFECTS.get() {
            effects.particles(world, "minecraft:lava", point, [1.0, 0.45, 1.0], 0.0, 12);
        }
        crate::vfx::burst(
            world,
            point,
            3.0,
            "shared/explosions/fire_ground_impact/fire_ground_impact",
            6,
            false,
        );
        let direct = event.actor.entity;
        for id in entities.entities_in(
            world,
            point.map(|x| x - 7.0),
            point.map(|x| x + 7.0),
            direct.id(),
        ) {
            entities.with_entity(world, id, |target| {
                if !self.eligible(world, target, owner) || crate::abilities::fire_immune(target) {
                    return;
                }
                let hurt = |source| {
                    mobs.hurt_from(target, direct, source, "minecraft:player_explosion", 20.0)
                };
                if owner
                    .and_then(|id| entities.with_entity(world, id, hurt))
                    .is_none()
                {
                    hurt(direct);
                }
                mobs.set_on_fire(target, 80);
            });
        }
        crate::abilities::terrain::volley(world, point);
    }
}

/// GroundEffectSurfaceSnap: inspect at most seven collision shapes below the effect.
pub(crate) fn snap(world: World<'_>, mut at: [f64; 3], offset: f64) -> [f64; 3] {
    let mut pos = crate::flight::cell([at[0], at[1] + 0.5, at[2]]);
    for _ in 0..7 {
        let Some(shape) = world.collision_shape(pos) else {
            break;
        };
        if let Some(top) = shape.iter().map(|b| b.max[1]).reduce(f64::max) {
            at[1] = f64::from(pos.y) + top - offset + 0.002;
            break;
        }
        pos.y -= 1;
    }
    at
}

#[derive(Serialize, Deserialize)]
pub(crate) struct Fissure {
    age: u32,
    duration: u32,
    owner: [u8; 16],
    radius: f64,
    damage: f32,
    // Java saves each target's next successful damage tick.
    next: Vec<([u8; 16], u32)>,
}

pub fn fissure(world: World<'_>, player: Entity<'_>, position: [f64; 3]) {
    let Some(owner) = MOBS.get().and_then(|m| m.uuid(player)) else {
        return;
    };
    // ToolsArmorConfig release defaults. The decal itself is a companion visual.
    spawn(
        world,
        snap(world, position, 0.0),
        1.0,
        0.1,
        Effect::Fissure(Fissure {
            age: 0,
            duration: 140,
            owner,
            radius: 9.0,
            damage: 10.0,
            next: Vec::new(),
        }),
    );
}

fn contact(at: [f64; 3], bounds: EntityBody, radius: f64) -> bool {
    bounds.min[1] <= at[1] + 1.25
        && bounds.max[1] >= at[1] - 0.25
        && [0, 2]
            .into_iter()
            .map(|i| (at[i].clamp(bounds.min[i], bounds.max[i]) - at[i]).powi(2))
            .sum::<f64>()
            <= radius * radius
}

impl Fissure {
    fn tick(&mut self, event: &Event<'_>) -> bool {
        let world = event.world;
        let Some(view) = event.actor.entity.view() else {
            return false;
        };
        self.age += 1;
        let at = snap(world, view.position, 0.0);
        if at != view.position {
            event.actor.entity.set_position(at);
        }
        self.damage(world, at);
        self.particles(world, at);
        self.age < self.duration
    }

    fn damage(&mut self, world: World<'_>, at: [f64; 3]) {
        let (Some(mobs), Some(entities)) = (MOBS.get(), ENTITIES.get()) else {
            return;
        };
        let Some(owner) = mobs.by_uuid(world, &self.owner) else {
            return;
        };
        let [x, y, z] = at;
        let r = self.radius + 0.5;
        self.next.retain(|(_, next)| *next > self.age);
        let mut cooldowns: HashMap<_, _> = self.next.iter().copied().collect();
        for id in entities.entities_in(
            world,
            [x - r, y - 2.5, z - r],
            [x + r, y + 2.6, z + r],
            owner,
        ) {
            entities.with_entity(world, id, |target| {
                let (Some(uuid), Some(body)) = (mobs.uuid(target), target.body()) else {
                    return;
                };
                if cooldowns.contains_key(&uuid) || !contact(at, body, self.radius) {
                    return;
                }
                let allowed = entities
                    .with_entity(world, owner, |player| {
                        player
                            .view()
                            .is_some_and(|v| v.kind & moddedmc_api::entities::ENTITY_PLAYER != 0)
                            && crate::player_moves::sword_target(player, target)
                    })
                    .unwrap_or(false);
                if allowed
                    && target.hurt_by(owner, "minecraft:player_attack", self.damage) == Some(true)
                {
                    cooldowns.insert(uuid, self.age + 20);
                }
            });
        }
        self.next = cooldowns.into_iter().collect();
    }

    fn particles(&self, world: World<'_>, at: [f64; 3]) {
        let Some(effects) = EFFECTS.get() else { return };
        let point = || {
            let random = || crate::ai::random(1 << 24) as f64 / f64::from(1 << 24);
            let angle = crate::ai::random(12) as f64 * std::f64::consts::TAU / 12.0
                + (random() - 0.5) * 0.24;
            let radius = 0.45 + random() * (self.radius - 0.45).max(0.1);
            [
                at[0] + angle.cos() * radius,
                at[1] + 0.025,
                at[2] + angle.sin() * radius,
            ]
        };
        if self.age.is_multiple_of(2) {
            for _ in 0..3 {
                effects.particles(world, "minecraft:flame", point(), [0.0, 0.3, 0.0], 1.0, 0);
            }
        }
        if self.age.is_multiple_of(5) {
            effects.particles(
                world,
                "minecraft:lava",
                point(),
                [0.08, 0.03, 0.08],
                0.02,
                1,
            );
        }
        if self.age.is_multiple_of(10) {
            for _ in 0..2 {
                let mut at = point();
                at[1] += 0.08;
                effects.particles(
                    world,
                    "minecraft:campfire_cosy_smoke",
                    at,
                    [0.0, 0.06, 0.0],
                    1.0,
                    0,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fissure_contacts_nearest_box_edge_not_entity_center() {
        let body = EntityBody {
            min: [8.5, 1.25, -0.5],
            max: [10.5, 3.0, 0.5],
            ..Default::default()
        };
        assert!(contact([0.0; 3], body, 9.0));
        assert!(!contact(
            [0.0; 3],
            EntityBody {
                min: [8.5, 1.26, -0.5],
                ..body
            },
            9.0
        ));
        assert!(!contact(
            [0.0; 3],
            EntityBody {
                min: [9.1, 0.0, -0.5],
                ..body
            },
            9.0
        ));
    }

    #[test]
    fn saved_fissure_preserves_age_and_cooldown() {
        let state = Fissure {
            age: 10,
            duration: 140,
            owner: [1; 16],
            radius: 9.0,
            damage: 10.0,
            next: vec![([2; 16], 25)],
        };
        let restored: Fissure =
            serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap();
        assert_eq!(restored.age, 10);
        assert_eq!(restored.next, vec![([2; 16], 25)]);
    }
}
