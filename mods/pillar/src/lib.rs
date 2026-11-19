//! `/pillar`: builds three sideways oak logs above the player through `moddedmc:world/1`, then
//! reads them back by name. The smallest real user of the interface, and its scenario fixture.
//! `/pillarstruct` places a structure template (a gold pillar) beside you.

use moddedmc_api::{Command, Mod, Pos, RegistrationError, Registry, world};

const HEIGHT: i32 = 3;

struct Pillar;

impl Mod for Pillar {
    fn register(&self, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
        registry.add_command("pillar", "Build a column of sideways logs above you", 0)?;
        registry.add_command("pillarstruct", "Place a gold pillar structure beside you", 1)
    }

    fn command(&self, tag: u64, command: Command<'_>) -> i32 {
        let Some(player) = command.player else {
            return 0;
        };
        let Some(world) = world::get() else {
            command.reply("This server does not offer moddedmc:world/1.");
            return 0;
        };
        if tag == 1 {
            let at = player.position();
            #[expect(clippy::cast_possible_truncation, reason = "block coordinates fit i32")]
            let origin = Pos {
                x: at.x.floor() as i32 + 2,
                y: at.y.floor() as i32,
                z: at.z.floor() as i32,
            };
            let placed = world.place_structure(player.dimension(), origin, &gold_pillar());
            command.reply(&format!(
                "pillarstruct: placed {} at {} {} {}",
                placed.map_or_else(|| "nothing".to_owned(), |n| n.to_string()),
                origin.x,
                origin.y,
                origin.z
            ));
            return placed.and_then(|n| i32::try_from(n).ok()).unwrap_or(0);
        }
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

/// A 1x3x1 structure template of gold blocks, in the NBT a structure block saves (written out
/// by hand: a real mod ships the `.nbt` file and uses `include_bytes!`).
fn gold_pillar() -> Vec<u8> {
    fn name(out: &mut Vec<u8>, tag: u8, name: &str) {
        out.push(tag);
        out.extend_from_slice(&u16::try_from(name.len()).unwrap_or(0).to_be_bytes());
        out.extend_from_slice(name.as_bytes());
    }
    fn int_list(out: &mut Vec<u8>, key: &str, values: [i32; 3]) {
        name(out, 9, key); // TAG_List
        out.push(3); // of TAG_Int
        out.extend_from_slice(&3i32.to_be_bytes());
        for v in values {
            out.extend_from_slice(&v.to_be_bytes());
        }
    }
    let mut out = Vec::new();
    name(&mut out, 10, ""); // root TAG_Compound
    int_list(&mut out, "size", [1, 3, 1]);
    name(&mut out, 9, "palette");
    out.push(10);
    out.extend_from_slice(&1i32.to_be_bytes());
    name(&mut out, 8, "Name"); // TAG_String
    let block = "minecraft:gold_block";
    out.extend_from_slice(&u16::try_from(block.len()).unwrap_or(0).to_be_bytes());
    out.extend_from_slice(block.as_bytes());
    out.push(0); // end of the palette entry
    name(&mut out, 9, "blocks");
    out.push(10);
    out.extend_from_slice(&3i32.to_be_bytes());
    for y in 0..3 {
        int_list(&mut out, "pos", [0, y, 0]);
        name(&mut out, 3, "state"); // TAG_Int
        out.extend_from_slice(&0i32.to_be_bytes());
        out.push(0);
    }
    name(&mut out, 9, "entities");
    out.push(0); // an empty list (of TAG_End)
    out.extend_from_slice(&0i32.to_be_bytes());
    out.push(0); // end of root
    out
}

moddedmc_api::export_mod!(Pillar);
