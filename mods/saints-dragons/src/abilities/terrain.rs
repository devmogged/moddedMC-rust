//! Terrain effects from CindervaneFireBodyAbility, CindervaneFireballEntity and Cindervane.

use moddedmc_api::{
    Pos,
    content::{BlockUpdate, UNLOADED_STATE, UPDATE_ALL, UPDATE_KNOWN_SHAPE, World},
};

use crate::{CONTENT, MOBS, ai::random, flight::cell};

const IMPRINT_RADIUS: i32 = 9;

fn griefing(world: World<'_>) -> bool {
    crate::config::gameplay("dragonGriefingEnabled")
        && MOBS.get().is_some_and(|mobs| {
            mobs.boolean_gamerule(world, "minecraft:mob_griefing") == Some(true)
        })
}

fn block(world: World<'_>, pos: Pos) -> Option<u32> {
    let mut state = [UNLOADED_STATE];
    (world.read(&[pos], &mut state) && state[0] != UNLOADED_STATE).then_some(state[0])
}

fn is_air(state: u32) -> bool {
    CONTENT.get().is_some_and(|content| {
        ["minecraft:air", "minecraft:cave_air", "minecraft:void_air"]
            .iter()
            .any(|name| content.state(name) == Some(state))
    })
}

fn offset(pos: Pos, x: i32, y: i32, z: i32) -> Pos {
    Pos {
        x: pos.x + x,
        y: pos.y + y,
        z: pos.z + z,
    }
}

fn ignite(world: World<'_>, pos: Pos, sturdy: bool) {
    if !crate::config::gameplay("fireDragonBlockIgnitionEnabled") {
        return;
    }
    let Some(fire) = CONTENT.get().and_then(|c| c.state("minecraft:fire")) else {
        return;
    };
    let below = offset(pos, 0, -1, 0);
    if block(world, pos).is_some_and(is_air)
        && block(world, below).is_some_and(|state| !is_air(state))
        && (!sturdy || world.face_sturdy(below, 1))
        && world.can_survive(pos, fire) == Some(true)
    {
        world.write(&[BlockUpdate {
            position: pos,
            state: fire,
            flags: UPDATE_ALL | UPDATE_KNOWN_SHAPE,
        }]);
    }
}

fn unit_random() -> f64 {
    random(1 << 24) as f64 / f64::from(1 << 24)
}

/// CindervaneFireBodyAbility.applyFireAura: six samples each tick, with 12% ignition chance.
pub(super) fn aura(world: World<'_>, position: [f64; 3]) {
    if !griefing(world) {
        return;
    }
    for _ in 0..6 {
        let angle = unit_random() * std::f64::consts::TAU;
        let radius = 0.5 + unit_random() * 3.0;
        let height = unit_random() * 2.5;
        if unit_random() <= 0.12 {
            let pos = cell([
                position[0] + angle.cos() * radius,
                position[1] + 1.375 - 1.25 + height - 0.5,
                position[2] + angle.sin() * radius,
            ]);
            ignite(world, pos, true);
        }
    }
}

fn cooking_positions(position: [f64; 3]) -> impl Iterator<Item = Pos> {
    let center = cell(position);
    (-4..=4).flat_map(move |x| {
        (-4..=4).flat_map(move |y| {
            (-4..=4).filter_map(move |z| {
                let p = offset(center, x, y, z);
                let delta = [p.x, p.y, p.z].map(f64::from);
                ((0..3)
                    .map(|i| (delta[i] + 0.5 - position[i]).powi(2))
                    .sum::<f64>()
                    <= 3.5 * 3.5)
                    .then_some(p)
            })
        })
    })
}

/// DragonUtilities.accelerateCooking: each loaded block center in a 3.5 block sphere,
/// independent of griefing. Vanilla finishes the recipe on its own next tick.
pub(super) fn cook(world: World<'_>, position: [f64; 3]) -> bool {
    let Some(containers) = crate::CONTAINERS.get() else {
        return false;
    };
    let mut completed = false;
    for pos in cooking_positions(position) {
        let Some(state) = containers.furnace_heat(world, pos, 0, 0) else {
            continue;
        };
        if state.total == 0 {
            continue;
        }
        let boost = match state.kind {
            1 => 4,
            2 | 3 => 6,
            _ => continue,
        };
        completed |= containers
            .furnace_heat(world, pos, boost, 20)
            .is_some_and(|result| result.reached_end);
    }
    completed
}

#[test]
fn cooking_radius_uses_block_centers_in_three_dimensions() {
    let positions: Vec<_> = cooking_positions([0.5, 0.5, 0.5]).collect();
    assert!(positions.contains(&Pos { x: 3, y: 1, z: 1 }));
    assert!(!positions.contains(&Pos { x: 3, y: 2, z: 0 }));
    assert!(!positions.contains(&Pos { x: 0, y: 4, z: 0 }));
}

/// CindervaneFireballEntity.igniteArea: the 3 by 3 impact layer.
pub(crate) fn volley(world: World<'_>, position: [f64; 3]) {
    if !griefing(world) {
        return;
    }
    let center = cell(position);
    for dx in -1..=1 {
        for dz in -1..=1 {
            ignite(world, offset(center, dx, 0, dz), false);
        }
    }
}

fn imprint_depth(dx: i32, dz: i32) -> Option<i32> {
    let radius = f64::from(IMPRINT_RADIUS);
    let distance_sq = f64::from(dx * dx + dz * dz);
    (distance_sq <= radius * radius)
        .then(|| ((1.0 - distance_sq / (radius * radius)) * radius * 0.6).floor() as i32)
}

/// Cindervane.carveFireBodyImprint and the crash's final fire placement pass.
pub(super) fn crash(world: World<'_>, position: [f64; 3]) {
    if !griefing(world) {
        return;
    }
    let Some(content) = CONTENT.get() else { return };
    let immune = [
        content.state("minecraft:bedrock"),
        content.state("minecraft:obsidian"),
    ];
    let center = cell(position);
    for dx in -IMPRINT_RADIUS..=IMPRINT_RADIUS {
        for dz in -IMPRINT_RADIUS..=IMPRINT_RADIUS {
            let Some(depth) = imprint_depth(dx, dz) else {
                continue;
            };
            for dy in 0..=depth {
                let pos = offset(center, dx, -dy, dz);
                if let Some(state) = block(world, pos)
                    && !is_air(state)
                    && !immune.contains(&Some(state))
                {
                    world.destroy(pos, true);
                }
            }
        }
    }
    for dx in -3..=3 {
        for dz in -3..=3 {
            if unit_random() <= 0.45 {
                ignite(world, offset(center, dx, 0, dz), false);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crash_imprint_is_a_shallow_parabolic_bowl() {
        assert_eq!(imprint_depth(0, 0), Some(5));
        assert_eq!(imprint_depth(3, 4), Some(3));
        assert_eq!(imprint_depth(-3, -4), Some(3));
        assert_eq!(imprint_depth(9, 0), Some(0));
        assert_eq!(imprint_depth(9, 1), None);
    }
}
