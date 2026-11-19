//! World generation in its smallest real use: zinc ore, as a Create port needs it. `ores:zinc_ore`
//! is a `blocks/1` block (seen as dead tube coral); `chunk_generated` places up to 8 per chunk in
//! stone between y 0 and 59, at positions picked from the chunk's coordinates. `/zinc` counts the
//! zinc ore in your chunk.

use moddedmc_api::{
    Command, Mod, Pos, RegistrationError, Registry, blocks::Definitions, info, world,
};

const ORE: &str = "ores:zinc_ore";
const PER_CHUNK: u64 = 8;

struct Ores;

/// A small deterministic mix (splitmix64) so a chunk always gets the same spots.
const fn mix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// Ore spots of a chunk.
fn spots(chunk_x: i32, chunk_z: i32) -> Vec<Pos> {
    let seed = (u64::from(chunk_x.cast_unsigned()) << 32) | u64::from(chunk_z.cast_unsigned());
    (0..PER_CHUNK)
        .map(|i| {
            let r = mix(seed ^ mix(i));
            Pos {
                x: chunk_x * 16 + (r & 15) as i32,
                y: ((r >> 4) % 60) as i32,
                z: chunk_z * 16 + ((r >> 12) & 15) as i32,
            }
        })
        .collect()
}

fn chunk_cube(chunk_x: i32, chunk_z: i32) -> Vec<Pos> {
    let mut cube = Vec::with_capacity(16 * 16 * 60);
    for y in 0..60 {
        for z in 0..16 {
            for x in 0..16 {
                cube.push(Pos {
                    x: chunk_x * 16 + x,
                    y,
                    z: chunk_z * 16 + z,
                });
            }
        }
    }
    cube
}

impl Mod for Ores {
    fn define_blocks(&self, blocks: &mut Definitions<'_>) -> Result<(), RegistrationError> {
        blocks.block(ORE, &[], &["minecraft:dead_tube_coral_block"])
    }

    fn register(&self, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
        registry.add_command("zinc", "Count the zinc ore in your chunk", 0)
    }

    fn chunk_generated(&self, dimension: &str, chunk_x: i32, chunk_z: i32) {
        if dimension != "minecraft:overworld" {
            return;
        }
        let Some(world) = world::get() else { return };
        let (Some(ore), Some(stone), Some(deepslate)) = (
            world.state_id(ORE),
            world.state_id("minecraft:stone"),
            world.state_id("minecraft:deepslate[axis=y]"),
        ) else {
            return;
        };
        let spots = spots(chunk_x, chunk_z);
        let writes: Vec<(Pos, u32)> = world
            .get_blocks(dimension, &spots)
            .into_iter()
            .zip(&spots)
            .filter(|(s, _)| *s == Some(stone) || *s == Some(deepslate))
            .map(|(_, p)| (*p, ore))
            .collect();
        let placed = world.set_blocks(dimension, &writes);
        info(&format!(
            "ores: decorated chunk {chunk_x} {chunk_z} with {placed}"
        ));
    }

    fn command(&self, _tag: u64, command: Command<'_>) -> i32 {
        let (Some(player), Some(world)) = (command.player, world::get()) else {
            return 0;
        };
        let Some(ore) = world.state_id(ORE) else {
            return 0;
        };
        let p = player.position();
        #[expect(clippy::cast_possible_truncation, reason = "block coordinates fit i32")]
        let (cx, cz) = ((p.x.floor() as i32) >> 4, (p.z.floor() as i32) >> 4);
        let count = world
            .get_blocks(player.dimension(), &chunk_cube(cx, cz))
            .into_iter()
            .filter(|s| *s == Some(ore))
            .count();
        command.reply(&format!("zinc: {count} in chunk {cx} {cz}"));
        i32::try_from(count).unwrap_or(0)
    }
}

moddedmc_api::export_mod!(Ores);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spots_are_in_the_chunk_and_stable() {
        let a = spots(-3, 7);
        assert_eq!(a, spots(-3, 7));
        assert_ne!(a, spots(7, -3));
        for p in &a {
            assert_eq!((p.x >> 4, p.z >> 4), (-3, 7));
            assert!((0..60).contains(&p.y));
        }
    }
}
