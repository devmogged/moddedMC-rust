//! `moddedmc:biomes/1` in its smallest real use: a biome of its own. `frost:glacier` plays like
//! snowy plains on the server; players see its pale sky, fog, grass and water. `/glacier` paints
//! it over the chunk you stand in, `/biomehere` names the biome at your feet.

use moddedmc_api::{
    Command, Mod, RegistrationError, Registry, abi::Pos, biomes, blocks::Definitions,
};

const GLACIER: &str = "frost:glacier";

/// The client's definition: a vanilla `worldgen/biome` file without features and spawns.
const JSON: &str = r##"{
  "has_precipitation": true,
  "temperature": -0.7,
  "downfall": 0.6,
  "effects": {
    "water_color": "#7fe0f0",
    "grass_color": "#b8e8f0",
    "foliage_color": "#a0d8e8"
  },
  "attributes": {
    "minecraft:visual/sky_color": "#c8ecff",
    "minecraft:visual/fog_color": "#e0f6ff"
  }
}"##;

struct Frost;

impl Mod for Frost {
    fn register(&self, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
        registry.add_command("glacier", "Paint the glacier biome over your chunk", 0)?;
        registry.add_command("biomehere", "Name the biome at your feet", 1)?;
        Ok(())
    }

    fn define_blocks(&self, defs: &mut Definitions<'_>) -> Result<(), RegistrationError> {
        defs.biome(GLACIER, "minecraft:snowy_plains", JSON)
    }

    fn command(&self, tag: u64, command: Command<'_>) -> i32 {
        let (Some(player), Some(biomes)) = (command.player, biomes::get()) else {
            return 0;
        };
        let dimension = player.dimension();
        let p = player.position();
        #[expect(clippy::cast_possible_truncation, reason = "world coordinates fit i32")]
        let (x, y, z) = (p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32);
        if tag == 1 {
            let name = biomes.at(dimension, Pos { x, y, z }).unwrap_or_default();
            command.reply(&format!("biomehere: {name}"));
            return 1;
        }
        // Every 4x4x4 cell of the chunk, 16 blocks above and below you.
        let (cx, cz) = (x & !15, z & !15);
        let mut painted = 0;
        for dy in (-16..=16).step_by(4) {
            for dx in (0..16).step_by(4) {
                for dz in (0..16).step_by(4) {
                    let at = Pos {
                        x: cx + dx,
                        y: y + dy,
                        z: cz + dz,
                    };
                    painted += i32::from(biomes.set(dimension, at, GLACIER));
                }
            }
        }
        command.reply(&format!("glacier: painted {painted}"));
        painted
    }
}

moddedmc_api::export_mod!(Frost);
