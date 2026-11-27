use std::sync::atomic::Ordering;

use fastnbt::{ByteArray, Value};
use moddedmc_api::{
    Player,
    content::World,
    inventory_menu::{ANY_PLAYER_SLOT, Access, Action, ActionSlots, Change, Control, Target},
};
use serde::Serialize;

use crate::{
    INVENTORY_REVISION, RUNTIME, Runtime, archive, available,
    records::{Grave, PAGE_SLOTS, SavedStack, blob, unblob},
};

const INVENTORY_SLOTS: [i32; 45] = [
    39, 38, 37, 36, -1, 40, -1, -1, -1, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23,
    24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 0, 1, 2, 3, 4, 5, 6, 7, 8,
];
pub const RESTORE: u64 = 0;
pub const RESTORE_SLOT: u8 = 8;
const ADDITIONAL: u64 = 1;
const BACK: u64 = 2;
const PREVIOUS: u64 = 3;
const NEXT: u64 = 4;

struct Button {
    slot: u8,
    tag: u64,
    item: &'static str,
    title: &'static str,
}

const RESTORE_BUTTON: Button = Button {
    slot: RESTORE_SLOT,
    tag: RESTORE,
    item: "minecraft:hopper",
    title: "Transfer items",
};

const MAIN_CONTROLS: &[Button] = &[
    Button {
        slot: 7,
        tag: ADDITIONAL,
        item: "minecraft:chest",
        title: "Additional items",
    },
    RESTORE_BUTTON,
];
const PAGE_CONTROLS: &[Button] = &[
    Button {
        slot: 45,
        tag: BACK,
        item: "minecraft:chest",
        title: "Corpse inventory",
    },
    Button {
        slot: 48,
        tag: PREVIOUS,
        item: "minecraft:arrow",
        title: "Previous page",
    },
    Button {
        slot: 50,
        tag: NEXT,
        item: "minecraft:spectral_arrow",
        title: "Next page",
    },
    Button {
        slot: 53,
        tag: RESTORE,
        item: "minecraft:hopper",
        title: "Transfer items",
    },
];

#[derive(Clone, Copy)]
struct View {
    id: i64,
    page: i32,
}

impl View {
    fn from_inventory(encoded: u64) -> Option<Self> {
        Some(Self {
            id: i64::from(u32::try_from(encoded >> 32).ok()?),
            page: i32::try_from(encoded & u64::from(u32::MAX)).ok()?,
        })
    }

    fn inventory(self) -> Option<u64> {
        Some((u64::from(u32::try_from(self.id).ok()?) << 32) | u64::try_from(self.page).ok()?)
    }
}

pub fn open_inventory(runtime: Runtime, world: World<'_>, player: &Player<'_>, id: i64) -> bool {
    let page = with_grave(runtime, world, id, |grave| {
        if grave.items.is_empty() {
            grave.pages.first().map_or(0, |page| page.id)
        } else {
            0
        }
    });
    page.is_some_and(|page| open_view(runtime, world, player, View { id, page }))
}

fn with_grave<T>(
    runtime: Runtime,
    world: World<'_>,
    id: i64,
    read: impl FnOnce(&Grave) -> T,
) -> Option<T> {
    let dimension = world.dimension_id()?;
    archive(runtime, world, |current| {
        let grave = current
            .get_mut(id)
            .filter(|grave| available(grave, dimension))?;
        grave.migrate_overflow()?;
        Some(read(grave))
    })?
}

fn open_view(runtime: Runtime, world: World<'_>, player: &Player<'_>, view: View) -> bool {
    let Some(inventory) = view.inventory() else {
        return false;
    };
    let Some(title) = with_grave(runtime, world, view.id, |grave| {
        if view.page == 0 {
            return Some(format!("Corpse of {}", grave.owner));
        }
        let index = grave.pages.iter().position(|page| page.id == view.page)?;
        Some(format!(
            "Additional items ({}/{})",
            index + 1,
            grave.pages.len()
        ))
    })
    .flatten() else {
        return false;
    };

    let (access, buttons) = if view.page == 0 {
        (main_access(true).to_vec(), MAIN_CONTROLS)
    } else {
        (
            (0..54)
                .map(|slot| Access {
                    take: slot < PAGE_SLOTS,
                    insert: false,
                })
                .collect(),
            PAGE_CONTROLS,
        )
    };
    let controls: Vec<_> = buttons
        .iter()
        .map(|button| Control {
            slot: button.slot,
            tag: button.tag,
        })
        .collect();
    runtime.interfaces.menus.open(
        player,
        runtime.menu,
        inventory,
        &title,
        ActionSlots {
            world,
            access: &access,
            controls: &controls,
        },
        || {
            let mut slots = with_grave(runtime, world, view.id, |grave| {
                read_slots(grave, view.page)
            })??;
            for button in buttons {
                slots[usize::from(button.slot)] = Some(button_stack(button)?);
            }
            Some(slots)
        },
    )
}

fn read_slots(grave: &Grave, page: i32) -> Option<Vec<Option<Vec<u8>>>> {
    if page == 0 {
        return Some(inventory_slots(&grave.items));
    }
    let page = grave.pages.iter().find(|record| record.id == page)?;
    let mut items: Vec<_> = page
        .slots
        .iter()
        .take(PAGE_SLOTS)
        .map(|stack| (!stack.is_empty()).then(|| unblob(stack)))
        .collect();
    items.resize(54, None);
    Some(items)
}

pub fn inventory_slots(items: &[SavedStack]) -> Vec<Option<Vec<u8>>> {
    INVENTORY_SLOTS
        .iter()
        .map(|&slot| {
            items
                .iter()
                .find(|item| item.slot == slot)
                .map(SavedStack::bytes)
        })
        .collect()
}

pub fn main_access(take: bool) -> [Access; INVENTORY_SLOTS.len()] {
    INVENTORY_SLOTS.map(|slot| Access {
        take: take && slot >= 0,
        insert: false,
    })
}

#[derive(Serialize)]
struct Components {
    #[serde(rename = "minecraft:custom_name")]
    name: &'static str,
}

#[derive(Serialize)]
struct Item {
    id: &'static str,
    components: Components,
}

fn button_stack(button: &Button) -> Option<Vec<u8>> {
    fastnbt::to_bytes(&Item {
        id: button.item,
        components: Components { name: button.title },
    })
    .ok()
}

pub fn restore_button_stack() -> Option<Vec<u8>> {
    button_stack(&RESTORE_BUTTON)
}

pub fn clicked(action: Action<'_>) {
    if let Some(player_id) = crate::history::creative_player(action.inventory) {
        crate::history::transfer(action, player_id);
        return;
    }
    let (Some(&runtime), Some(view)) = (RUNTIME.get(), View::from_inventory(action.inventory))
    else {
        return;
    };
    if action.tag == RESTORE {
        restore(runtime, action, view);
        return;
    }
    let page = with_grave(runtime, action.world, view.id, |grave| match action.tag {
        BACK => Some(0),
        ADDITIONAL => grave.pages.first().map(|page| page.id),
        PREVIOUS => grave
            .pages
            .iter()
            .rfind(|page| page.id < view.page)
            .map(|page| page.id),
        NEXT => grave
            .pages
            .iter()
            .find(|page| page.id > view.page)
            .map(|page| page.id),
        _ => None,
    })
    .flatten();
    if let Some(page) = page {
        open_view(runtime, action.world, &action.player, View { page, ..view });
    }
}

pub fn matching_targets() -> Vec<Target> {
    (0_u8..41)
        .filter_map(|player_slot| {
            let menu_slot = INVENTORY_SLOTS
                .iter()
                .position(|&slot| slot == i32::from(player_slot))?;
            Some(Target {
                menu_slot: u8::try_from(menu_slot).ok()?,
                player_slot,
            })
        })
        .collect()
}

fn restore(runtime: Runtime, action: Action<'_>, view: View) {
    let Some(pages) = with_grave(runtime, action.world, view.id, |grave| {
        grave.pages.iter().map(|page| page.id).collect::<Vec<_>>()
    }) else {
        return;
    };
    let menus = runtime.interfaces.menus.menus();
    if view.page == 0 {
        menus.restore_matching(
            &action.player,
            runtime.menu,
            action.inventory,
            &matching_targets(),
        );
    }
    let loose: Vec<_> = (0..PAGE_SLOTS)
        .filter_map(|menu_slot| {
            Some(Target {
                menu_slot: u8::try_from(menu_slot).ok()?,
                player_slot: ANY_PLAYER_SLOT,
            })
        })
        .collect();
    let mut changed_view = false;
    for page in pages {
        let target = View { page, ..view };
        let Some(inventory) = target.inventory() else {
            continue;
        };
        if page == view.page && !changed_view {
            menus.restore_matching(&action.player, runtime.menu, inventory, &loose);
        } else if open_view(runtime, action.world, &action.player, target) {
            changed_view = true;
            menus.restore_matching(&action.player, runtime.menu, inventory, &loose);
        }
    }
    let first = with_grave(runtime, action.world, view.id, |grave| {
        grave.pages.first().map_or(0, |page| page.id)
    });
    if let Some(page) = first.filter(|&page| page != view.page || changed_view) {
        open_view(runtime, action.world, &action.player, View { page, ..view });
    }
}

pub fn changed(mut change: Change<'_>) {
    if let Some(player_id) = crate::history::creative_player(change.inventory) {
        crate::history::change(change, player_id);
        return;
    }
    let (Some(&runtime), Some(dimension), Some(view)) = (
        RUNTIME.get(),
        change.world.dimension_id(),
        View::from_inventory(change.inventory),
    ) else {
        return;
    };
    archive(runtime, change.world, |current| {
        let Some(grave) = current
            .get_mut(view.id)
            .filter(|grave| available(grave, dimension))
        else {
            return;
        };
        if grave.migrate_overflow().is_none() {
            return;
        }
        let overflow: Vec<_> = change.overflow().map(blob).collect();
        if view.page == 0 {
            grave.items = INVENTORY_SLOTS
                .into_iter()
                .zip(change.slots())
                .filter_map(|(slot, stack)| {
                    let stack = stack?;
                    (slot >= 0).then(|| SavedStack::new(slot, stack))
                })
                .collect();
        } else {
            let slots: Vec<_> = change
                .slots()
                .take(PAGE_SLOTS)
                .map(|stack| blob(stack.unwrap_or_default()))
                .collect();
            if !overflow.is_empty()
                && slots.iter().all(|stack| stack.is_empty())
                && grave
                    .pages
                    .iter()
                    .find(|page| page.id == view.page)
                    .is_some_and(|page| same_stacks(&page.slots, &overflow))
            {
                return;
            }
            if grave.save_page(view.page, slots).is_none() {
                return;
            }
        }
        if grave.append_overflow(overflow).is_none() {
            return;
        }
        if let Ok(bytes) = fastnbt::to_bytes(current)
            && change.commit(runtime.key, &bytes)
        {
            INVENTORY_REVISION.fetch_add(1, Ordering::Release);
        }
    });
}

fn same_stacks(before: &[ByteArray], after: &[ByteArray]) -> bool {
    let before = before.iter().filter(|stack| !stack.is_empty());
    before.clone().count() == after.len()
        && before.zip(after).all(|(left, right)| {
            if left == right {
                return true;
            }
            // The game's NBT writer can change compound key order between snapshots.
            matches!(
                (fastnbt::from_bytes::<Value>(&unblob(left)), fastnbt::from_bytes::<Value>(&unblob(right))),
                (Ok(left), Ok(right)) if left == right
            )
        })
}
