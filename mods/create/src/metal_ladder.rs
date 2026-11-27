//! Create's metal ladders and vertical placement assistance.
use std::{cell::Cell, sync::OnceLock};

use moddedmc_api::{
    InterfaceUnavailable, Pos, RegistrationError, Registry,
    block_physics::{self, BlockPhysics},
    content::{Block, BlockMaterial, BlockUpdate, Content, Placement, State, UPDATE_ALL},
    effects::SOUND_BLOCKS,
    interaction::{BlockUseResult, UseBlock, UseOnBlock, UseResult},
};

use crate::{
    CONTENT, EFFECTS, StateIds, belt_tunnel::read, kinetics::offset, processing_recipes::item_id,
};

const BLOCKS: [(&str, &str, &str); 3] = [
    ("create:andesite_ladder", "Andesite Ladder", "stone"),
    ("create:brass_ladder", "Brass Ladder", "terracotta_yellow"),
    ("create:copper_ladder", "Copper Ladder", "color_orange"),
];
const PLACEMENT_ASSIST_RANGE: i32 = 12;
const LADDER_HARDNESS: f32 = 0.4;

static PHYSICS: OnceLock<BlockPhysics> = OnceLock::new();
thread_local! {
    static PLACEMENT: Cell<Option<(Pos, u32)>> = const { Cell::new(None) };
}

pub fn require() -> Result<(), InterfaceUnavailable> {
    let _ = PHYSICS.set(block_physics::require()?);
    Ok(())
}

pub fn is_metal_ladder(id: &str) -> bool {
    BLOCKS.iter().any(|(block, ..)| *block == id)
}

pub fn register(content: Content, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
    let physics = PHYSICS.get().ok_or(RegistrationError::InvalidDefinition)?;
    for (id, name, map_color) in BLOCKS {
        let path = id
            .split_once(':')
            .ok_or(RegistrationError::InvalidDefinition)?
            .1;
        let model = format!("create:block/{path}");
        let variants = std::array::from_fn::<_, 8, _>(|index| {
            let (facing, y) =
                [("north", 0), ("east", 90), ("south", 180), ("west", 270)][index / 2];
            let wet = index % 2 != 0;
            let properties = format!("facing={facing},waterlogged={wet}");
            let physical = format!("minecraft:ladder[{properties}]");
            (properties, physical, y)
        });
        let states = variants.each_ref().map(|(properties, physical, y)| State {
            properties,
            physical,
            model: &model,
            rotating_model: "",
            x: 0,
            y: *y,
            orientation: [0.0, 0.0, 0.0, 1.0],
            shape: &[],
        });
        content.add_block(
            registry,
            &Block {
                id,
                display_name: name,
                entity: "",
                item_model: &format!("create:item/{path}"),
                states: &states,
            },
            Some(crate::placed_state),
        )?;
        content.set_block_material(
            registry,
            id,
            &BlockMaterial {
                template: "minecraft:ladder",
                hardness: None,
                map_color: Some(map_color),
            },
        )?;
        physics.set(
            registry,
            id,
            LADDER_HARDNESS,
            LADDER_HARDNESS,
            false,
            "minecraft:copper_bars",
        )?;
    }
    Ok(())
}

pub fn assisted_state(context: &Placement<'_>) -> Option<u32> {
    PLACEMENT
        .get()
        .filter(|&(position, _)| position == context.position)
        .map(|(_, state)| state)
}

pub fn use_block(context: &mut UseBlock<'_>, states: &StateIds) -> Option<BlockUseResult> {
    if !context.with_item || context.sneaking {
        return None;
    }
    let content = CONTENT.get()?;
    let description = content.describe_state(context.state)?;
    let (block, properties) = description.split_once('[')?;
    if !is_metal_ladder(block) {
        return None;
    }
    let player = context.player.filter(|player| player.may_build())?;
    let facing = properties
        .strip_suffix(']')?
        .split(',')
        .find_map(|part| part.strip_prefix("facing="))?;
    let held: fastnbt::Value = fastnbt::from_bytes(&context.stack.read()?).ok()?;
    let held = item_id(&held).filter(|id| is_metal_ladder(id))?;
    let step = if player.view()?.pitch < 0.0 { 1 } else { -1 };

    let mut target = context.position;
    for _ in 0..PLACEMENT_ASSIST_RANGE {
        target = offset(target, [0, step, 0]);
        let state = read(context.world, target);
        let description = content.describe_state(state)?;
        let name = description.split('[').next()?;
        if name == "minecraft:ladder" || is_metal_ladder(name) {
            continue;
        }
        if !states.replaceable.contains(&state)
            || context.world.can_survive(target, context.state) != Some(true)
        {
            return None;
        }
        let wet = content.state("minecraft:water[level=0]") == Some(state);
        let selected = content.state(&format!("{held}[facing={facing},waterlogged={wet}]"))?;
        if context.world.can_survive(target, selected) != Some(true) {
            return None;
        }
        let placed = PLACEMENT.with(|hint| {
            let previous = hint.replace(Some((target, selected)));
            let placed = context.place_held_block(target, context.face);
            hint.set(previous);
            placed
        });
        if placed {
            if !context.creative {
                context.stack.shrink(1);
            }
            return Some(BlockUseResult::Success);
        }
        return None;
    }
    None
}

pub fn rotate(context: &UseOnBlock<'_>) -> Option<UseResult> {
    if context.sneaking || !context.player.is_some_and(|player| player.may_build()) {
        return None;
    }
    let content = CONTENT.get()?;
    let state = read(context.world, context.position);
    let description = content.describe_state(state)?;
    let (block, properties) = description.split_once('[')?;
    if !is_metal_ladder(block) {
        return None;
    }
    if context.face > 1 {
        return Some(UseResult::Success);
    }
    let properties = properties.strip_suffix(']')?;
    let facing = properties
        .split(',')
        .find_map(|part| part.strip_prefix("facing="))?;
    let wet = properties
        .split(',')
        .find_map(|part| part.strip_prefix("waterlogged="))?;
    let facing = match facing {
        "north" => "east",
        "east" => "south",
        "south" => "west",
        "west" => "north",
        _ => return None,
    };
    let next = content.state(&format!("{block}[facing={facing},waterlogged={wet}]"))?;
    if context.world.can_survive(context.position, next) != Some(true) {
        return None;
    }
    let changed = context.world.write_with_neighbor_shapes(&[BlockUpdate {
        position: context.position,
        state: next,
        flags: UPDATE_ALL,
    }]);
    if changed != 1 {
        return None;
    }
    if let Some(effects) = EFFECTS.get() {
        let pos = context.position;
        let coords = [pos.x, pos.y, pos.z];
        effects.play_sound(
            context.world,
            coords.map(|coord| f64::from(coord) + 0.5),
            "minecraft:block.wooden_trapdoor.close",
            SOUND_BLOCKS,
            0.25,
            1.25 * (0.5 + crate::decor::wrench_pitch(coords)),
        );
    }
    Some(UseResult::Success)
}
