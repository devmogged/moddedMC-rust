//! Create 6.0.10 SpoutBlockEntity's timed container filling.

use std::collections::HashMap;

use fastnbt::Value;
use moddedmc_api::{
    Pos, RegistrationError, Registry,
    content::{self, Block, Content, ContentEvent, ShapeBox, State, World},
};

use super::{network::FluidHandler, pipe_connection::FluidStack, smart_tank, tank::TankInventory};
use crate::{
    belt_inventory::TransportedItemStack,
    processing_recipes::{ProcessingResult, item_id},
};

pub const ID: &str = "create:spout";
const TANK_CAPACITY: u32 = 1_000;
const FILLING_TIME: i32 = 20;
const FILL_TICK: i32 = 5;
const IDLE: i32 = -1;
const PROCESSING_TICKS: &str = "ProcessingTicks";

pub fn register(content: Content, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
    content.add_block(
        registry,
        &Block {
            id: ID,
            display_name: "Spout",
            entity: ID,
            item_model: "create:item/spout",
            states: &[State {
                properties: "",
                physical: "minecraft:copper_block",
                model: "create:block/spout/block",
                rotating_model: "",
                x: 0,
                y: 0,
                orientation: [0.0, 0.0, 0.0, 1.0],
                shape: &[
                    ShapeBox {
                        min: [1, 2, 1],
                        max: [15, 14, 15],
                    },
                    ShapeBox {
                        min: [2, 0, 2],
                        max: [14, 16, 14],
                    },
                ],
            }],
        },
        None,
    )
}

pub fn load_tank(world: World<'_>, pos: Pos) -> Option<TankInventory> {
    smart_tank::load(&smart_tank::read(world, pos)?, TANK_CAPACITY)
}

pub struct Spout {
    ticks: i32,
    filled_this_tick: bool,
}

impl Spout {
    fn load(world: World<'_>, pos: Pos) -> Self {
        let ticks = smart_tank::read(world, pos)
            .and_then(|data| match data.get(PROCESSING_TICKS) {
                Some(Value::Int(ticks)) => Some((*ticks).clamp(IDLE, FILLING_TIME)),
                _ => None,
            })
            .unwrap_or(IDLE);
        Self {
            ticks,
            filled_this_tick: false,
        }
    }

    pub fn process(
        &mut self,
        world: World<'_>,
        pos: Pos,
        item: &TransportedItemStack,
    ) -> ProcessingResult {
        use ProcessingResult::{Convert, Hold, Pass};
        if item.locked && (self.filled_this_tick || !matches!(self.ticks, IDLE | FILL_TICK)) {
            return Hold;
        }
        let Some(id) = item_id(&item.item) else {
            return Pass;
        };
        let recipes = crate::processing_recipes::current();
        if !matches!(id, "minecraft:bucket" | "minecraft:glass_bottle")
            && !recipes
                .as_ref()
                .is_some_and(|recipes| recipes.can_fill(&item.item))
        {
            return Pass;
        }
        let Some(mut data) = smart_tank::read(world, pos) else {
            return Pass;
        };
        let Some(mut tank) = smart_tank::load(&data, TANK_CAPACITY) else {
            return Pass;
        };
        let Some(fluid) = tank.fluid() else {
            return Hold;
        };
        let Some((amount, output)) = recipes
            .as_ref()
            .and_then(|recipes| recipes.fill(&item.item, fluid))
            .or_else(|| {
                filled_container(id, fluid).map(|(amount, output)| {
                    (
                        amount,
                        crate::processing_recipes::FillingResult::Item(output),
                    )
                })
            })
        else {
            return Pass;
        };
        if !item.locked || fluid.amount < amount {
            return Hold;
        }
        if self.ticks == IDLE {
            data.insert(PROCESSING_TICKS.to_owned(), Value::Int(FILLING_TIME));
            if smart_tank::write(world, pos, &data) {
                self.ticks = FILLING_TIME;
            }
            return Hold;
        }
        let mut request = fluid.clone();
        request.amount = amount;
        if tank.drain(&request, false).is_none()
            || !smart_tank::store(&mut data, &tank)
            || !smart_tank::write(world, pos, &data)
        {
            return Hold;
        }
        self.filled_this_tick = true;
        Convert {
            outputs: vec![output.finish()],
            lock_belt_outputs: false,
        }
    }
}

pub fn event(world: World<'_>, spouts: &mut HashMap<Pos, Spout>, event: &ContentEvent) {
    let pos = event.position;
    if event.reason == content::UNLOADED || !super::runtime::is_spout(event.state) {
        spouts.remove(&pos);
    } else if event.reason != content::NEIGHBOR_CHANGED {
        spouts.entry(pos).or_insert_with(|| Spout::load(world, pos));
    }
}

pub fn tick(world: World<'_>, spouts: &mut HashMap<Pos, Spout>) {
    for (&pos, spout) in spouts {
        spout.filled_this_tick = false;
        if spout.ticks < 0 {
            continue;
        }
        if let Some(mut data) = smart_tank::read(world, pos) {
            let next = spout.ticks - 1;
            data.insert(PROCESSING_TICKS.to_owned(), Value::Int(next));
            if smart_tank::write(world, pos, &data) {
                spout.ticks = next;
            }
        }
    }
}

pub(super) fn filled_container(id: &str, fluid: &FluidStack) -> Option<(u32, Value)> {
    let (amount, output_id) = match (id, fluid.id.as_str()) {
        ("minecraft:bucket", "minecraft:water") => (1_000, "minecraft:water_bucket"),
        ("minecraft:bucket", "minecraft:lava") => (1_000, "minecraft:lava_bucket"),
        ("minecraft:bucket", "minecraft:milk") => (1_000, "minecraft:milk_bucket"),
        ("minecraft:glass_bottle", "minecraft:water") => (250, "minecraft:potion"),
        _ => return None,
    };
    let mut stack = HashMap::from([
        ("id".to_owned(), Value::String(output_id.to_owned())),
        ("count".to_owned(), Value::Int(1)),
    ]);
    if output_id == "minecraft:potion" {
        stack.insert(
            "components".to_owned(),
            Value::Compound(HashMap::from([(
                "minecraft:potion_contents".to_owned(),
                Value::Compound(HashMap::from([(
                    "potion".to_owned(),
                    Value::String("minecraft:water".to_owned()),
                )])),
            )])),
        );
    }
    Some((amount, Value::Compound(stack)))
}
