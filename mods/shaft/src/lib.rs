//! `shaft:shaft`, a real modded block through `moddedmc:blocks/1`: three states (`axis=x|y|z`)
//! that a stock client sees as end rods. Using it turns it to the next axis. `/shaftgive` puts
//! one in your hand; `/shaftcheck` lists the shafts around you. The scenario fixture for
//! `blocks/1`, and the first piece of the drivetrain proof.

use moddedmc_api::{
    Command, Mod, Pos, RegistrationError, Registry,
    blocks::{Definitions, Event, EventKind},
    info, world,
};

const ID: &str = "shaft:shaft";
const AXES: [&str; 3] = ["x", "y", "z"];
const RADIUS: i32 = 3;

struct Shaft;

fn at(p: Pos) -> String {
    format!("{} {} {}", p.x, p.y, p.z)
}

impl Mod for Shaft {
    fn define_blocks(&self, blocks: &mut Definitions<'_>) -> Result<(), RegistrationError> {
        blocks.block(
            ID,
            &[("axis", &AXES)],
            &[
                "minecraft:end_rod[facing=east]",
                "minecraft:end_rod[facing=up]",
                "minecraft:end_rod[facing=south]",
            ],
        )
    }

    fn register(&self, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
        registry.add_command("shaftgive", "Put a shaft in your hand", 0)?;
        registry.add_command("shaftcheck", "List the shafts around you", 1)
    }

    fn command(&self, tag: u64, command: Command<'_>) -> i32 {
        let Some(player) = command.player else {
            return 0;
        };
        if tag == 0 {
            return i32::from(player.give_block(ID));
        }
        let Some(world) = world::get() else {
            return 0;
        };
        let p = player.position();
        #[expect(clippy::cast_possible_truncation, reason = "block coordinates fit i32")]
        let center = Pos {
            x: p.x.floor() as i32,
            y: p.y.floor() as i32,
            z: p.z.floor() as i32,
        };
        let mut cube = Vec::new();
        for x in -RADIUS..=RADIUS {
            for y in -RADIUS..=RADIUS {
                for z in -RADIUS..=RADIUS {
                    cube.push(Pos {
                        x: center.x + x,
                        y: center.y + y,
                        z: center.z + z,
                    });
                }
            }
        }
        let found: Vec<String> = world
            .get_blocks(player.dimension(), &cube)
            .into_iter()
            .zip(&cube)
            .filter_map(|(s, &pos)| {
                let name = world.state_name(s?)?;
                name.starts_with(ID)
                    .then(|| format!("{name} at {}", at(pos)))
            })
            .collect();
        command.reply(&format!(
            "shaftcheck: {} found: {}",
            found.len(),
            found.join(", ")
        ));
        i32::try_from(found.len()).unwrap_or(0)
    }

    fn block_event(&self, event: Event<'_>) -> bool {
        let Some(world) = world::get() else {
            return false;
        };
        let name = world.state_name(event.state).unwrap_or_default();
        match event.kind {
            EventKind::Use => {
                let axis = AXES
                    .iter()
                    .position(|a| name.ends_with(&format!("axis={a}]")));
                let next = AXES[axis.map_or(0, |i| (i + 1) % AXES.len())];
                let Some(state) = world.state_id(&format!("{ID}[axis={next}]")) else {
                    return false;
                };
                world.set_blocks(event.dimension, &[(event.pos, state)]);
                if let Some(player) = event.player {
                    player.send_message(&format!(
                        "shaft: turned to axis={next} at {}",
                        at(event.pos)
                    ));
                }
                true
            }
            EventKind::Place => {
                if let Some(player) = event.player {
                    player.send_message(&format!("shaft: placed {name} at {}", at(event.pos)));
                }
                false
            }
            EventKind::Break => {
                info(&format!("shaft: broke {name} at {}", at(event.pos)));
                false
            }
            EventKind::Load => {
                info(&format!("shaft: loaded {name} at {}", at(event.pos)));
                false
            }
            EventKind::Tick | EventKind::Neighbor => false,
        }
    }
}

moddedmc_api::export_mod!(Shaft);
