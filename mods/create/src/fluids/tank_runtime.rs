use std::collections::{HashMap, HashSet};

use fastnbt::{IntArray, Value};
use moddedmc_api::{
    Pos, RegistrationError, Registry,
    content::{self, Block, BlockUpdate, Content, State, World},
    interaction::{BlockUseResult, UseBlock},
};

use super::{network::FluidHandler, pipe_connection::FluidStack, smart_tank, tank::TankInventory};
use crate::{CONTENT, offset};

pub(super) const ID: &str = "create:fluid_tank";
pub(super) const CAPACITY_PER_BLOCK: u32 = 8_000;
pub(super) const MAX_HEIGHT: u16 = 32;
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

type TankTag = HashMap<String, Value>;

pub(super) fn new_tank() -> Option<TankInventory> {
    TankInventory::new(CAPACITY_PER_BLOCK, MAX_HEIGHT).ok()
}

pub(super) fn from_tag(data: &TankTag) -> Option<TankInventory> {
    let mut tank = new_tank()?;
    if data.contains_key("TankContent") {
        tank.read_controller_nbt(data).ok()?;
    }
    Some(tank)
}

pub(super) fn controller_from_tag(data: &TankTag, pos: Pos) -> Option<Pos> {
    match data.get("Controller") {
        None => Some(pos),
        Some(Value::IntArray(array)) => {
            let &[x, y, z] = array.as_ref() else {
                return None;
            };
            Some(Pos { x, y, z })
        }
        _ => None,
    }
}

pub(super) fn read_tag(world: World<'_>, pos: Pos) -> Option<TankTag> {
    let data = smart_tank::read(world, pos)?;
    matches!(data.get("id"), Some(Value::String(id)) if id == ID).then_some(data)
}

fn live_tag(world: World<'_>, pos: Pos) -> Option<TankTag> {
    let mut state = [content::UNLOADED_STATE];
    if !world.read(&[pos], &mut state) || !super::runtime::is_tank(state[0]) {
        return None;
    }
    read_tag(world, pos)
}

pub(super) fn controller(world: World<'_>, pos: Pos) -> Option<Pos> {
    controller_from_tag(&live_tag(world, pos)?, pos)
}

pub fn load(world: World<'_>, pos: Pos) -> Option<TankInventory> {
    let data = live_tag(world, pos)?;
    if data.contains_key("Controller") {
        return None;
    }
    from_tag(&data)
}

fn member_tag(mut data: TankTag, owner: Pos) -> TankTag {
    for key in ["TankContent", "Size", "Height", "Window"] {
        data.remove(key);
    }
    data.insert(
        "Controller".to_owned(),
        Value::IntArray(IntArray::new(vec![owner.x, owner.y, owner.z])),
    );
    data
}

pub fn save(world: World<'_>, pos: Pos, tank: &TankInventory) -> bool {
    let Some(data) = live_tag(world, pos) else {
        return false;
    };
    smart_tank::write(world, pos, &controller_tag(data, tank))
}

fn controller_tag(mut data: TankTag, tank: &TankInventory) -> TankTag {
    data.remove("Controller");
    data.extend(tank.write_controller_nbt());
    data
}

pub(super) fn parts(origin: Pos, width: u8, height: u16) -> impl Iterator<Item = Pos> {
    let w = i32::from(width);
    (0..i32::from(height))
        .flat_map(move |y| (0..w).flat_map(move |x| (0..w).map(move |z| offset(origin, [x, y, z]))))
}

fn part_states(origin: Pos, tank: &TankInventory, content: Content) -> Option<Vec<BlockUpdate>> {
    let width = tank.width();
    let height = tank.height();
    parts(origin, width, height)
        .map(|position| {
            let (x, z) = (position.x - origin.x, position.z - origin.z);
            let shape = match (tank.window(), width, x, z) {
                (true, 1, _, _) => "window",
                (true, 2, 0, 0) => "window_nw",
                (true, 2, 0, _) => "window_sw",
                (true, 2, _, 0) => "window_ne",
                (true, 2, _, _) => "window_se",
                (true, 3, _, _) if (x - z).abs() == 1 => "window",
                _ => "plain",
            };
            let bottom = position.y == origin.y;
            let top = position.y == origin.y + i32::from(height) - 1;
            let state = content.state(&format!("{ID}[bottom={bottom},shape={shape},top={top}]"))?;
            Some(BlockUpdate {
                position,
                state,
                flags: content::UPDATE_CLIENTS,
            })
        })
        .collect()
}

pub(super) fn update_groups(world: World<'_>, groups: &[(Pos, TankInventory)]) -> bool {
    let Some(&content) = CONTENT.get() else {
        return false;
    };
    let mut updates = Vec::new();
    let mut tags = Vec::new();
    for (owner, tank) in groups {
        let Some(states) = part_states(*owner, tank, content) else {
            return false;
        };
        for state in &states {
            let pos = state.position;
            let Some(old) = live_tag(world, pos) else {
                return false;
            };
            let new = if pos == *owner {
                controller_tag(old.clone(), tank)
            } else {
                member_tag(old.clone(), *owner)
            };
            tags.push((pos, old, new));
        }
        updates.extend(states);
    }
    let written = tags
        .iter()
        .take_while(|(pos, _, new)| smart_tank::write(world, *pos, new))
        .count();
    if written == tags.len() && world.write(&updates) == updates.len() {
        return true;
    }
    for (pos, old, _) in &tags[..written] {
        smart_tank::write(world, *pos, old);
    }
    false
}

pub(super) fn contains(origin: Pos, tank: &TankInventory, pos: Pos) -> bool {
    let width = i32::from(tank.width());
    (origin.x..origin.x + width).contains(&pos.x)
        && (origin.z..origin.z + width).contains(&pos.z)
        && (origin.y..origin.y + i32::from(tank.height())).contains(&pos.y)
}

pub fn canonical(world: World<'_>, pos: Pos) -> Option<(Pos, TankInventory)> {
    let data = live_tag(world, pos)?;
    let owner = controller_from_tag(&data, pos)?;
    let tank = if owner == pos {
        from_tag(&data)?
    } else {
        load(world, owner)?
    };
    contains(owner, &tank, pos).then_some((owner, tank))
}

pub fn describe(tank: &TankInventory) -> String {
    let fluid = tank.fluid().map_or("empty", |fluid| fluid.id.as_str());
    format!(" Tank({fluid} {}/{} mB)", tank.amount(), tank.capacity())
}

pub fn use_item_on(
    context: &mut UseBlock<'_>,
    connectivity: &super::tank_connectivity::Connectivity,
) -> BlockUseResult {
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
    let Some((pos, mut tank)) = canonical(world, context.position) else {
        return BlockUseResult::Fail;
    };
    if connectivity.pending(pos) {
        return BlockUseResult::Fail;
    }
    if id == crate::WRENCH {
        tank.set_window(!tank.window());
        return if parts(pos, tank.width(), tank.height())
            .all(|part| controller(world, part) == Some(pos))
            && update_groups(world, &[(pos, tank)])
        {
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
