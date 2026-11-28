//! IgnivorusMagmaPillarEntity and StegonautAmethystPillarEntity, 0.9.88.
use crate::{EFFECTS, ENTITIES, MOBS, effect_actors, taming, vfx};
use moddedmc_api::{
    abi::heightmap,
    content::World,
    effects::SOUND_PLAYERS,
    entities::{ENTITY_ALIVE, ENTITY_LIVING, ENTITY_PLAYER, Entity, EntityChange, SET_MOTION},
    native_entity::{Event, Phase},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    sync::{LazyLock, Mutex},
};

#[derive(Clone, Copy, Serialize, Deserialize)]
enum Kind {
    Magma(u32),
    Amethyst,
}

impl Kind {
    fn rig(self, subsiding: bool) -> (&'static str, &'static str, f32) {
        match (self, subsiding) {
            (Self::Magma(_), false) => (vfx::MAGMA, "animation.ignivorus_magma_pillar.emerge", 1.0),
            (Self::Magma(_), true) => (vfx::MAGMA, "animation.ignivorus_magma_pillar.subside", 1.0),
            (Self::Amethyst, false) => (vfx::AMETHYST, "animation.amethyst_pillar_emerge", 2.0),
            (Self::Amethyst, true) => (vfx::AMETHYST, "animation.amethyst_pillar_subside", 2.0),
        }
    }

    fn timing(self) -> (u64, u64, u64) {
        match self {
            Self::Magma(_) => (5, 34, 9),
            Self::Amethyst => (4, 26, 10),
        }
    }

    fn bounds(self, [x, y, z]: [f64; 3]) -> ([f64; 3], [f64; 3]) {
        let (radius, below, above) = match self {
            Self::Magma(stage) => {
                let scale = f64::from(1.0_f32 + stage as f32 * 0.2);
                (2.25 + scale * 0.65, 0.75, 5.5 + scale * 2.4 - 0.75)
            }
            Self::Amethyst => (4.5, 0.5, 7.5),
        };
        (
            [x - radius, y - below, z - radius],
            [x + radius, y + above, z + radius],
        )
    }

    fn lift(self) -> f64 {
        match self {
            Self::Magma(stage) => 0.35 + f64::from(stage) * 0.05,
            Self::Amethyst => 0.32,
        }
    }
}

#[derive(Serialize, Deserialize)]
pub(crate) struct Pillar {
    kind: Kind,
    position: [f64; 3],
    owner: [u8; 16],
    player_owned: bool,
    age: u64,
    yaw: f32,
    damage: f32,
    knockback: f64,
    hits: HashSet<[u8; 16]>,
}

// Display IDs are process-local; recreate them on carrier load.
static DISPLAYS: LazyLock<Mutex<HashMap<[u8; 16], i32>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn spawn(
    world: World<'_>,
    owner: Entity<'_>,
    position: [f64; 3],
    yaw: f32,
    kind: Kind,
    damage: f32,
    knockback: f64,
) {
    let player_owned = owner.view().is_some_and(|v| v.kind & ENTITY_PLAYER != 0);
    let Some(owner) = MOBS.get().and_then(|m| m.uuid(owner)) else {
        return;
    };
    if matches!(kind, Kind::Magma(_)) {
        magma_sound(world, position, false, player_owned);
    }
    let (width, height) = match kind {
        Kind::Magma(stage) => {
            let scale = 1.0 + stage as f32 * 0.2;
            (5.5 * scale, 5.5 * scale)
        }
        Kind::Amethyst => (2.0, 4.0),
    };
    effect_actors::spawn(
        world,
        position,
        width,
        height,
        effect_actors::Effect::Pillar(Pillar {
            kind,
            position,
            owner,
            player_owned,
            age: 0,
            yaw,
            damage,
            knockback,
            hits: HashSet::new(),
        }),
    );
}

/// IgnivorusRoarAbility.spawnWave: three lanes, staggered by six blocks.
pub fn magma_wave(
    world: World<'_>,
    dragon: Entity<'_>,
    at: [f64; 3],
    forward: [f64; 3],
    stage: u32,
) {
    for lane in -1..=1 {
        let distance = 20.0 + f64::from(stage) * 6.0;
        let lateral = f64::from(lane) * 5.0;
        let x = at[0] + forward[0] * distance - forward[2] * lateral;
        let z = at[2] + forward[2] * distance + forward[0] * lateral;
        let Some(y) = world.surface_height(
            x.floor() as i32,
            z.floor() as i32,
            heightmap::MOTION_BLOCKING_NO_LEAVES,
        ) else {
            continue;
        };
        spawn(
            world,
            dragon,
            [x, f64::from(y), z],
            yaw(forward),
            Kind::Magma(stage),
            18.0 + stage as f32 * 4.0,
            0.9 + f64::from(stage) * 0.2,
        );
    }
}

fn yaw([x, _, z]: [f64; 3]) -> f32 {
    (z.atan2(x).to_degrees() - 90.0) as f32
}

pub fn amethyst(
    world: World<'_>,
    dragon: Entity<'_>,
    position: [f64; 3],
    direction: [f64; 3],
    damage: f32,
) {
    spawn(
        world,
        dragon,
        position,
        yaw(direction),
        Kind::Amethyst,
        damage,
        0.9,
    );
}

fn sword_pillars(at: [f64; 3], forward: [f64; 3]) -> [[f64; 3]; 3] {
    std::array::from_fn(|stage| {
        let distance = 3.0 + stage as f64 * 6.0;
        [
            at[0] + forward[0] * distance,
            at[1],
            at[2] + forward[2] * distance,
        ]
    })
}

/// DragonlordSwordAbility.tryUse: three independently ticking pillars at 3, 9, 15 blocks.
pub fn sword_wave(world: World<'_>, player: Entity<'_>, at: [f64; 3], forward: [f64; 3]) {
    for (stage, mut position) in sword_pillars(at, forward).into_iter().enumerate() {
        let Some(y) = world.surface_height(
            position[0].floor() as i32,
            position[2].floor() as i32,
            heightmap::MOTION_BLOCKING_NO_LEAVES,
        ) else {
            continue;
        };
        position[1] = f64::from(y);
        spawn(
            world,
            player,
            position,
            yaw(forward),
            Kind::Magma(stage as u32),
            18.0 + stage as f32 * 4.0,
            0.9 + stage as f64 * 0.2,
        );
    }
}

fn magma_sound(world: World<'_>, position: [f64; 3], first_hit: bool, player_owned: bool) {
    if let Some(effects) = EFFECTS.get() {
        let sound = if cfg!(feature = "original-assets") {
            "saintsdragons:ignivorus_magma_pillar"
        } else {
            "minecraft:entity.ender_dragon.growl"
        };
        let (volume, base_pitch) = if first_hit { (1.4, 0.8) } else { (1.2, 0.9) };
        effects.play_sound(
            world,
            position,
            sound,
            // Vanilla SoundSource.HOSTILE has ordinal 5.
            if player_owned && !first_hit {
                SOUND_PLAYERS
            } else {
                5
            },
            volume,
            base_pitch + crate::ai::random(1000) as f32 * 0.0002,
        );
    }
}

/// Both Java effects push horizontally away from the base, even at high Y.
fn push(kind: Kind, origin: [f64; 3], target: [f64; 3], strength: f64) -> [f64; 3] {
    let (x, z) = (target[0] - origin[0], target[2] - origin[2]);
    let length = x.hypot(z);
    let (x, z) = if length < 0.01 {
        (0.0, 1.0)
    } else {
        (x / length, z / length)
    };
    [x * strength, kind.lift(), z * strength]
}

fn eligible(world: World<'_>, owner: Option<i32>, player_owned: bool, target: Entity<'_>) -> bool {
    let (Some(mobs), Some(view)) = (MOBS.get(), target.view()) else {
        return false;
    };
    if view.kind & ENTITY_LIVING == 0
        || target.flags() & ENTITY_ALIVE == 0
        || target.is_attackable() != Some(true)
        || mobs.game_mode(target) == Some(3)
        || (!player_owned && mobs.game_mode(target) == Some(1))
        || owner == Some(target.id())
    {
        return false;
    }
    let Some(owner) = owner else { return true };
    if player_owned {
        // SwordAbilityTargeting excludes all tamed/owned animals, not just the
        // caster's pets, and checks the alliance in both directions.
        return !taming::is_tamed(target)
            && mobs.pet_owner(target).is_none()
            && ENTITIES
                .get()
                .and_then(|entities| {
                    entities.with_entity(world, owner, |player| {
                        !mobs.allied(player, target) && !mobs.allied(target, player)
                    })
                })
                .unwrap_or(true);
    }
    // DragonEntity.isAlly includes village defenders and trusted companions.
    if matches!(
        target.type_id().trim_start_matches("minecraft:"),
        "villager" | "wandering_trader" | "iron_golem"
    ) {
        return false;
    }
    ENTITIES
        .get()
        .and_then(|entities| {
            entities.with_entity(world, owner, |dragon| {
                !crate::allies::is_ally(world, dragon, target)
            })
        })
        .unwrap_or(true)
}

impl Pillar {
    fn impact(&mut self, world: World<'_>) {
        let (Some(entities), Some(mobs)) = (ENTITIES.get(), MOBS.get()) else {
            return;
        };
        let owner = mobs.by_uuid(world, &self.owner);
        let (min, max) = self.kind.bounds(self.position);
        let first = self.hits.is_empty();
        for id in entities.entities_matching(world, min, max, owner.unwrap_or(-1), 0) {
            entities.with_entity(world, id, |target| {
                let Some(uuid) = mobs.uuid(target) else {
                    return;
                };
                if self.hits.contains(&uuid) || !eligible(world, owner, self.player_owned, target) {
                    return;
                }
                self.hits.insert(uuid);
                if let Some(owner) = owner {
                    let source = if self.player_owned {
                        "minecraft:player_attack"
                    } else {
                        "minecraft:mob_attack"
                    };
                    target.hurt_by(owner, source, self.damage);
                } else {
                    let source = if matches!(self.kind, Kind::Magma(_)) {
                        "minecraft:hot_floor"
                    } else {
                        "minecraft:generic"
                    };
                    target.hurt(source, self.damage);
                }
                if matches!(self.kind, Kind::Magma(_)) {
                    mobs.set_on_fire(target, 80);
                }
                if let Some(view) = target.view() {
                    let impulse = push(self.kind, self.position, view.position, self.knockback);
                    target.change(&EntityChange {
                        mask: SET_MOTION,
                        motion: std::array::from_fn(|i| view.motion[i] + impulse[i]),
                        ..Default::default()
                    });
                }
            });
        }
        if first && !self.hits.is_empty() && matches!(self.kind, Kind::Magma(_)) {
            magma_sound(world, self.position, true, self.player_owned);
        }
    }
}

impl Pillar {
    /// The loaded carrier ticks independently of its owner; age and hits are saved.
    pub(crate) fn event(&mut self, event: &Event<'_>) -> bool {
        let world = event.world;
        let uuid = event.actor.uuid;
        if matches!(event.phase, Phase::Removed | Phase::Unloaded) {
            if let Some(id) = DISPLAYS
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&uuid)
            {
                vfx::remove(world, id);
            }
            return false;
        }
        if event.phase == Phase::Used {
            return false;
        }
        let (warmup, lifetime, subside) = self.kind.timing();
        let age = self.age;
        if !DISPLAYS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains_key(&uuid)
        {
            let (model, clip, scale) = self.kind.rig(age >= lifetime);
            if let Some(id) = vfx::pillar(world, model, clip, self.position, self.yaw, scale) {
                DISPLAYS
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert(uuid, id);
            }
        }
        if event.phase != Phase::Tick {
            return true;
        }
        self.age += 1;
        let age = self.age;
        if age >= lifetime + subside {
            event.actor.entity.discard();
            return false;
        }
        if (warmup..=lifetime).contains(&age) {
            self.impact(world);
        }
        if age == lifetime {
            if let Some(id) = DISPLAYS
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(&uuid)
                .copied()
            {
                let (model, clip, _) = self.kind.rig(true);
                vfx::pillar_clip(world, id, model, clip);
            }
        }
        // Supplement the model with the release's sparse amethyst particles
        // and an approximation of the magma impact emitter.
        if let Some(effects) = EFFECTS.get() {
            let [x, y, z] = self.position;
            if age == 1 && matches!(self.kind, Kind::Magma(_)) {
                effects.particles(
                    world,
                    "minecraft:lava",
                    [x, y + 0.02, z],
                    [0.35, 0.22, 0.35],
                    0.0,
                    64,
                );
            }
            if matches!(self.kind, Kind::Amethyst) && crate::ai::random(100) < 35 {
                effects.particles(
                    world,
                    "minecraft:end_rod",
                    [x, y + 1.5, z],
                    [0.325, 1.5, 0.325],
                    0.02,
                    1,
                );
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn released_pillar_timings_and_geometry() {
        assert_eq!(Kind::Magma(0).timing(), (5, 34, 9));
        assert_eq!(Kind::Amethyst.timing(), (4, 26, 10));
        let (min, max) = Kind::Magma(0).bounds([0.0; 3]);
        assert_eq!(min, [-2.9, -0.75, -2.9]);
        assert_eq!(max, [2.9, 7.15, 2.9]);
        assert_eq!(
            Kind::Amethyst.bounds([0.0; 3]),
            ([-4.5, -0.5, -4.5], [4.5, 7.5, 4.5])
        );
        assert!(Kind::Magma(2).bounds([0.0; 3]).1[1] > max[1]);
        assert_eq!(
            Kind::Magma(0).rig(false).1,
            "animation.ignivorus_magma_pillar.emerge"
        );
        assert_eq!(
            Kind::Magma(0).rig(true).1,
            "animation.ignivorus_magma_pillar.subside"
        );
        assert_eq!(Kind::Amethyst.rig(false).2, 2.0);
        assert_eq!(
            Kind::Amethyst.rig(true).1,
            "animation.amethyst_pillar_subside"
        );
    }

    #[test]
    fn push_ignores_height_and_handles_center() {
        assert_eq!(
            push(Kind::Magma(0), [0.0; 3], [0.0, 7.0, 0.0], 0.9),
            [0.0, 0.35, 0.9]
        );
        assert_eq!(
            push(Kind::Amethyst, [0.0; 3], [3.0, 7.0, 4.0], 5.0),
            [3.0, 0.32, 4.0]
        );
    }

    #[test]
    fn sword_pillars_follow_the_horizontal_look_at_released_distances() {
        assert_eq!(
            sword_pillars([1.5, 80.0, -2.5], [0.0, 0.0, 1.0]),
            [[1.5, 80.0, 0.5], [1.5, 80.0, 6.5], [1.5, 80.0, 12.5]]
        );
        assert_eq!(
            sword_pillars([1.5, 80.0, -2.5], [-1.0, 0.0, 0.0])[2],
            [-13.5, 80.0, -2.5]
        );
    }
}
