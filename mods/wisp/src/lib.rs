//! `moddedmc:entities/1` in its smallest real use: a creature the mod owns and moves. A wisp is
//! an armor stand named "Wisp"; the mod saves the ids of its wisps in `config/wisp/wisps.txt`
//! (they survive restarts with the world). `/wisp` spawns one where you stand, `/wispstep` moves
//! every wisp one block east, `/wisplist` lists where they are. `/wisppet` spawns a chicken with
//! its vanilla AI off, so it moves only when the mod moves it. `/wispgo` gives the pets their AI
//! back and walks them five blocks east along a path; `/wisparrived` counts those that got there.
//! `/wisphunt` spawns a husk that targets the first pet (which shrugs off its blows).

use std::{
    fs,
    sync::{Mutex, PoisonError},
};

use moddedmc_api::{
    Command, Mod, RegistrationError, Registry,
    abi::{EntityId, Vec3},
    data_dir, entities, info, warn,
};

struct Wisp {
    /// (dimension, id)
    wisps: Mutex<Vec<(String, EntityId)>>,
    /// Where `/wispgo` sent each walking wisp.
    targets: Mutex<Vec<(EntityId, Vec3)>>,
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
        registry.add_command("wisppet", "Spawn a chicken that only the mod moves", 3)?;
        registry.add_command("wispgo", "Walk every pet five blocks east", 4)?;
        registry.add_command("wisparrived", "Count the pets that reached their target", 5)?;
        registry.add_command("wisphunt", "Spawn a husk that hunts the first pet", 6)?;
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

    fn entity_event(&self, event: entities::Event<'_>) -> bool {
        if !self.is_mine(event.id) {
            return false;
        }
        match (event.kind, event.player) {
            (entities::EventKind::Use, Some(player)) => player.send_message("wisp: the wisp hums"),
            // Handled: a wisp takes no damage.
            (entities::EventKind::Attack, Some(player)) => {
                player.send_message("wisp: your hand passes through");
            }
            // Wisps and pets shrug off everything but /kill.
            (entities::EventKind::Damage { cause, .. }, _) if cause != "genericKill" => {
                info(&format!("wisp: shrugged off {cause}"));
            }
            (entities::EventKind::Death, _) => {
                info("wisp: a wisp died");
                let mut wisps = self.wisps.lock().unwrap_or_else(PoisonError::into_inner);
                wisps.retain(|(_, id)| *id != event.id);
                Self::save(&wisps);
                return false;
            }
            _ => return false,
        }
        true
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
            4 => {
                let mut targets = self.targets.lock().unwrap_or_else(PoisonError::into_inner);
                targets.clear();
                for (d, id) in wisps.iter().filter(|(d, _)| d == dimension) {
                    let Some(at) = entities.position(d, *id) else {
                        continue;
                    };
                    let to = Vec3 {
                        x: at.x + 5.0,
                        ..at
                    };
                    // Armor stands are not mobs: only pets walk.
                    if entities.set_ai(d, *id, true) && entities.walk_to(d, *id, to, 1.0) {
                        targets.push((*id, to));
                    }
                }
                command.reply(&format!("wispgo: walking {}", targets.len()));
                i32::try_from(targets.len()).unwrap_or(0)
            }
            6 => {
                // The first wisp that is a mob: a pet.
                let pet = wisps
                    .iter()
                    .filter(|(d, _)| d == dimension)
                    .find(|(d, id)| entities.set_ai(d, *id, true));
                let Some((at, pet)) =
                    pet.and_then(|(d, id)| Some((entities.position(d, *id)?, *id)))
                else {
                    command.reply("wisphunt: no pet");
                    return 0;
                };
                let near = Vec3 {
                    x: at.x - 3.0,
                    ..at
                };
                let hunting = entities
                    .spawn(dimension, "minecraft:husk", near)
                    .is_some_and(|husk| entities.set_target(dimension, husk, Some(pet)));
                command.reply(&format!("wisphunt: hunting {hunting}"));
                i32::from(hunting)
            }
            5 => {
                let targets = self.targets.lock().unwrap_or_else(PoisonError::into_inner);
                let arrived = targets
                    .iter()
                    .filter(|(id, to)| {
                        entities
                            .position(dimension, *id)
                            .is_some_and(|at| (at.x - to.x).hypot(at.z - to.z) < 2.0)
                    })
                    .count();
                command.reply(&format!("wisparrived: {arrived} of {}", targets.len()));
                i32::try_from(arrived).unwrap_or(0)
            }
            3 => {
                let at = player.position();
                let at = Vec3 {
                    x: at.x,
                    y: at.y.round(),
                    z: at.z,
                };
                let Some(id) = entities.spawn(dimension, "minecraft:chicken", at) else {
                    return 0;
                };
                // Its vanilla AI would wander off; the mod moves it with /wispstep instead.
                let off = entities.set_ai(dimension, id, false);
                entities.set_name(dimension, id, "Pet");
                wisps.push((dimension.to_owned(), id));
                Self::save(&wisps);
                command.reply(&format!("wisppet: ai off {off}"));
                i32::from(off)
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

impl Wisp {
    fn is_mine(&self, id: EntityId) -> bool {
        self.wisps
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .any(|(_, w)| *w == id)
    }
}

moddedmc_api::export_mod!(Wisp {
    wisps: Mutex::new(Vec::new()),
    targets: Mutex::new(Vec::new()),
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
