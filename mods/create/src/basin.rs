//! Create 6.0.10's basin item inventories and world interaction.

use std::collections::HashMap;

use fastnbt::Value;
use moddedmc_api::{
    Pos, RegistrationError, Registry,
    content::{self, Block, Content, ContentEvent, ShapeBox, State, World},
    entities::{ENTITY_ALIVE, Entity},
    interaction::{BlockUseResult, UseBlock},
};

use crate::{
    EFFECTS, STACKS, STONE, StateIds,
    basin_inventory::{BASIN_SLOTS, BasinInventory, BasinStack},
    belt_inventory::next_int,
    fluids::{
        basin_tanks::{BasinTanks, container_exchange, is_container},
        pipe_connection::FluidStack,
    },
};

pub const ID: &str = "create:basin";
pub const FACINGS: [&str; 5] = ["down", "north", "south", "west", "east"];

pub fn register(content: Content, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
    let states = [
        ("down", 0),
        ("north", 180),
        ("south", 0),
        ("west", 90),
        ("east", 270),
    ];
    let properties = states.map(|(face, _)| format!("facing={face}"));
    let modeled: [State; 5] = std::array::from_fn(|index| State {
        properties: &properties[index],
        physical: STONE,
        model: if index == 0 {
            "create:block/basin/block"
        } else {
            "create:block/basin/block_directional"
        },
        rotating_model: "",
        x: 0,
        y: states[index].1,
        orientation: [0.0, 0.0, 0.0, 1.0],
        shape: &[
            ShapeBox {
                min: [0, 2, 0],
                max: [16, 16, 2],
            },
            ShapeBox {
                min: [0, 2, 14],
                max: [16, 16, 16],
            },
            ShapeBox {
                min: [0, 2, 2],
                max: [2, 16, 14],
            },
            ShapeBox {
                min: [14, 2, 2],
                max: [16, 16, 14],
            },
            ShapeBox {
                min: [2, 0, 2],
                max: [14, 2, 14],
            },
        ],
    });
    content.add_block(
        registry,
        &Block {
            id: ID,
            display_name: "Basin",
            entity: ID,
            item_model: "create:item/basin",
            states: &modeled,
        },
        None,
    )
}

pub fn add_states(content: Content, ids: &mut StateIds) {
    for face in FACINGS {
        ids.add(content, ID, &format!("facing={face}"), None);
    }
}

pub fn is_basin(states: &StateIds, id: u32) -> bool {
    states.by_id.get(&id).is_some_and(|entry| entry.block == ID)
}

#[derive(Clone)]
pub struct Basin {
    data: HashMap<String, Value>,
    inventories: [BasinInventory; 2],
}

impl Basin {
    pub fn load(world: World<'_>, pos: Pos) -> Option<Self> {
        let data: HashMap<String, Value> = fastnbt::from_bytes(&world.entity(pos)?).ok()?;
        let mut inventories = [BasinInventory::default(), BasinInventory::output()];
        inventories[0].load(data.get("InputItems"));
        inventories[1].load(data.get("OutputItems"));
        Some(Self { data, inventories })
    }

    pub fn save(&mut self, world: World<'_>, pos: Pos) -> bool {
        let [input, output] = &self.inventories;
        self.data.insert("InputItems".to_owned(), input.save());
        self.data.insert("OutputItems".to_owned(), output.save());
        fastnbt::to_bytes(&self.data).is_ok_and(|bytes| world.set_entity(pos, &bytes))
    }

    pub fn consume(&mut self, matches: impl Fn(&str) -> bool) -> bool {
        for inventory in &mut self.inventories {
            let slot = (0..BASIN_SLOTS)
                .find(|&slot| inventory.slot(slot).is_some_and(|item| matches(&item.id)));
            if let Some(slot) = slot {
                return inventory.extract(slot, 1, false).is_some();
            }
        }
        false
    }

    pub fn put_result(&mut self, item: &Value) -> bool {
        let Some(stack) = BasinStack::from_nbt(item) else {
            return false;
        };
        let output = &mut self.inventories[1];
        output.allow_insertion = true;
        let accepted = output.insert_stack(&stack, max_stack(item).unwrap_or_default());
        output.allow_insertion = false;
        accepted == stack.amount
    }

    pub fn consume_fluid(&mut self, matches: impl Fn(&FluidStack) -> bool, amount: u32) -> bool {
        self.modify_tanks(|tanks| tanks.drain_matching(matches, amount).then_some(()))
            .is_some()
    }

    pub fn put_fluid_result(&mut self, fluid: &FluidStack) -> bool {
        self.modify_tanks(|tanks| tanks.insert_result(fluid).then_some(()))
            .is_some()
    }

    fn modify_tanks<T>(&mut self, change: impl FnOnce(&mut BasinTanks) -> Option<T>) -> Option<T> {
        let mut tanks = BasinTanks::load(&self.data)?;
        let result = change(&mut tanks)?;
        tanks.store(&mut self.data);
        Some(result)
    }

    fn take_contents(&mut self) -> Vec<(usize, usize, BasinStack)> {
        self.inventories
            .iter_mut()
            .enumerate()
            .flat_map(|(inventory, stacks)| {
                (0..BASIN_SLOTS).filter_map(move |slot| {
                    Some((inventory, slot, stacks.extract(slot, u32::MAX, false)?))
                })
            })
            .collect()
    }
}

fn max_stack(item: &Value) -> Option<u32> {
    let bytes = fastnbt::to_bytes(item).ok()?;
    Some(STACKS.get()?.info(&bytes)?.max_count)
}

pub fn use_item_on(context: &mut UseBlock<'_>) -> BlockUseResult {
    let (world, pos) = (context.world, context.position);
    let Some(mut basin) = Basin::load(world, pos) else {
        return BlockUseResult::Pass;
    };
    if let Some(bytes) = context.stack.read() {
        let Ok(item) = fastnbt::from_bytes::<Value>(&bytes) else {
            return BlockUseResult::Pass;
        };
        let mut before = basin.clone();
        let Some(container) = basin.modify_tanks(|tanks| tanks.use_held(&item)) else {
            return if is_container(&item) {
                BlockUseResult::Success
            } else {
                BlockUseResult::TryEmptyHand
            };
        };
        if !basin.save(world, pos) {
            return BlockUseResult::Fail;
        }
        if !container_exchange(context, container) {
            before.save(world, pos);
            return BlockUseResult::Fail;
        }
        return BlockUseResult::Success;
    }
    let mut holding = basin.take_contents();
    if holding.is_empty() {
        return BlockUseResult::Success;
    }
    if !basin.save(world, pos) {
        return BlockUseResult::Fail;
    }
    let attempted = holding.len();
    holding.retain(|(_, _, item)| {
        !fastnbt::to_bytes(&item.nbt()).is_ok_and(|bytes| context.give(&bytes))
    });
    let given = holding.len() != attempted;
    if !holding.is_empty() {
        for (inventory, slot, item) in holding {
            basin.inventories[inventory].restore_slot(slot, item);
        }
        basin.save(world, pos);
    }
    if given && let Some(effects) = EFFECTS.get() {
        effects.play_sound(
            world,
            center(pos),
            "minecraft:entity.item.pickup",
            moddedmc_api::effects::SOUND_PLAYERS,
            0.2,
            1.0 + next_int(100) as f32 / 100.0,
        );
    }
    BlockUseResult::Success
}

pub fn on_landed(world: World<'_>, pos: Pos, entity: Entity<'_>) {
    let Some(view) = entity.view() else { return };
    if entity.flags() & ENTITY_ALIVE == 0
        || !view.on_ground
        || view.motion[1] > 0.0
        || view.position.map(|coord| coord.floor() as i32) != [pos.x, pos.y, pos.z]
    {
        return;
    }
    let Some(stacks) = STACKS.get() else { return };
    let Some(bytes) = stacks.entity_stack(entity) else {
        return;
    };
    let Some(max) = stacks.info(&bytes).map(|info| info.max_count) else {
        return;
    };
    let Ok(item) = fastnbt::from_bytes::<Value>(&bytes) else {
        return;
    };
    let Some(mut stack) = BasinStack::from_nbt(&item) else {
        return;
    };
    let Some(mut basin) = Basin::load(world, pos) else {
        return;
    };
    let before = basin.clone();
    let accepted = basin.inventories[0].insert_stack(&stack, max);
    if accepted == 0 {
        return;
    }
    stack.amount -= accepted;
    let bytes = if stack.amount == 0 {
        Vec::new()
    } else {
        let Ok(bytes) = fastnbt::to_bytes(&stack.nbt()) else {
            return;
        };
        bytes
    };
    if basin.save(world, pos) && !stacks.set_entity_stack(entity, &bytes) {
        let mut before = before;
        before.save(world, pos);
    }
}

pub fn replaced(world: World<'_>, event: &ContentEvent) {
    let Some(states) = crate::states() else {
        return;
    };
    if matches!(
        event.reason,
        content::UNLOADED | content::MOVED_STATE_CHANGED
    ) || !is_basin(states, event.previous)
        || is_basin(states, event.state)
    {
        return;
    }
    let Some((mut basin, stacks)) = Basin::load(world, event.position).zip(STACKS.get()) else {
        return;
    };
    for (_, _, stack) in basin.take_contents() {
        if let Ok(bytes) = fastnbt::to_bytes(&stack.nbt()) {
            stacks.spawn(world, center(event.position), [0.0, 0.125, 0.0], &bytes);
        }
    }
}

fn center(pos: Pos) -> [f64; 3] {
    [pos.x, pos.y, pos.z].map(|coord| f64::from(coord) + 0.5)
}
