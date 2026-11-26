use std::sync::OnceLock;

use moddedmc_api::{
    Command, InterfaceUnavailable, Mod, RegistrationError, Registry,
    inventory_menu::{self, Access, Change, MenuKind, Menus, Target},
    saved_data::{self, Key},
};
use serde::{Deserialize, Serialize};

static MENUS: OnceLock<Menus> = OnceLock::new();
static KIND: OnceLock<MenuKind> = OnceLock::new();
static RECORD: OnceLock<Key> = OnceLock::new();
static OVERFLOW: OnceLock<Key> = OnceLock::new();

#[derive(Serialize)]
struct Stack {
    id: &'static str,
    count: i32,
}

#[derive(Deserialize)]
struct SavedStack {
    id: String,
    count: i32,
}

fn changed(mut change: Change<'_>) {
    if change.inventory == 1 {
        moddedmc_api::info("abi_inventory_menu rejected");
        return;
    }
    let (Some(&items), Some(&overflow_key)) = (RECORD.get(), OVERFLOW.get()) else {
        return;
    };
    if change.inventory == 4 {
        let save = saved_data::require().expect("saved data required in start");
        let items_empty = save
            .read(&change.world, items)
            .is_some_and(|data| data.is_empty());
        let helmet_saved = save
            .read(&change.world, overflow_key)
            .and_then(|data| fastnbt::from_bytes::<SavedStack>(&data).ok())
            .is_some_and(|stack| stack.id == "minecraft:iron_helmet" && stack.count == 1);
        if items_empty && helmet_saved {
            moddedmc_api::info("abi_inventory_menu saved slots and overflow survived restart");
        }
        return;
    }
    let overflow = change.overflow().len();
    let (key, record) = if change.inventory == 2 {
        (overflow_key, change.overflow().next().unwrap_or_default())
    } else {
        (items, change.slots().next().flatten().unwrap_or_default())
    };
    if change.commit(key, record) {
        moddedmc_api::info(&format!("abi_inventory_menu committed overflow={overflow}"));
    }
}

struct Fixture;

impl Mod for Fixture {
    fn start(&self) -> Result<(), InterfaceUnavailable> {
        let _ = MENUS.set(inventory_menu::require()?);
        saved_data::require()?;
        Ok(())
    }

    fn register(&self, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
        let menus = MENUS.get().expect("inventory menu required in start");
        let save = saved_data::require().map_err(|_| RegistrationError::InvalidDefinition)?;
        let _ = RECORD.set(save.register(registry, "fixture_items")?);
        let _ = OVERFLOW.set(save.register(registry, "fixture_overflow")?);
        let _ = KIND.set(menus.register(registry, changed)?);
        registry.add_command("abi_menu", "Open the fixture inventory", 0)?;
        registry.add_command("abi_menu_deny", "Open an inventory that declines edits", 1)?;
        registry.add_command("abi_menu_equipment", "Open matching equipment", 2)?;
        registry.add_command("abi_menu_restore", "Restore matching equipment", 3)?;
        registry.add_command("abi_menu_saved", "Inspect the saved fixture records", 4)?;
        moddedmc_api::info("abi_inventory_menu ready");
        Ok(())
    }

    fn command(&self, tag: u64, command: Command<'_>) -> i32 {
        let (Some(player), Some(&menus), Some(&kind)) = (command.player, MENUS.get(), KIND.get())
        else {
            return 0;
        };
        if tag == 3 {
            return i32::from(menus.restore_matching(
                &player,
                kind,
                2,
                &[Target {
                    menu_slot: 0,
                    player_slot: 39,
                }],
            ));
        }
        let item = if tag == 2 {
            Stack {
                id: "minecraft:diamond_helmet",
                count: 1,
            }
        } else {
            Stack {
                id: "minecraft:diamond",
                count: 7,
            }
        };
        let access = [Access {
            take: true,
            insert: false,
        }; 9];
        i32::from(
            menus.open(&player, kind, tag, "Fixture inventory", &access, || {
                let mut slots = vec![None; 9];
                slots[0] = Some(fastnbt::to_bytes(&item).ok()?);
                Some(slots)
            }),
        )
    }
}

moddedmc_api::export_mod!(Fixture, id = "abi_inventory_menu");
