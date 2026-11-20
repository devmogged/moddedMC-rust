use std::collections::HashMap;

use pumpkin_data::{
    item::Item,
    packet::{CURRENT_MC_VERSION, PacketId, clientbound::play as p},
};
use pumpkin_protocol::{
    codec::item_stack_seralizer::ItemStackSerializer,
    ser::{NetworkReadExt as _, ReadingError},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Stack {
    item: &'static str,
    count: u8,
}

#[derive(Default)]
pub struct Inventory {
    player: HashMap<i16, Stack>,
    menu: HashMap<i16, Stack>,
    menu_slots: i16,
    cursor: Option<Stack>,
    selected: i16,
}

impl Inventory {
    pub fn observe(
        &mut self,
        packet: i32,
        mut bytes: &[u8],
        window: i32,
    ) -> Result<(), ReadingError> {
        match PacketId(packet) {
            p::OPEN_SCREEN | p::MOUNT_SCREEN_OPEN | p::CONTAINER_CLOSE => {
                self.close_menu();
            }
            p::CONTAINER_SET_CONTENT => {
                let id = bytes.get_container_id(&CURRENT_MC_VERSION)?.0;
                bytes.get_var_int()?;
                let count = bytes.get_var_int()?.0;
                if !(0..=4096).contains(&count) {
                    return Err(ReadingError::Message("invalid inventory slot count".into()));
                }
                if id == 0 || id == window {
                    self.slots(id).clear();
                    if id != 0 {
                        self.menu_slots = count as i16;
                    }
                    for slot in 0..count as i16 {
                        self.insert(id, slot, stack(&mut bytes)?);
                    }
                    let cursor = stack(&mut bytes)?;
                    if id == window {
                        self.cursor = Some(cursor);
                    }
                }
            }
            p::CONTAINER_SET_SLOT => {
                let id = bytes.get_container_id(&CURRENT_MC_VERSION)?.0;
                bytes.get_var_int()?;
                let slot = bytes.get_i16_be()?;
                if (id == 0 || id == window) && (0..4096).contains(&slot) {
                    self.insert(id, slot, stack(&mut bytes)?);
                }
            }
            p::SET_PLAYER_INVENTORY => {
                let raw = bytes.get_var_int()?.0;
                let slot = match raw {
                    0..=8 => raw + 36,
                    9..=35 => raw,
                    36..=39 => 44 - raw,
                    40 => 45,
                    _ => return Ok(()),
                };
                self.player.insert(slot as i16, stack(&mut bytes)?);
            }
            p::SET_CURSOR_ITEM => self.cursor = Some(stack(&mut bytes)?),
            p::SET_HELD_SLOT => {
                let selected = bytes.get_var_int()?.0;
                if (0..=8).contains(&selected) {
                    self.selected = selected as i16;
                }
            }
            p::RESPAWN => *self = Self::default(),
            _ => {}
        }
        Ok(())
    }

    pub const fn select(&mut self, slot: i16) {
        self.selected = slot;
    }

    pub fn holds(&self, item: &'static Item) -> bool {
        [36 + self.selected, 45].into_iter().any(|slot| {
            self.player
                .get(&slot)
                .is_some_and(|stack| stack.item == item.registry_key && stack.count > 0)
        })
    }

    pub fn close_menu(&mut self) {
        self.menu.clear();
        self.menu_slots = 0;
        self.cursor = None;
    }

    pub const fn predict_empty_cursor(&mut self) {
        self.cursor = Some(Stack {
            item: Item::AIR.registry_key,
            count: 0,
        });
    }

    fn insert(&mut self, id: i32, slot: i16, stack: Stack) {
        self.slots(id).insert(slot, stack);
        // Standard container menus end with the same 27 inventory and nine hotbar slots.
        let player_start = self.menu_slots - 36;
        if id != 0 && player_start >= 0 && (player_start..self.menu_slots).contains(&slot) {
            self.player.insert(9 + slot - player_start, stack);
        }
    }

    const fn slots(&mut self, id: i32) -> &mut HashMap<i16, Stack> {
        if id == 0 {
            &mut self.player
        } else {
            &mut self.menu
        }
    }

    pub fn check(&self, args: &str, window: i32) -> Result<String, String> {
        let args = args.split_whitespace().collect::<Vec<_>>();
        let (name, expected_item, expected_count, actual) = match args.as_slice() {
            ["cursor", item, count] => ("cursor".to_owned(), item, count, self.cursor),
            ["slot", slot, item, count] => {
                let slot = slot
                    .parse::<i16>()
                    .ok()
                    .filter(|&slot| (0..4096).contains(&slot))
                    .ok_or("invalid inventory slot")?;
                let slots = if window == 0 {
                    &self.player
                } else {
                    &self.menu
                };
                (
                    format!("slot {slot}"),
                    item,
                    count,
                    slots.get(&slot).copied(),
                )
            }
            _ => {
                return Err(
                    "expect requires slot <index> <item> <count> or cursor <item> <count>".into(),
                );
            }
        };
        let count = expected_count
            .parse::<u8>()
            .map_err(|_| "invalid expected item count")?;
        let item = Item::from_registry_key(expected_item)
            .ok_or_else(|| format!("unknown vanilla item {expected_item}"))?
            .registry_key;
        let actual =
            actual.ok_or_else(|| format!("server did not send {name} for menu {window}"))?;
        if actual == (Stack { item, count }) {
            Ok(format!("{name} {} {}", actual.item, actual.count))
        } else {
            Err(format!(
                "expected bot {name} {item} {count}, got {} {}",
                actual.item, actual.count
            ))
        }
    }
}

fn stack(bytes: &mut &[u8]) -> Result<Stack, ReadingError> {
    let stack = ItemStackSerializer::read_with_version(bytes, &CURRENT_MC_VERSION)?.0;
    Ok(Stack {
        item: stack.get_item().registry_key,
        count: stack.item_count,
    })
}
