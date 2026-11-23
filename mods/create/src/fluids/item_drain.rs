//! Create 6.0.10 ItemDrainBlock's manual exchange and drain-only fluid buffer.

use std::collections::HashMap;

use fastnbt::Value;
use moddedmc_api::{
    Pos, RegistrationError, Registry,
    content::{Block, Content, ShapeBox, State, World},
    interaction::{BlockUseResult, UseBlock},
};

use super::{network::FluidHandler, pipe_connection::FluidStack, tank::TankInventory};
use crate::processing_recipes::item_id;

pub const ID: &str = "create:item_drain";
const CAPACITY: u32 = 1_500;
const TANKS: &str = "Tanks";

pub fn register(content: Content, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
    content.add_block(
        registry,
        &Block {
            id: ID,
            display_name: "Item Drain",
            entity: ID,
            item_model: "create:item/item_drain",
            states: &[State {
                properties: "",
                physical: "minecraft:copper_block",
                model: "create:block/item_drain",
                rotating_model: "",
                x: 0,
                y: 0,
                orientation: [0.0, 0.0, 0.0, 1.0],
                shape: &[ShapeBox {
                    min: [0, 0, 0],
                    max: [16, 13, 16],
                }],
            }],
        },
        None,
    )
}

pub fn load(world: World<'_>, pos: Pos) -> Option<TankInventory> {
    let mut tank = TankInventory::new(CAPACITY, 1).ok()?;
    let data: HashMap<String, Value> = fastnbt::from_bytes(&world.entity(pos)?).ok()?;
    if let Some(tanks) = data.get(TANKS) {
        let Value::List(tanks) = tanks else {
            return None;
        };
        let Some(Value::Compound(first)) = tanks.first() else {
            return None;
        };
        tank.read_content_nbt(first.get("TankContent")?).ok()?;
    }
    Some(tank)
}

pub fn save(world: World<'_>, pos: Pos, tank: &TankInventory) -> bool {
    let Some(mut data) = world
        .entity(pos)
        .and_then(|data| fastnbt::from_bytes::<HashMap<String, Value>>(&data).ok())
    else {
        return false;
    };
    let segment = match data.remove(TANKS) {
        Some(Value::List(mut tanks)) => match tanks.pop() {
            Some(Value::Compound(segment)) => segment,
            _ => return false,
        },
        None => HashMap::new(),
        _ => return false,
    };
    let mut segment = segment;
    segment.insert("TankContent".to_owned(), tank.content_nbt());
    data.insert(
        TANKS.to_owned(),
        Value::List(vec![Value::Compound(segment)]),
    );
    fastnbt::to_bytes(&data).is_ok_and(|bytes| world.set_entity(pos, &bytes))
}

fn bucket(id: &str) -> Option<FluidStack> {
    let fluid = match id {
        "minecraft:water_bucket" => "minecraft:water",
        "minecraft:lava_bucket" => "minecraft:lava",
        "minecraft:milk_bucket" => "minecraft:milk",
        _ => return None,
    };
    Some(FluidStack {
        id: fluid.to_owned(),
        amount: 1_000,
        components: None,
    })
}

pub fn use_item_on(context: &mut UseBlock<'_>) -> BlockUseResult {
    let Some(bytes) = context.stack.read() else {
        return BlockUseResult::TryEmptyHand;
    };
    let Some(fluid) = fastnbt::from_bytes::<Value>(&bytes)
        .ok()
        .and_then(|item| bucket(item_id(&item)?))
    else {
        return BlockUseResult::TryEmptyHand;
    };
    let (world, pos) = (context.world, context.position);
    let Some(mut tank) = load(world, pos) else {
        return BlockUseResult::Fail;
    };
    if tank.fill(&fluid, true) != fluid.amount {
        return BlockUseResult::Success;
    }
    let output = HashMap::from([
        ("id", Value::String("minecraft:bucket".to_owned())),
        ("count", Value::Int(1)),
    ]);
    let Ok(output) = fastnbt::to_bytes(&output) else {
        return BlockUseResult::Fail;
    };
    tank.fill(&fluid, false);
    if !save(world, pos, &tank) {
        return BlockUseResult::Fail;
    }
    if !context.creative {
        let exchanged = if context.stack.count() == 1 {
            context.stack.write(&output)
        } else if context.give(&output) {
            context.stack.shrink(1);
            true
        } else {
            false
        };
        if !exchanged {
            tank.drain(&fluid, false);
            save(world, pos, &tank);
            return BlockUseResult::Fail;
        }
    }
    BlockUseResult::Success
}
