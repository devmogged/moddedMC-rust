//! Option-free stock-client substitutes for released dragon particle effects.
use moddedmc_api::{
    Pos,
    content::World,
    entities::{Entity, EntityView},
    tick_count,
};

use crate::{EFFECTS, flight};

/// DragonEntity.spawnClientEventParticles: seven hearts or smoke across the body.
pub fn tame(world: World<'_>, mob: Entity<'_>, happy: bool) {
    let (Some(effects), Some(view), Some(body)) = (EFFECTS.get(), mob.view(), mob.body()) else {
        return;
    };
    let particle = if happy {
        "minecraft:heart"
    } else {
        "minecraft:smoke"
    };
    let [x, y, z] = view.position;
    let width = body.max[0] - body.min[0];
    let height = body.max[1] - body.min[1];
    for _ in 0..7 {
        let at = [
            x + (unit_random() * 2.0 - 1.0) * width,
            y + unit_random() * height + 0.5,
            z + (unit_random() * 2.0 - 1.0) * width,
        ];
        effects.particles(world, particle, at, [0.0; 3], 0.02, 1);
    }
}

fn unit_random() -> f64 {
    crate::ai::random(1_000_000) as f64 / 1_000_000.0
}

/// RideableFlyingDragon.tickNearGroundFlightDust. The host has no custom dust
/// texture or forced per-viewer delivery; cloud is a stock-client substitute.
pub fn flight_dust(world: World<'_>, mob: Entity<'_>, view: EntityView) {
    if view.on_ground
        || flight::landing(mob.id()) == Some(true)
        || !(tick_count().wrapping_add(mob.id() as u64)).is_multiple_of(3)
    {
        return;
    }
    let (Some(effects), Some(body)) = (EFFECTS.get(), mob.body()) else {
        return;
    };
    let [x, _, z] = view.position;
    let mut best: Option<[f64; 3]> = None;
    for x in [x, body.min[0] + 0.35, body.max[0] - 0.35] {
        for z in [z, body.min[2] + 0.35, body.max[2] - 0.35] {
            let start = flight::cell([x, body.min[1], z]);
            for y in (start.y - 10..=start.y).rev() {
                let pos = Pos { y, ..start };
                if world.fluid_id(pos).as_deref() != Some("minecraft:empty") {
                    return;
                }
                let Some(shape) = world.collision_shape(pos) else {
                    return;
                };
                let Some(top) = shape.iter().map(|b| b.max[1]).reduce(f64::max) else {
                    continue;
                };
                let ground = f64::from(y) + top;
                if body.min[1] - ground >= -0.25 && best.is_none_or(|p| ground > p[1]) {
                    best = Some([x, ground, z]);
                }
                break;
            }
        }
    }
    let Some([x, y, z]) = best else { return };
    let radius = ((body.max[0] - body.min[0]) * 0.42).max(0.9);
    let speed = view.motion[0].hypot(view.motion[2]);
    let drift = if speed > 0.003 {
        [
            -0.08 * view.motion[0] / speed,
            -0.08 * view.motion[2] / speed,
        ]
    } else {
        [0.0; 2]
    };
    for _ in 0..9 {
        let angle = unit_random() * std::f64::consts::TAU;
        let distance = radius * unit_random();
        let (sin, cos) = angle.sin_cos();
        let radial_speed = 0.04 + unit_random() * 0.07;
        effects.particles(
            world,
            "minecraft:cloud",
            [x + cos * distance, y + 0.08, z + sin * distance],
            [
                drift[0] + cos * radial_speed,
                0.03 + unit_random() * 0.06,
                drift[1] + sin * radial_speed,
            ],
            1.0,
            0,
        );
    }
}

/// RaevyxGroundRendAbility: start/end dust and the five trailing ground fragments.
pub fn rend(world: World<'_>, mob: Entity<'_>, view: EntityView, age: i32, direction: [f64; 3]) {
    let (Some(effects), Some(body)) = (EFFECTS.get(), mob.body()) else {
        return;
    };
    let width = body.max[0] - body.min[0];
    let [x, _, z] = view.position;
    let y = body.min[1] + 0.08;
    let burst = match age {
        1 => Some((32, 0.95, 0.28)),
        62 => Some((42, 1.15, 0.34)),
        _ => None,
    };
    if let Some((count, radius_scale, speed)) = burst {
        let radius = (width * radius_scale).max(1.4);
        effects.particles(
            world,
            "minecraft:cloud",
            [x, y, z],
            [radius * 0.5, 0.08, radius * 0.5],
            speed,
            count,
        );
    }
    if view.on_ground && (20..55).contains(&age) {
        let at = [
            x - direction[0] * width * 0.42,
            y,
            z - direction[2] * width * 0.42,
        ];
        effects.particles(
            world,
            "minecraft:cloud",
            at,
            [width * 0.26, 0.02, width * 0.26],
            0.1,
            5,
        );
    }
}

/// RaevyxDiveImpactAbility.spawnGroundDust: radial dust with released speed/count.
pub fn dive_impact(world: World<'_>, at: [f64; 3], power: f64) {
    let Some(effects) = EFFECTS.get() else { return };
    let count = dive_dust_count(power);
    let speed = 0.18 + power * 0.22;
    for i in 0..count {
        let angle = std::f64::consts::TAU * f64::from(i) / f64::from(count);
        effects.particles(
            world,
            "minecraft:cloud",
            [at[0], at[1] + 0.08, at[2]],
            [
                angle.cos() * speed,
                0.08 + power * 0.08,
                angle.sin() * speed,
            ],
            1.0,
            0,
        );
    }
    // shortcut: block-state particle options aren't available in EffectsV1.
    effects.particles(
        world,
        "minecraft:poof",
        [at[0], at[1] + 0.05, at[2]],
        [2.0 + power * 2.0, 0.25, 2.0 + power * 2.0],
        (0.22 + power * 0.15) as f32,
        35 + (power * 35.0) as u32,
    );
}

fn dive_dust_count(power: f64) -> u32 {
    18 + (power * 18.0) as u32
}

/// AtroxiiaHelheimQuakeAbility.performQuakeImpact: snowflakes approximate the custom ice burst.
pub fn quake(world: World<'_>, mob: Entity<'_>, age: i64) {
    if !matches!(age, 18 | 36) {
        return;
    }
    if let (Some(effects), Some(view)) = (EFFECTS.get(), mob.view()) {
        let [x, y, z] = view.position;
        effects.particles(
            world,
            "minecraft:snowflake",
            [x, y + 0.12, z],
            [2.0, 0.25, 2.0],
            0.3,
            32,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dive_dust_scales_with_impact_power() {
        assert_eq!(dive_dust_count(0.0), 18);
        assert_eq!(dive_dust_count(0.5), 27);
        assert_eq!(dive_dust_count(1.0), 36);
    }
}
