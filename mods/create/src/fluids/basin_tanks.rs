//! Basin SmartFluidTankBehaviour, two input tanks and two output tanks.

use std::collections::HashMap;

use fastnbt::Value;
use moddedmc_api::{Pos, content::World, interaction::UseBlock};

use super::{
    item_drain::emptying, network::FluidHandler, pipe_connection::FluidStack, smart_tank,
    spout::filled_container, tank::TankInventory,
};
use crate::processing_recipes::item_id;

const CAPACITY: u32 = 1_000;
const OUTPUT: usize = 0;
const INPUT: usize = 1;
const TANK_KEYS: [&str; 2] = ["OutputTanks", "InputTanks"];

pub struct BasinTanks([[TankInventory; 2]; 2]);

impl BasinTanks {
    pub fn load(data: &HashMap<String, Value>) -> Option<Self> {
        let empty = TankInventory::new(CAPACITY, 1).ok()?;
        let mut tanks = std::array::from_fn(|_| std::array::from_fn(|_| empty.clone()));
        for (group, key) in tanks.iter_mut().zip(TANK_KEYS) {
            let Some(Value::List(saved)) = data.get(key) else {
                continue;
            };
            for (tank, tag) in group.iter_mut().zip(saved) {
                let Value::Compound(tag) = tag else {
                    return None;
                };
                if let Some(content) = tag.get("TankContent") {
                    tank.read_content_nbt(content).ok()?;
                }
            }
        }
        Some(Self(tanks))
    }

    pub fn store(&self, data: &mut HashMap<String, Value>) {
        for (group, key) in self.0.iter().zip(TANK_KEYS) {
            let mut saved = match data.remove(key) {
                Some(Value::List(list)) => list,
                _ => Vec::new(),
            };
            saved.resize_with(group.len(), || Value::Compound(HashMap::new()));
            for (tank, entry) in group.iter().zip(&mut saved) {
                if let Value::Compound(tag) = entry {
                    tag.insert("TankContent".to_owned(), tank.content_nbt());
                }
            }
            data.insert(key.to_owned(), Value::List(saved));
        }
    }

    fn fill_slot(&self, fluid: &FluidStack, group: usize) -> Option<usize> {
        let tanks = &self.0[group];
        tanks
            .iter()
            .position(|tank| {
                tank.fluid()
                    .is_some_and(|stored| stored.same_fluid_same_components(fluid))
            })
            .or_else(|| tanks.iter().position(|tank| tank.amount() == 0))
    }

    fn fill(&mut self, fluid: &FluidStack, group: usize) -> u32 {
        let Some(slot) = self.fill_slot(fluid, group) else {
            return 0;
        };
        self.0[group][slot].fill(fluid, false)
    }

    pub fn insert_result(&mut self, fluid: &FluidStack) -> bool {
        self.fill(fluid, OUTPUT) == fluid.amount
    }

    pub fn drain_matching(
        &mut self,
        matches: impl Fn(&FluidStack) -> bool,
        mut amount: u32,
    ) -> bool {
        for tank in self.0.iter_mut().flatten() {
            if tank.fluid().is_some_and(&matches)
                && let Some(drained) = tank.drain_any(amount, false)
            {
                amount -= drained.amount;
                if amount == 0 {
                    return true;
                }
            }
        }
        false
    }

    pub fn use_held(&mut self, item: &Value) -> Option<Option<Value>> {
        if let Some((fluid, container)) = emptying(item) {
            let slot = self.fill_slot(&fluid, INPUT)?;
            let tank = &mut self.0[INPUT][slot];
            if tank.fill(&fluid, true) == fluid.amount {
                tank.fill(&fluid, false);
                return Some(Some(container));
            }
            return None;
        }
        let id = item_id(item)?;
        if id == "minecraft:sponge" {
            let mut drained = false;
            for tank in self.0.iter_mut().flatten() {
                drained |= tank.drain_any(u32::MAX, false).is_some();
            }
            return drained.then_some(None);
        }
        for tank in self.0.iter_mut().flatten() {
            let Some((amount, filled)) = tank.fluid().and_then(|fluid| filled_container(id, fluid))
            else {
                continue;
            };
            if tank.amount() >= amount {
                tank.drain_any(amount, false);
                return Some(Some(filled));
            }
        }
        None
    }
}

pub fn container_exchange(context: &mut UseBlock<'_>, result: Option<Value>) -> bool {
    let Some(result) = result else { return true };
    let Ok(bytes) = fastnbt::to_bytes(&result) else {
        return false;
    };
    if context.creative {
        true
    } else if context.stack.count() == 1 {
        context.stack.write(&bytes)
    } else if context.give(&bytes) {
        context.stack.shrink(1);
        true
    } else {
        false
    }
}

pub fn is_container(item: &Value) -> bool {
    emptying(item).is_some()
        || matches!(
            item_id(item),
            Some("minecraft:bucket" | "minecraft:glass_bottle")
        )
}

pub(super) struct BasinTransfer {
    position: Pos,
    group: usize,
    slot: usize,
}

impl BasinTransfer {
    pub(super) fn describe(world: World<'_>, position: Pos) -> Option<TankInventory> {
        let tanks = BasinTanks::load(&smart_tank::read(world, position)?)?;
        Some(
            tanks
                .0
                .iter()
                .flatten()
                .find(|tank| tank.amount() > 0)
                .unwrap_or(&tanks.0[0][0])
                .clone(),
        )
    }

    pub(super) fn source(
        world: World<'_>,
        position: Pos,
        request: &FluidStack,
    ) -> Option<(Self, TankInventory)> {
        let tanks = BasinTanks::load(&smart_tank::read(world, position)?)?;
        for (group, tanks) in tanks.0.into_iter().enumerate() {
            for (slot, tank) in tanks.into_iter().enumerate() {
                if tank
                    .fluid()
                    .is_some_and(|fluid| fluid.same_fluid_same_components(request))
                {
                    return Some((
                        Self {
                            position,
                            group,
                            slot,
                        },
                        tank,
                    ));
                }
            }
        }
        None
    }

    pub(super) fn target(
        world: World<'_>,
        position: Pos,
        fluid: &FluidStack,
    ) -> Option<(Self, TankInventory)> {
        let tanks = BasinTanks::load(&smart_tank::read(world, position)?)?;
        let slot = tanks.fill_slot(fluid, INPUT)?;
        Some((
            Self {
                position,
                group: INPUT,
                slot,
            },
            tanks.0[INPUT][slot].clone(),
        ))
    }

    pub(super) fn save(&self, world: World<'_>, tank: &TankInventory) -> bool {
        let Some(mut data) = smart_tank::read(world, self.position) else {
            return false;
        };
        let Some(mut tanks) = BasinTanks::load(&data) else {
            return false;
        };
        tanks.0[self.group][self.slot] = tank.clone();
        tanks.store(&mut data);
        smart_tank::write(world, self.position, &data)
    }
}
