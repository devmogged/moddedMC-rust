//! `moddedmc:entities/1` in its smallest real use: a creature the mod owns and moves. A wisp is
//! an armor stand named "Wisp"; the mod saves the ids of its wisps in `config/wisp/wisps.txt`
//! (they survive restarts with the world). `/wisp` spawns one where you stand, `/wispstep` moves
//! every wisp one block east, `/wisplist` lists where they are.

use std::{
    fs,
    sync::{Mutex, PoisonError},
};

use moddedmc_api::{
    Command, Mod, RegistrationError, Registry,
    abi::{EntityId, Vec3},
    data_dir, entities, warn,
};

struct Wisp {
    /// (dimension, id)
    wisps: Mutex<Vec<(String, EntityId)>>,
}

fn file() -> std::path::PathBuf {
    data_dir().join("wisps.txt")
}

fn hex(id: EntityId) -> String {
    id.0.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(s: &str) -> Option<EntityId> {
    let mut id = [0u8; 16];
    for (i, byte) in id.iter_mut().enumerate() {
        *byte = u8::from_str_radix(s.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(EntityId(id))
}

impl Wisp {
    fn save(wisps: &[(String, EntityId)]) {
        let text: String = wisps
            .iter()
            .map(|(d, id)| format!("{d} {}\n", hex(*id)))
            .collect();
        if let Err(e) = fs::write(file(), text) {
            warn(&format!("wisp: could not save: {e}"));
        }
    }
}

impl Mod for Wisp {
    fn register(&self, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
        registry.add_command("wisp", "Spawn a wisp where you stand", 0)?;
        registry.add_command("wispstep", "Move every wisp one block east", 1)?;
        registry.add_command("wisplist", "List the wisps", 2)?;
        let saved = fs::read_to_string(file()).unwrap_or_default();
        *self.wisps.lock().unwrap_or_else(PoisonError::into_inner) = saved
            .lines()
            .filter_map(|l| {
                let (d, id) = l.split_once(' ')?;
                Some((d.to_owned(), unhex(id)?))
            })
            .collect();
        Ok(())
    }

    fn command(&self, tag: u64, command: Command<'_>) -> i32 {
        let (Some(player), Some(entities)) = (command.player, entities::get()) else {
            return 0;
        };
        let dimension = player.dimension();
        let mut wisps = self.wisps.lock().unwrap_or_else(PoisonError::into_inner);
        match tag {
            0 => {
                let p = player.position();
                let at = Vec3 {
                    x: p.x,
                    // A player standing on a block can read just below its top: not inside it.
                    y: p.y.round(),
                    z: p.z,
                };
                let Some(id) = entities.spawn(dimension, "minecraft:armor_stand", at) else {
                    command.reply("wisp: could not spawn");
                    return 0;
                };
                entities.set_name(dimension, id, "Wisp");
                wisps.push((dimension.to_owned(), id));
                Self::save(&wisps);
                command.reply(&format!(
                    "wisp: spawned at {:.1} {:.1} {:.1}",
                    at.x, at.y, at.z
                ));
                1
            }
            1 => {
                let mut moved = 0;
                for (d, id) in wisps.iter().filter(|(d, _)| d == dimension) {
                    if let Some(at) = entities.position(d, *id) {
                        let to = Vec3 {
                            x: at.x + 1.0,
                            ..at
                        };
                        moved += i32::from(entities.move_to(d, *id, to, None));
                    }
                }
                command.reply(&format!("wispstep: moved {moved}"));
                moved
            }
            _ => {
                let found: Vec<String> = wisps
                    .iter()
                    .filter(|(d, _)| d == dimension)
                    .map(|(d, id)| {
                        entities.position(d, *id).map_or_else(
                            || "gone".to_owned(),
                            |at| format!("{:.1} {:.1} {:.1}", at.x, at.y, at.z),
                        )
                    })
                    .collect();
                command.reply(&format!("wisplist: {}", found.join(", ")));
                i32::try_from(found.len()).unwrap_or(0)
            }
        }
    }
}

moddedmc_api::export_mod!(Wisp {
    wisps: Mutex::new(Vec::new()),
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip() {
        let id = EntityId([0xab; 16]);
        assert_eq!(unhex(&hex(id)), Some(id));
        assert_eq!(unhex("zz"), None);
    }
}
