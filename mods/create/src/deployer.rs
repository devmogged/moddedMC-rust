//! The Deployer's timed Use-mode processing on depots and belts.
use fastnbt::Value;
use moddedmc_api::{
    Pos, RegistrationError, Registry,
    content::{Block, Content, Placement, State, World},
    interaction::{BlockUseResult, UseBlock},
    items::Item,
};
use serde::{Deserialize, Serialize};

use crate::{
    DIRECTIONS, ITEMS, Kinetics, STACKS, StateIds, WRENCH,
    belt_inventory::{TransportedItemStack, count, with_count},
    kinetics::{Kind, directional_shaft_axis, face_axis, positive},
    processing_recipes::{self, ProcessingResult, item_id},
};

pub const ID: &str = "create:deployer";
const EXTEND: i32 = 1000;
const RETRACT: i32 = 1000;
const COOLDOWN: i32 = 500;

fn variants() -> impl Iterator<Item = (u8, bool, String)> {
    [true, false].into_iter().flat_map(|along_first| {
        (0..6).map(move |facing| (facing, along_first, properties(facing, along_first)))
    })
}

fn properties(facing: u8, along_first: bool) -> String {
    format!(
        "axis_along_first={along_first},facing={}",
        DIRECTIONS[usize::from(facing)]
    )
}

pub fn register(content: Content, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
    let rows: Vec<_> = variants()
        .map(|(facing, along_first, properties)| {
            let (x, y) =
                [(270, 0), (90, 0), (0, 180), (0, 0), (0, 90), (0, 270)][usize::from(facing)];
            let y = y + if facing < 2 && !along_first { 90 } else { 0 };
            let shaft_axis = directional_shaft_axis(facing, along_first);
            let model = if facing >= 2 && shaft_axis == 1 {
                "create:block/deployer/vertical"
            } else {
                "create:block/deployer/horizontal"
            };
            let shape = [crate::piston::axial_box(facing, 0, 12, 0, 16)];
            (properties, x, y, shaft_axis, model, shape)
        })
        .collect();
    let states: Vec<_> = rows
        .iter()
        .map(|(properties, x, y, axis, model, shape)| State {
            properties,
            physical: "minecraft:stone",
            model,
            rotating_model: "create:block/shaft",
            x: *x,
            y: *y,
            orientation: crate::relay_orientation(*axis),
            shape,
        })
        .collect();
    content.add_block(
        registry,
        &Block {
            id: ID,
            display_name: "Deployer",
            item_model: "create:item/deployer",
            entity: ID,
            states: &states,
        },
        Some(crate::placed_state),
    )?;
    let items = ITEMS.get().ok_or(RegistrationError::InvalidDefinition)?;
    for (id, name, model) in [
        ("create:brass_hand", "Brass Hand", "create:item/brass_hand"),
        (
            "create:precision_mechanism",
            "Precision Mechanism",
            "create:item/precision_mechanism",
        ),
        (
            "create:incomplete_precision_mechanism",
            "Incomplete Precision Mechanism",
            "create:item/incomplete_precision_mechanism",
        ),
    ] {
        items.add_item(
            registry,
            &Item {
                id,
                display_name: name,
                carrier: "minecraft:paper",
                model,
            },
        )?;
    }
    Ok(())
}

pub fn add_states(content: Content, states: &mut StateIds) {
    for (facing, along_first, selector) in variants() {
        states.add(
            content,
            ID,
            &selector,
            Some(Kind::Deployer {
                facing,
                along_first,
            }),
        );
    }
}

pub fn placed(
    args: &Placement<'_>,
    look: u8,
    states: &StateIds,
    neighbors: &[u32; 7],
) -> Option<u32> {
    let facing = look ^ u8::from(!args.sneak);
    let shaft_towards = |side: u8| {
        states
            .kind(neighbors[usize::from(side)])
            .is_some_and(|kind| kind.has_shaft_towards(side ^ 1))
    };
    let along_first = if facing < 2 {
        let x = shaft_towards(4) || shaft_towards(5);
        let z = shaft_towards(2) || shaft_towards(3);
        if x == z {
            face_axis(crate::nearest_looking_direction(args.yaw, 0.0)) == 0
        } else {
            x
        }
    } else {
        (face_axis(facing) == 2) ^ (shaft_towards(0) || shaft_towards(1))
    };
    states.lookup(&format!("{ID}[{}]", properties(facing, along_first)))
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
enum Phase {
    #[default]
    Waiting,
    Expanding,
    Retracting,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct Deployer {
    #[serde(rename = "State")]
    phase: Phase,
    timer: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub held_item: Option<Value>,
}

fn start(deployer: &mut Deployer) {
    deployer.phase = Phase::Expanding;
    deployer.timer = EXTEND;
}

pub fn tick(world: World<'_>, kinetics: &mut Kinetics) {
    for (pos, item) in kinetics.dropped_deployers.drain(..) {
        if let (Some(&stacks), Ok(bytes)) = (STACKS.get(), fastnbt::to_bytes(&item)) {
            stacks.spawn(
                world,
                [pos.x, pos.y, pos.z].map(|c| f64::from(c) + 0.5),
                [0.0; 3],
                &bytes,
            );
        }
    }
    let positions = kinetics.positions(|kind| matches!(kind, Kind::Deployer { .. }));
    if positions.is_empty() {
        return;
    }
    kinetics.recipes = processing_recipes::current();
    for pos in positions {
        let Some(be) = kinetics.get_mut(pos) else {
            continue;
        };
        let speed = be.get_speed().abs();
        let Some(deployer) = be.deployer.as_mut() else {
            continue;
        };
        if speed == 0.0 {
            continue;
        }
        if deployer.timer > 0 {
            let timer_speed = (speed * 2.0).clamp(8.0, 512.0) as i32;
            deployer.timer = (deployer.timer - timer_speed).max(0);
        } else {
            match deployer.phase {
                Phase::Waiting => continue,
                Phase::Expanding => {
                    deployer.phase = Phase::Retracting;
                    deployer.timer = RETRACT;
                }
                Phase::Retracting => {
                    deployer.phase = Phase::Waiting;
                    deployer.timer = COOLDOWN;
                }
            }
        }
        be.dirty = true;
    }
}

pub fn process(
    world: World<'_>,
    kinetics: &mut Kinetics,
    pos: Pos,
    item: &TransportedItemStack,
) -> ProcessingResult {
    use ProcessingResult::{Convert, Hold, Pass};
    let Some(recipes) = kinetics.recipes.clone() else {
        return Pass;
    };
    let Some(be) = kinetics.get_mut(pos) else {
        return Pass;
    };
    if !matches!(be.kind, Kind::Deployer { facing: 0, .. }) || be.get_speed() == 0.0 {
        return Pass;
    }
    let Some(deployer) = be.deployer.as_mut() else {
        return Pass;
    };
    if !item.locked && deployer.phase != Phase::Waiting {
        return Hold;
    }
    let powered = deployer.phase == Phase::Waiting && world.has_neighbor_signal(pos);
    if !item.locked && powered {
        return Pass;
    }
    let Some(held) = &deployer.held_item else {
        return Hold;
    };
    let Some(recipe) = item_id(held).and_then(|id| recipes.deploy(&item.item, id)) else {
        return Pass;
    };
    if deployer.phase == Phase::Waiting {
        if powered {
            return Pass;
        }
        start(deployer);
        be.dirty = true;
        return Hold;
    }
    if !item.locked || deployer.phase != Phase::Retracting || deployer.timer != RETRACT {
        return Hold;
    }
    if !recipe.keep_held_item() {
        let remaining = count(held) - 1;
        deployer.held_item = (remaining > 0).then(|| with_count(held, remaining));
        be.dirty = true;
    }
    let outputs = recipe.finish();
    Convert(outputs)
}

pub fn use_block(
    kinetics: &mut Kinetics,
    context: &mut UseBlock<'_>,
    facing: u8,
) -> BlockUseResult {
    let coordinate = context.hit[usize::from(face_axis(facing))];
    let end = if positive(facing) {
        coordinate
    } else {
        1.0 - coordinate
    };
    if end < 0.75 || context.player.is_none_or(|player| !player.may_build()) {
        return BlockUseResult::Pass;
    }
    let held = context.stack.read();
    if context.with_item && held.is_none() {
        return BlockUseResult::TryEmptyHand;
    }
    let incoming = held
        .as_deref()
        .and_then(|bytes| fastnbt::from_bytes::<Value>(bytes).ok());
    if held.is_some()
        && incoming
            .as_ref()
            .and_then(item_id)
            .is_none_or(|id| id == WRENCH || id == ID)
    {
        return BlockUseResult::Pass;
    }
    let Some(be) = kinetics.get_mut(context.position) else {
        return BlockUseResult::Fail;
    };
    let Some(deployer) = be.deployer.as_mut() else {
        return BlockUseResult::Fail;
    };
    let encoded = deployer
        .held_item
        .as_ref()
        .map(fastnbt::to_bytes)
        .transpose();
    let Ok(previous) = encoded else {
        return BlockUseResult::Fail;
    };
    let exchanged = if context.with_item {
        context.stack.write(previous.as_deref().unwrap_or_default())
    } else if let Some(bytes) = previous {
        context.give(&bytes)
    } else {
        false
    };
    if !exchanged {
        return BlockUseResult::Pass;
    }
    deployer.held_item = incoming;
    be.dirty = true;
    BlockUseResult::Success
}

/// The Deployer item handler has one held stack. Only identical stacks merge.
pub fn insert(
    kinetics: &mut Kinetics,
    pos: Pos,
    incoming: &TransportedItemStack,
    simulate: bool,
) -> Option<i32> {
    let be = kinetics.get_mut(pos)?;
    let deployer = be.deployer.as_mut()?;
    let held = deployer.held_item.as_ref();
    if held.is_some_and(|held| with_count(held, 1) != with_count(&incoming.item, 1)) {
        return Some(0);
    }
    let stored = held.map_or(0, count);
    let accepted = count(&incoming.item).min((incoming.max_count() - stored).max(0));
    if !simulate && accepted > 0 {
        deployer.held_item = Some(with_count(&incoming.item, stored + accepted));
        be.dirty = true;
    }
    Some(accepted)
}

pub fn extract(kinetics: &mut Kinetics, pos: Pos, limit: i32, simulate: bool) -> Option<Value> {
    let be = kinetics.get_mut(pos)?;
    let deployer = be.deployer.as_mut()?;
    let item = deployer.held_item.as_ref()?;
    let extracted = count(item).min(limit);
    if extracted <= 0 {
        return None;
    }
    let stack = with_count(item, extracted);
    if !simulate {
        let remaining = count(item) - extracted;
        deployer.held_item = (remaining > 0).then(|| with_count(item, remaining));
        be.dirty = true;
    }
    Some(stack)
}
