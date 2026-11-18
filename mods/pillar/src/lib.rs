//! `/pillar`: builds three sideways oak logs above the player through `moddedmc:world/1`, then
//! reads them back by name. The smallest real user of the interface, and its scenario fixture.

use moddedmc_api::{Command, Mod, Pos, RegistrationError, Registry, world};

const HEIGHT: i32 = 3;

struct Pillar;

impl Mod for Pillar {
    fn register(&self, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
        registry.add_command("pillar", "Build a column of sideways logs above you", 0)
    }

    fn command(&self, _tag: u64, command: Command<'_>) -> i32 {
        let Some(player) = command.player else {
            return 0;
        };
        let Some(world) = world::get() else {
            command.reply("This server does not offer moddedmc:world/1.");
            return 0;
        };
        let Some(log) = world.state_id("minecraft:oak_log[axis=x]") else {
            return 0;
        };
        let at = player.position();
        #[expect(clippy::cast_possible_truncation, reason = "block coordinates fit i32")]
        let base = Pos {
            x: at.x.floor() as i32,
            y: at.y.floor() as i32 + 2,
            z: at.z.floor() as i32,
        };
        let column: Vec<Pos> = (0..HEIGHT)
            .map(|dy| Pos {
                y: base.y + dy,
                ..base
            })
            .collect();
        let dimension = player.dimension();
        let written = world.set_blocks(
            dimension,
            &column.iter().map(|&p| (p, log)).collect::<Vec<_>>(),
        );
        let names: Vec<String> = world
            .get_blocks(dimension, &column)
            .into_iter()
            .map(|s| {
                s.and_then(|s| world.state_name(s))
                    .unwrap_or_else(|| "?".into())
            })
            .collect();
        command.reply(&format!(
            "pillar: wrote {written} at {} {} {}: {}",
            base.x,
            base.y,
            base.z,
            names.join(", ")
        ));
        i32::try_from(written).unwrap_or(0)
    }
}

moddedmc_api::export_mod!(Pillar);
