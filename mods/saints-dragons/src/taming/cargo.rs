//! DragonInventoryMenu: two attachment slots and species-specific persistent cargo.
use super::{CHEST, SADDLE, Tamed, tamed, with_attachment_lock};
use crate::{ENTITIES, MOBS, STACKS};
use fastnbt::Value;
use moddedmc_api::{
    abi::MENU_ROUTE_PLAYER_FALLBACK,
    content::World,
    entities::{ENTITY_ALIVE, Entity},
    inventory_menu::{Access, ManagedChange, ManagedSlots, Validity},
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

const DATA: &str = "saintsdragons:cargo";
const MAX_CARGO: usize = 15;
const SLOTS: usize = 18;
type Stack = HashMap<String, Value>;

#[derive(Clone, Serialize, Deserialize)]
struct Cargo {
    revision: i32,
    slots: Vec<Stack>,
}

impl Default for Cargo {
    fn default() -> Self {
        Self {
            revision: 0,
            slots: vec![Stack::new(); MAX_CARGO],
        }
    }
}

impl Cargo {
    fn load(mob: Entity<'_>) -> Option<Self> {
        match mob.data(DATA) {
            None => Some(Self::default()),
            Some(bytes) => {
                let cargo: Self = fastnbt::from_bytes(&bytes).ok()?;
                (cargo.slots.len() == MAX_CARGO).then_some(cargo)
            }
        }
    }

    fn save(&self, mob: Entity<'_>) -> bool {
        fastnbt::to_bytes(self).is_ok_and(|bytes| mob.set_data(DATA, &bytes))
    }

    fn inventory(&self, mob: Entity<'_>, state: Tamed) -> u64 {
        (1 << 63)
            | (u64::from(state.chest) << 62)
            | (u64::from(state.saddle) << 61)
            | ((self.revision as u64 & 0x1fff_ffff) << 32)
            | u64::from(mob.id() as u32)
    }

    fn stacks(&self, state: Tamed) -> Option<Vec<Option<Vec<u8>>>> {
        let mut stacks = vec![Stack::new(); SLOTS];
        if state.saddle {
            stacks[0] = attachment(SADDLE);
        }
        if state.chest {
            stacks[1] = attachment(CHEST);
            stacks[2..2 + MAX_CARGO].clone_from_slice(&self.slots);
        }
        stacks
            .iter()
            .map(|stack| {
                if stack.is_empty() {
                    Some(None)
                } else {
                    fastnbt::to_bytes(stack).ok().map(Some)
                }
            })
            .collect()
    }
}

fn attachment(id: &str) -> Stack {
    HashMap::from([
        ("id".into(), Value::String(id.into())),
        ("count".into(), Value::Int(1)),
    ])
}

fn item(stack: &Stack) -> Option<&str> {
    match stack.get("id")? {
        Value::String(id) => Some(id),
        _ => None,
    }
}

fn capacity(mob: Entity<'_>) -> Option<usize> {
    match MOBS.get()?.kind(mob)? {
        "saintsdragons:cindervane" | "saintsdragons:stegonaut" => Some(15),
        "saintsdragons:atroxiia" => Some(10),
        _ => None,
    }
}

pub(crate) fn is_context(bytes: &[u8]) -> bool {
    bytes.len() == 18
}

fn context(mob: Entity<'_>, state: Tamed) -> Option<[u8; 18]> {
    let mut context = [0; 18];
    context[..16].copy_from_slice(&MOBS.get()?.uuid(mob)?);
    context[16] = capacity(mob)? as u8;
    context[17] = u8::from(state.saddle) | (u8::from(state.chest) << 1);
    Some(context)
}

fn access(context: &[u8; 18]) -> [Access; SLOTS] {
    let saddle = context[17] & 1 != 0;
    let chest = context[17] & 2 != 0;
    std::array::from_fn(|i| match i {
        0 => Access {
            take: saddle && !chest,
            insert: !saddle,
        },
        1 => Access {
            take: chest,
            insert: saddle && !chest,
        },
        _ => {
            let active = chest && i < 2 + usize::from(context[16]);
            Access {
                take: active,
                insert: active,
            }
        }
    })
}

fn accessible(player: Entity<'_>, mob: Entity<'_>, state: Tamed) -> bool {
    let Some(mobs) = MOBS.get() else { return false };
    mob.flags() & ENTITY_ALIVE != 0
        && mobs.uuid(player) == Some(state.owner)
        && mob.view().zip(player.view()).is_some_and(|(a, b)| {
            a.position
                .iter()
                .zip(b.position)
                .map(|(a, b)| (a - b).powi(2))
                .sum::<f64>()
                < 64.0
        })
}

pub(crate) fn open(world: World<'_>, rider: Entity<'_>, mob: Entity<'_>) {
    let Some(mobs) = MOBS.get() else { return };
    if mobs.passengers(mob).first() != Some(&rider.id()) {
        return;
    }
    let Some((menus, kind)) = crate::crucible::managed_menu() else {
        return;
    };
    let (Some(player), Some(state), Some(cargo)) =
        (rider.as_player(), tamed(mob), Cargo::load(mob))
    else {
        return;
    };
    let Some(context) = context(mob, state) else {
        return;
    };
    if !accessible(rider, mob, state) {
        return;
    }
    // shortcut: the shared managed menu cannot alter access during a click;
    // attachment transitions invalidate this view and the rider reopens it.
    let inventory = cargo.inventory(mob, state);
    menus.open(
        &player,
        kind,
        inventory,
        "Dragon Inventory",
        ManagedSlots {
            world,
            access: &access(&context),
            controls: &[],
            context: &context,
        },
        || {
            let current = Cargo::load(mob)?;
            let state = tamed(mob)?;
            (current.inventory(mob, state) == inventory)
                .then(|| current.stacks(state))
                .flatten()
        },
    );
}

fn matches(mob: Entity<'_>, state: Tamed, cargo: &Cargo, inventory: u64, expected: &[u8]) -> bool {
    context(mob, state)
        .as_ref()
        .is_some_and(|ctx| ctx == expected)
        && cargo.inventory(mob, state) == inventory
}

pub(crate) fn valid(view: Validity<'_>) -> bool {
    let Some(entities) = ENTITIES.get() else {
        return false;
    };
    view.player
        .with_entity_context(|world, rider| {
            world.id() == view.world.id()
                && entities.with_entity(world, view.inventory as i32, |mob| {
                    tamed(mob)
                        .zip(Cargo::load(mob))
                        .is_some_and(|(state, cargo)| {
                            accessible(rider, mob, state)
                                && matches(mob, state, &cargo, view.inventory, view.context)
                        })
                }) == Some(true)
        })
        .unwrap_or(false)
}

pub(crate) fn accepts(context: &[u8], slot: u8, bytes: &[u8]) -> bool {
    let Ok(context) = <&[u8; 18]>::try_from(context) else {
        return false;
    };
    let Some(access) = access(context).get(usize::from(slot)).copied() else {
        return false;
    };
    if !access.insert {
        return false;
    }
    match slot {
        0 | 1 => fastnbt::from_bytes::<Stack>(bytes)
            .ok()
            .is_some_and(|stack| item(&stack) == Some(if slot == 0 { SADDLE } else { CHEST })),
        _ => true,
    }
}

pub(crate) fn route(context: &[u8], bytes: &[u8]) -> u64 {
    for slot in 0..2 {
        if accepts(context, slot, bytes) {
            return 1 << slot;
        }
    }
    (2..SLOTS as u8)
        .filter(|&slot| accepts(context, slot, bytes))
        .fold(MENU_ROUTE_PLAYER_FALLBACK, |mask, slot| mask | (1 << slot))
}

fn installed(stack: &Stack, id: &str) -> Option<bool> {
    if stack.is_empty() {
        Some(false)
    } else if item(stack) == Some(id) && stack.get("count") == Some(&Value::Int(1)) {
        Some(true)
    } else {
        None
    }
}

pub(crate) fn changed(mut event: ManagedChange<'_>) {
    let (Some(entities), Some(mobs)) = (ENTITIES.get(), MOBS.get()) else {
        return;
    };
    let Some(proposed) = event
        .change
        .slots()
        .map(|slot| {
            slot.map_or_else(
                || Some(Stack::new()),
                |bytes| fastnbt::from_bytes(bytes).ok(),
            )
        })
        .collect::<Option<Vec<Stack>>>()
    else {
        return;
    };
    if proposed.len() != SLOTS || event.change.overflow().len() != 0 {
        return;
    }
    let (Some(saddle), Some(chest)) = (
        installed(&proposed[0], SADDLE),
        installed(&proposed[1], CHEST),
    ) else {
        return;
    };
    let world = event.change.world;
    entities.with_entity(world, event.change.inventory as i32, |mob| {
        let drops = with_attachment_lock(|| {
            let state = tamed(mob)?;
            let before = Cargo::load(mob)?;
            let cap = capacity(mob)?;
            if mob.flags() & ENTITY_ALIVE == 0
                || state.owner != event.change.player_uuid
                || !matches(mob, state, &before, event.change.inventory, event.context)
                || chest && !saddle
                || state.chest && !saddle
                || proposed[2 + cap..].iter().any(|s| !s.is_empty())
                || !state.chest && proposed[2..].iter().any(|s| !s.is_empty())
            {
                return None;
            }
            let mut after = before.clone();
            after.slots.clone_from_slice(&proposed[2..2 + MAX_CARGO]);
            let mut next = state;
            next.saddle = saddle;
            next.chest = chest;
            if next.saddle != state.saddle || next.chest != state.chest {
                after.revision = after.revision.wrapping_add(1);
            }
            let drops = if state.chest && !chest {
                std::mem::replace(&mut after.slots, vec![Stack::new(); MAX_CARGO])
            } else {
                Vec::new()
            };
            event
                .commit_external(|| {
                    if !after.save(mob) {
                        return false;
                    }
                    if next != state && !mobs.set_state(mob, &next.encode()) {
                        before.save(mob);
                        return false;
                    }
                    true
                })
                .then_some(drops)
        });
        if let Some(drops) = drops {
            drop_stacks(world, mob, drops);
        }
    });
}

pub(super) fn take_on_death(mob: Entity<'_>) -> Vec<Stack> {
    let Some(mut cargo) = Cargo::load(mob) else {
        return Vec::new();
    };
    let slots = std::mem::replace(&mut cargo.slots, vec![Stack::new(); MAX_CARGO]);
    cargo.revision = cargo.revision.wrapping_add(1);
    if cargo.save(mob) { slots } else { Vec::new() }
}

pub(super) fn drop_stacks(world: World<'_>, mob: Entity<'_>, stacks: Vec<Stack>) {
    let (Some(api), Some(view)) = (STACKS.get(), mob.view()) else {
        return;
    };
    for stack in stacks.iter().filter(|stack| !stack.is_empty()) {
        if let Ok(bytes) = fastnbt::to_bytes(stack) {
            api.spawn(world, view.position, [0.0; 3], &bytes);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_cargo_preserves_stack_components_and_empty_slots() {
        let mut cargo = Cargo::default();
        cargo.slots[9] = HashMap::from([
            ("id".into(), Value::String("minecraft:diamond".into())),
            ("count".into(), Value::Int(7)),
            (
                "components".into(),
                Value::Compound(HashMap::from([(
                    "minecraft:custom_name".into(),
                    Value::String("Precious".into()),
                )])),
            ),
        ]);
        let restored: Cargo = fastnbt::from_bytes(&fastnbt::to_bytes(&cargo).unwrap()).unwrap();
        assert_eq!(restored.slots, cargo.slots);
        assert!(restored.slots[10].is_empty());
    }

    #[test]
    fn chest_blocks_saddle_removal_and_inactive_cargo() {
        let mut ctx = [0; 18];
        ctx[16] = 10;
        ctx[17] = 3;
        let permissions = access(&ctx);
        assert!(!permissions[0].take);
        assert!(permissions[1].take && permissions[11].insert);
        assert!(!permissions[12].take && !permissions[12].insert);
        ctx[17] = 1;
        let permissions = access(&ctx);
        assert!(permissions[0].take && permissions[1].insert);
        assert!(!permissions[2].take && !permissions[2].insert);
    }
}
