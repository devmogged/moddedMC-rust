use std::collections::{HashMap, HashSet};

use fastnbt::Value;
use moddedmc_api::{
    Pos, RegistrationError, Registry,
    content::{self, Block, BlockUpdate, Content, State, World},
    interaction::{BlockUseResult, UseBlock},
};

use super::{network::FluidHandler, pipe_connection::FluidStack, tank::TankInventory};
use crate::CONTENT;

const ID: &str = "create:fluid_tank";
const CAPACITY_PER_BLOCK: u32 = 8_000;
const MAX_HEIGHT: u16 = 32;
const BUCKET: u32 = 1_000;
const SHAPES: [&str; 6] = [
    "window",
    "plain",
    "window_nw",
    "window_sw",
    "window_ne",
    "window_se",
];

fn variants() -> impl Iterator<Item = (String, String)> {
    [
        (true, true, "single"),
        (true, false, "top"),
        (false, true, "bottom"),
        (false, false, "middle"),
    ]
    .into_iter()
    .flat_map(|(top, bottom, part)| {
        SHAPES.into_iter().map(move |shape| {
            let suffix = if shape == "plain" {
                String::new()
            } else {
                format!("_{shape}")
            };
            (
                format!("bottom={bottom},shape={shape},top={top}"),
                format!("create:block/fluid_tank/block_{part}{suffix}"),
            )
        })
    })
}

pub fn state_ids(content: Content) -> HashSet<u32> {
    variants()
        .filter_map(|(properties, _)| content.state(&format!("{ID}[{properties}]")))
        .collect()
}

pub fn register(content: Content, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
    let rows: Vec<_> = variants().collect();
    let states: Vec<_> = rows
        .iter()
        .map(|(properties, model)| State {
            properties,
            physical: "minecraft:copper_block",
            model,
            rotating_model: "",
            x: 0,
            y: 0,
            orientation: [0.0, 0.0, 0.0, 1.0],
            shape: &[],
        })
        .collect();
    content.add_block(
        registry,
        &Block {
            id: ID,
            display_name: "Fluid Tank",
            entity: ID,
            item_model: &rows[0].1,
            states: &states,
        },
        None,
    )
}

pub fn load(world: World<'_>, pos: Pos) -> Option<TankInventory> {
    let mut tank = TankInventory::new(CAPACITY_PER_BLOCK, MAX_HEIGHT).ok()?;
    let data: HashMap<String, Value> = fastnbt::from_bytes(&world.entity(pos)?).ok()?;
    if data.contains_key("Controller") {
        return None;
    }
    if data.contains_key("TankContent") {
        tank.read_controller_nbt(&data).ok()?;
    }
    Some(tank)
}

pub fn save(world: World<'_>, pos: Pos, tank: &TankInventory) -> bool {
    fastnbt::to_bytes(&tank.write_controller_nbt()).is_ok_and(|bytes| world.set_entity(pos, &bytes))
}

pub fn describe(tank: &TankInventory) -> String {
    let fluid = tank.fluid().map_or("empty", |fluid| fluid.id.as_str());
    format!(" Tank({fluid} {}/{} mB)", tank.amount(), tank.capacity())
}

pub fn use_item_on(context: &mut UseBlock<'_>) -> BlockUseResult {
    let Some(stack) = context
        .stack
        .read()
        .and_then(|data| fastnbt::from_bytes::<HashMap<String, Value>>(&data).ok())
    else {
        return BlockUseResult::TryEmptyHand;
    };
    let Some(Value::String(id)) = stack.get("id") else {
        return BlockUseResult::Pass;
    };
    let world = context.world;
    let pos = context.position;
    if id == crate::WRENCH {
        let Some((content, mut tank)) = CONTENT.get().zip(load(world, pos)) else {
            return BlockUseResult::Fail;
        };
        let window = !tank.window();
        let shape = if window { "window" } else { "plain" };
        let Some(state) = content.state(&format!("{ID}[bottom=true,shape={shape},top=true]"))
        else {
            return BlockUseResult::Fail;
        };
        if world.write(&[BlockUpdate {
            position: pos,
            state,
            flags: content::UPDATE_ALL,
        }]) != 1
        {
            return BlockUseResult::Fail;
        }
        tank.set_window(window);
        return if save(world, pos, &tank) {
            BlockUseResult::Success
        } else {
            BlockUseResult::Fail
        };
    }
    if !context.creative {
        return BlockUseResult::TryEmptyHand;
    }
    let fluid = match id.as_str() {
        "minecraft:water_bucket" => "minecraft:water",
        "minecraft:lava_bucket" => "minecraft:lava",
        "minecraft:bucket" => "",
        _ => return BlockUseResult::TryEmptyHand,
    };
    let Some(mut tank) = load(world, pos) else {
        return BlockUseResult::Fail;
    };
    let changed = if fluid.is_empty() {
        tank.amount() >= BUCKET && tank.drain_any(BUCKET, false).is_some()
    } else {
        let offered = FluidStack {
            id: fluid.to_owned(),
            amount: BUCKET,
            components: None,
        };
        tank.fill(&offered, true) == BUCKET && tank.fill(&offered, false) == BUCKET
    };
    if changed && !save(world, pos, &tank) {
        return BlockUseResult::Fail;
    }
    BlockUseResult::Success
}
