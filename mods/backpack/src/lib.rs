//! `moddedmc:menus/1` in its smallest real use: a backpack. `/backpack` opens three rows of
//! storage that belong to you; what you leave in it is saved in `config/backpack/<name>.txt`
//! (one item per line, hex) when you close it. `/backpackseed` puts 5 diamonds in an empty first
//! slot, `/backpacklist` lists what is inside. `/smelter` opens a furnace screen whose bars the
//! mod drives (`menus/1` machine screens).

use std::fs;

use moddedmc_api::{Command, Mod, Player, RegistrationError, Registry, data_dir, menus, warn};

const ROWS: u32 = 3;
const KEY: &str = "backpack:";

struct Backpack;

fn file(name: &str) -> std::path::PathBuf {
    data_dir().join(format!("{name}.txt"))
}

fn load(name: &str) -> Vec<menus::Item> {
    fs::read_to_string(file(name))
        .unwrap_or_default()
        .lines()
        .map(|l| unhex(l).unwrap_or_default())
        .collect()
}

fn save(name: &str, items: &[menus::Item]) {
    let text: String = items.iter().map(|i| hex(i) + "\n").collect();
    if let Err(e) = fs::write(file(name), text) {
        warn(&format!("backpack: could not save {name}: {e}"));
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

impl Mod for Backpack {
    fn register(&self, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
        registry.add_command("backpack", "Open your backpack", 0)?;
        registry.add_command("backpackseed", "Put 5 diamonds in an empty first slot", 1)?;
        registry.add_command("backpacklist", "List what is in your backpack", 2)?;
        registry.add_command(
            "smelter",
            "Open a smelter screen, its progress half done",
            3,
        )?;
        Ok(())
    }

    fn command(&self, tag: u64, command: Command<'_>) -> i32 {
        let (Some(player), Some(menus)) = (command.player, menus::get()) else {
            return 0;
        };
        let name = player.name();
        if tag == 3 {
            // A machine screen: the mod owns its slots and drives its bars.
            let key = format!("{KEY}smelter:{name}");
            let opened = menus.open_machine(&player, "minecraft:furnace", "Smelter", &key, &[]);
            let set = opened
                && menus.set_property(&player, &key, 2, 100)
                && menus.set_property(&player, &key, 3, 200);
            command.reply(&format!("smelter: progress set {set}"));
            return i32::from(set);
        }
        let mut items = load(name);
        match tag {
            0 => i32::from(menus.open(&player, "Backpack", ROWS, &format!("{KEY}{name}"), &items)),
            1 => {
                if items.first().is_some_and(|i| !i.is_empty()) {
                    command.reply("backpack: the first slot is taken");
                    return 0;
                }
                let Some(diamonds) = menus.item("minecraft:diamond", 5) else {
                    return 0;
                };
                items.resize(items.len().max(1), Vec::new());
                items[0] = diamonds;
                save(name, &items);
                command.reply("backpack: seeded");
                1
            }
            _ => {
                let found: Vec<String> = items
                    .iter()
                    .filter_map(|i| menus.describe(i))
                    .map(|(id, count)| format!("{id} x{count}"))
                    .collect();
                command.reply(&format!("backpacklist: {}", found.join(", ")));
                i32::try_from(found.len()).unwrap_or(0)
            }
        }
    }

    fn menu_closed(&self, player: Player<'_>, key: &str, items: Vec<menus::Item>) {
        // The key names the owner, so a backpack closed by someone else still saves to its owner.
        let Some(name) = key.strip_prefix(KEY) else {
            return;
        };
        if name.starts_with("smelter:") {
            return;
        }
        save(name, &items);
        let stacks = items.iter().filter(|i| !i.is_empty()).count();
        player.send_message(&format!("backpack: saved {stacks} stacks"));
    }
}

moddedmc_api::export_mod!(Backpack);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trips() {
        assert_eq!(unhex(&hex(&[0, 0xab, 0xff])), Some(vec![0, 0xab, 0xff]));
        assert_eq!(unhex(""), Some(vec![]));
        assert_eq!(unhex("zz"), None);
    }
}
