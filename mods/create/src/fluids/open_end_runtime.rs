//! Create 6.0.10 OpenEndedPipe's in-world transfer and per-face buffer.

use std::{collections::HashMap, sync::OnceLock};

use fastnbt::Value;
use moddedmc_api::{
    InterfaceUnavailable,
    content::{self, BlockUpdate, World},
    fluid_world::{self, FluidCell, FluidWorld},
};

use super::{
    network::FluidHandler,
    network_graph::BlockFace,
    open_end::{self, DrainDecision, FillDecision, FluidKind},
    pipe_connection::FluidStack,
    smart_tank,
    tank::TankInventory,
    vanilla_targets::{self, Fluid, Target},
};
use crate::{CONTENT, DIRECTIONS, EFFECTS, TAGS, belt_inventory::next_int};

const BUCKET: u32 = 1_000;
static FLUID_WORLD: OnceLock<FluidWorld> = OnceLock::new();
static SPECIAL: OnceLock<HashMap<u32, (Target, Option<u32>)>> = OnceLock::new();

pub fn require() -> Result<(), InterfaceUnavailable> {
    let _ = FLUID_WORLD.set(fluid_world::require_tick_control()?);
    Ok(())
}

pub(super) fn fluids() -> Option<FluidWorld> {
    FLUID_WORLD.get().copied()
}

pub(super) fn inspect(world: World<'_>, pos: moddedmc_api::Pos) -> Option<FluidCell> {
    fluids()?.inspect(world, pos)
}

fn specials() -> Option<&'static HashMap<u32, (Target, Option<u32>)>> {
    let content = CONTENT.get()?;
    Some(SPECIAL.get_or_init(|| {
        let mut states = HashMap::new();
        let empty_cauldron = content.state("minecraft:cauldron");
        if let Some(id) = empty_cauldron {
            states.insert(id, (Target::EmptyCauldron, None));
        }
        if let Some(id) = content.state("minecraft:lava_cauldron") {
            states.insert(id, (Target::LavaCauldron, empty_cauldron));
        }
        for level in 1..=3 {
            if let Some(id) = content.state(&format!("minecraft:water_cauldron[level={level}]")) {
                states.insert(id, (Target::WaterCauldron { level }, empty_cauldron));
            }
        }
        for name in ["beehive", "bee_nest"] {
            for direction in &DIRECTIONS[2..] {
                let empty = content.state(&format!(
                    "minecraft:{name}[facing={direction},honey_level=0]"
                ));
                for level in 0..=5 {
                    if let Some(id) = content.state(&format!(
                        "minecraft:{name}[facing={direction},honey_level={level}]"
                    )) {
                        states.insert(id, (Target::Honey { level }, empty));
                    }
                }
            }
        }
        states
    }))
}

fn cell_plan(cell: &FluidCell) -> open_end::FluidCell<'_> {
    open_end::FluidCell {
        loaded: true,
        replaceable: cell.replaceable,
        has_waterlogged_property: cell.has_waterlogged_property,
        fluid: cell.fluid.as_deref(),
        source: cell.source,
    }
}

pub fn open_cell(world: World<'_>, face: BlockFace) -> Option<FluidCell> {
    let pos = face.connected_pos();
    let cell = FLUID_WORLD.get()?.inspect(world, pos)?;
    if specials()?.contains_key(&cell.state) {
        return Some(cell);
    }
    if world.face_sturdy(pos, face.side ^ 1) {
        let state = CONTENT.get()?.describe_state(cell.state)?;
        let id = state.split('[').next()?;
        if !TAGS.get()?.contains("block", "create:fan_transparent", id) {
            return None;
        }
    }
    (cell.has_waterlogged_property
        || (cell.replaceable && world.destroy_speed(pos).is_some_and(|speed| speed >= 0.0)))
    .then_some(cell)
}

#[derive(Clone, Copy)]
enum Change {
    Keep,
    Set(u32),
    Liquid(u32),
    Waterlog(u32),
    Evaporate,
}

pub(super) fn waterlogged_state(cell: &FluidCell, wet: bool) -> Option<u32> {
    let content = CONTENT.get()?;
    let state = content.describe_state(cell.state)?;
    let selector = if wet {
        state.replace("waterlogged=false", "waterlogged=true")
    } else {
        state.replace("waterlogged=true", "waterlogged=false")
    };
    content.state(&selector)
}

fn source_plan(
    world: World<'_>,
    face: BlockFace,
    cell: &FluidCell,
) -> Option<(FluidStack, Change)> {
    if let Some(&(target, Some(state))) = specials()?.get(&cell.state)
        && let Some(drained) = vanilla_targets::drain_block(target, false)
    {
        let id = match drained.fluid {
            Fluid::Water => "minecraft:water",
            Fluid::Lava => "minecraft:lava",
            Fluid::Honey => "create:honey",
        };
        return Some((fluid_stack(id, drained.amount), Change::Set(state)));
    }
    let id = cell.fluid.as_deref()?;
    let change = match open_end::plan_source_drain(cell_plan(cell), false) {
        DrainDecision::RemoveWaterlogging => Change::Waterlog(waterlogged_state(cell, false)?),
        DrainDecision::ReplaceWithFlowing => {
            if FLUID_WORLD
                .get()?
                .would_regenerate_source(world, face.connected_pos(), id)?
            {
                Change::Keep
            } else {
                Change::Liquid(CONTENT.get()?.state(&format!("{id}[level=14]"))?)
            }
        }
        _ => return None,
    };
    Some((fluid_stack(id, BUCKET), change))
}

fn fluid_stack(id: &str, amount: u32) -> FluidStack {
    FluidStack {
        id: id.to_owned(),
        amount,
        components: None,
    }
}

fn target_plan(
    world: World<'_>,
    face: BlockFace,
    cell: &FluidCell,
    fluid: &FluidStack,
) -> Option<Change> {
    let id = fluid.id.as_str();
    let water = id == "minecraft:water";
    let vanilla_fluid = water || id == "minecraft:lava";
    let kind = FluidKind {
        id,
        amount: fluid.amount,
        flowing: vanilla_fluid,
        has_block_state: vanilla_fluid,
        water_tag: water,
    };
    let output_pos = face.connected_pos();
    let evaporates = FLUID_WORLD.get()?.water_evaporates(world, output_pos);
    match open_end::plan_fill(cell_plan(cell), kind, true, evaporates, false) {
        FillDecision::PlaceSource => Some(Change::Liquid(CONTENT.get()?.state(id)?)),
        FillDecision::Waterlog => Some(Change::Waterlog(waterlogged_state(cell, true)?)),
        FillDecision::Evaporate => Some(Change::Evaporate),
        FillDecision::ReactWithExisting => {
            let block = match (id, cell.fluid.as_deref(), cell.source) {
                ("minecraft:water", Some("minecraft:lava"), true) => "minecraft:obsidian",
                ("minecraft:lava", Some("minecraft:water"), true) => "minecraft:stone",
                _ => "minecraft:cobblestone",
            };
            if let Some(state) = CONTENT.get()?.state(block) {
                apply(world, face, cell.state, Change::Set(state));
            }
            None
        }
        _ => None,
    }
}

fn apply(world: World<'_>, face: BlockFace, expected: u32, change: Change) -> bool {
    let position = face.connected_pos();
    let mut states = [content::UNLOADED_STATE];
    if !world.read(&[position], &mut states) || states[0] != expected {
        return false;
    }
    let state = match change {
        Change::Keep => return true,
        Change::Set(state) | Change::Liquid(state) | Change::Waterlog(state) => state,
        Change::Evaporate => {
            if let Some(effects) = EFFECTS.get() {
                effects.play_sound(
                    world,
                    [
                        f64::from(position.x),
                        f64::from(position.y),
                        f64::from(position.z),
                    ],
                    "minecraft:block.fire.extinguish",
                    moddedmc_api::effects::SOUND_BLOCKS,
                    0.5,
                    2.6 + (next_int(1_000) - next_int(1_000)) as f32 * 0.0008,
                );
            }
            return true;
        }
    };
    if world.write(&[BlockUpdate {
        position,
        state,
        flags: content::UPDATE_ALL,
    }]) != 1
    {
        return false;
    }
    if let Some(fluids) = FLUID_WORLD.get() {
        match change {
            Change::Waterlog(_) => {
                fluids.schedule_tick(world, position, "minecraft:water", 1);
            }
            Change::Liquid(_) => {
                if let Some(fluid) = world.fluid_id(position) {
                    fluids.schedule_default_tick(world, position, &fluid);
                }
            }
            _ => {}
        }
    }
    true
}

fn read_buffer(world: World<'_>, face: BlockFace) -> Option<TankInventory> {
    let data = smart_tank::read(world, face.pos)?;
    let mut tank = TankInventory::new(BUCKET, 1).ok()?;
    if let Some(Value::Compound(side)) = data.get(DIRECTIONS[usize::from(face.side)])
        && let Some(tag) = side.get("OpenEnd")
    {
        tank.read_content_nbt(tag).ok()?;
    }
    Some(tank)
}

fn write_buffer(world: World<'_>, face: BlockFace, tank: &TankInventory) -> bool {
    let Some(mut data) = smart_tank::read(world, face.pos) else {
        return false;
    };
    let side = data
        .entry(DIRECTIONS[usize::from(face.side)].to_owned())
        .or_insert_with(|| Value::Compound(HashMap::new()));
    let Value::Compound(side) = side else {
        return false;
    };
    if tank.amount() == 0 {
        side.remove("OpenEnd");
    } else {
        side.insert("OpenEnd".to_owned(), tank.content_nbt());
    }
    smart_tank::write(world, face.pos, &data)
}

pub struct OpenEnd {
    face: BlockFace,
    expected: u32,
    before: TankInventory,
    change: Change,
    pulling: bool,
}

impl OpenEnd {
    pub fn source(
        world: World<'_>,
        face: BlockFace,
        cell: &FluidCell,
    ) -> Option<(Self, TankInventory)> {
        let before = read_buffer(world, face)?;
        let mut available = before.clone();
        let change = if before.amount() == 0 {
            let (fluid, change) = source_plan(world, face, cell)?;
            available.fill(&fluid, false);
            change
        } else {
            Change::Keep
        };
        Some((
            Self {
                face,
                expected: cell.state,
                before,
                change,
                pulling: true,
            },
            available,
        ))
    }

    pub fn target(
        world: World<'_>,
        face: BlockFace,
        cell: &FluidCell,
        fluid: &FluidStack,
    ) -> Option<(Self, TankInventory)> {
        let change = target_plan(world, face, cell, fluid)?;
        let before = read_buffer(world, face)?;
        Some((
            Self {
                face,
                expected: cell.state,
                before: before.clone(),
                change,
                pulling: false,
            },
            before,
        ))
    }

    pub fn save(&self, world: World<'_>, tank: &TankInventory) -> bool {
        let flush = !self.pulling && tank.amount() == BUCKET;
        let stored = if flush {
            let Ok(empty) = TankInventory::new(BUCKET, 1) else {
                return false;
            };
            empty
        } else {
            tank.clone()
        };
        if !write_buffer(world, self.face, &stored) {
            return false;
        }
        let change = if self.pulling || flush {
            self.change
        } else {
            Change::Keep
        };
        if apply(world, self.face, self.expected, change) {
            return true;
        }
        write_buffer(world, self.face, &self.before);
        false
    }

    pub fn refund(&self, world: World<'_>, tank: &TankInventory) -> bool {
        write_buffer(world, self.face, tank)
    }
}
