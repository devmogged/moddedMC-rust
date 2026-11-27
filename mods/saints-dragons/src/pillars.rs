//! IgnivorusMagmaPillarEntity and StegonautAmethystPillarEntity, 0.9.88.
use crate::{EFFECTS, ENTITIES, MOBS, taming};
use moddedmc_api::{
    abi::heightmap,
    content::World,
    effects::SOUND_PLAYERS,
    entities::{ENTITY_ALIVE, ENTITY_LIVING, ENTITY_PLAYER, Entity, EntityChange, SET_MOTION},
    tick_count,
};
use std::{
    collections::{HashMap, HashSet},
    sync::{LazyLock, Mutex},
};

#[derive(Clone, Copy)]
enum Kind {
    Magma(u32),
    Amethyst,
}

impl Kind {
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

struct Pillar {
    kind: Kind,
    position: [f64; 3],
    owner: [u8; 16],
    player_owned: bool,
    born: u64,
    damage: f32,
    knockback: f64,
    hits: HashSet<[u8; 16]>,
}

// shortcut: the host lacks a tickable custom nonliving effect entity and its
// load callback; these world-ticked effects survive the owner's removal but
// do not persist across server restarts or pause on a chunk unload.
static ACTIVE: LazyLock<Mutex<HashMap<&'static str, Vec<Pillar>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn spawn(
    world: World<'_>,
    owner: Entity<'_>,
    position: [f64; 3],
    kind: Kind,
    damage: f32,
    knockback: f64,
) {
    let player_owned = owner.view().is_some_and(|v| v.kind & ENTITY_PLAYER != 0);
    let (Some(dimension), Some(owner)) =
        (world.dimension_id(), MOBS.get().and_then(|m| m.uuid(owner)))
    else {
        return;
    };
    if matches!(kind, Kind::Magma(_)) {
        magma_sound(world, position, false, player_owned);
    }
    ACTIVE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .entry(dimension)
        .or_default()
        .push(Pillar {
            kind,
            position,
            owner,
            player_owned,
            born: tick_count(),
            damage,
            knockback,
            hits: HashSet::new(),
        });
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
            Kind::Magma(stage),
            18.0 + stage as f32 * 4.0,
            0.9 + f64::from(stage) * 0.2,
        );
    }
}

pub fn amethyst(world: World<'_>, dragon: Entity<'_>, position: [f64; 3], damage: f32) {
    spawn(world, dragon, position, Kind::Amethyst, damage, 0.9);
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
        // shortcut: no native LivingEntity.attackable query yet.
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
    // shortcut: scoreboard team/allied checks and vanilla pet owners need host queries.
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
                !taming::is_owner(dragon, target)
                    && !taming::owner(dragon).is_some_and(|id| taming::owner(target) == Some(id))
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

/// The effect rises, hits once per target from warmup through lifetime inclusive,
/// and cannot be interacted with while it subsides.
pub fn tick(world: World<'_>, now: u64) {
    let Some(dimension) = world.dimension_id() else {
        return;
    };
    let Some(mut pillars) = ACTIVE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(dimension)
    else {
        return;
    };
    pillars.retain_mut(|pillar| {
        let age = now.saturating_sub(pillar.born);
        let (warmup, lifetime, subside) = pillar.kind.timing();
        if age >= lifetime + subside {
            return false;
        }
        if age == 0 {
            return true;
        }
        if (warmup..=lifetime).contains(&age) {
            pillar.impact(world);
        }
        // shortcut: particle silhouettes approximate the rise/subside until the
        // companion can render nonliving effect entities and synced animation state.
        if let Some(effects) = EFFECTS.get() {
            let fraction = rise(age, warmup, lifetime, subside);
            let (particle, height) = match pillar.kind {
                Kind::Magma(stage) => ("minecraft:flame", 5.5 * (1.0 + f64::from(stage) * 0.2)),
                Kind::Amethyst => ("minecraft:end_rod", 8.0),
            };
            let [x, y, z] = pillar.position;
            if age == 1 && matches!(pillar.kind, Kind::Magma(_)) {
                effects.particles(
                    world,
                    "minecraft:lava",
                    [x, y + 0.02, z],
                    [0.35, 0.22, 0.35],
                    0.0,
                    64,
                );
            }
            let height = height * fraction;
            effects.particles(
                world,
                particle,
                [x, y + height * 0.5, z],
                [0.3, height * 0.5, 0.3],
                0.02,
                4,
            );
        }
        true
    });
    if !pillars.is_empty() {
        ACTIVE
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry(dimension)
            .or_default()
            .append(&mut pillars);
    }
}

fn rise(age: u64, warmup: u64, lifetime: u64, subside: u64) -> f64 {
    if age > lifetime {
        (lifetime + subside).saturating_sub(age) as f64 / subside as f64
    } else {
        (age as f64 / warmup as f64).min(1.0)
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
        assert_eq!(rise(0, 5, 34, 9), 0.0);
        assert_eq!(rise(34, 5, 34, 9), 1.0);
        assert_eq!(rise(43, 5, 34, 9), 0.0);
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
