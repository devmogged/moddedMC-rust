//! The Deployer's timed Use-mode processing on depots and belts.
use fastnbt::Value;
use moddedmc_api::{
    Pos, RegistrationError, Registry,
    content::{Block, Content, Placement, State, World},
    effects::SOUND_BLOCKS,
    interaction::{BlockUseResult, UseBlock},
    items::Item,
    stacks::ItemDisplay,
};
use serde::{Deserialize, Serialize};

use crate::{
    DIRECTIONS, ITEMS, Kinetics, STACKS, StateIds, WRENCH,
    belt_inventory::{TransportedItemStack, count, with_count},
    display_model::ModelDisplay,
    kinetics::{FACES, Kind, directional_shaft_axis, face_axis, positive},
    manual_application, offset,
    processing_recipes::{self, Deployment, ProcessingResult, item_id},
};

pub const ID: &str = "create:deployer";
const EXTEND: i32 = 1000;
const RETRACT: i32 = 1000;
const COOLDOWN: i32 = 500;
const MAX_REACH: f32 = 1.25;
const MAX_HAND_OFFSET: f32 = 21.0 / 16.0;
const POLE_MODEL: &str = "create:moddedmc/parts/deployer/pole";
const POINTING_MODEL: &str = "create:moddedmc/parts/deployer/pointing";
const HOLDING_MODEL: &str = "create:moddedmc/parts/deployer/holding";

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
    let (facing, along_first) = placement_axis(args, look, states, neighbors);
    states.lookup(&format!("{ID}[{}]", properties(facing, along_first)))
}

pub(crate) fn placement_axis(
    args: &Placement<'_>,
    look: u8,
    states: &StateIds,
    neighbors: &[u32; 7],
) -> (u8, bool) {
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
    (facing, along_first)
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
    reach: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub held_item: Option<Value>,
}

impl Deployer {
    fn consume_held(&mut self) {
        if let Some(held) = &self.held_item {
            let remaining = count(held) - 1;
            self.held_item = (remaining > 0).then(|| with_count(held, remaining));
        }
    }
}

fn start(deployer: &mut Deployer, world: World<'_>, target: Pos, facing: u8) {
    deployer.phase = Phase::Expanding;
    deployer.timer = EXTEND;
    let axis = usize::from(face_axis(facing));
    // Sample along the center of the target block, matching the Deployer's axial reach ray.
    let nearest = world
        .collision_shape(target)
        .into_iter()
        .flatten()
        .filter(|shape| {
            (0..3)
                .filter(|&i| i != axis)
                .all(|i| (shape.min[i]..=shape.max[i]).contains(&0.5))
        })
        .map(|shape| {
            if positive(facing) {
                shape.min[axis]
            } else {
                1.0 - shape.max[axis]
            }
        })
        .fold(0.75, f64::min);
    deployer.reach = Some(0.5 + nearest.clamp(0.0, 0.75) as f32);
}

#[derive(Clone, Copy, PartialEq)]
struct HandPose {
    facing: u8,
    along_first: bool,
    holding: bool,
    distance: f32,
}

impl Deployer {
    fn hand_pose(&self, facing: u8, along_first: bool) -> HandPose {
        let progress = match self.phase {
            Phase::Waiting => 0.0,
            Phase::Expanding => 1.0 - self.timer as f32 / EXTEND as f32,
            Phase::Retracting => self.timer as f32 / RETRACT as f32,
        }
        .clamp(0.0, 1.0);
        let holding = self.held_item.is_some();
        let hand_length = if holding { 4.0 / 16.0 } else { 0.0 };
        let distance =
            (progress * (self.reach.unwrap_or(MAX_REACH) + hand_length)).min(MAX_HAND_OFFSET);
        HandPose {
            facing,
            along_first,
            holding,
            distance,
        }
    }
}

#[derive(Default)]
pub(crate) struct HandDisplays {
    hand: ModelDisplay,
    pole: ModelDisplay,
    pose: Option<HandPose>,
}

impl HandDisplays {
    fn show(&mut self, world: World<'_>, pos: Pos, time: u64, pose: HandPose) {
        if self.pose == Some(pose) && !time.is_multiple_of(20) {
            return;
        }
        let facing = usize::from(pose.facing);
        let [x, y] = [
            [90.0f32, 0.0],
            [270.0, 0.0],
            [0.0, 180.0],
            [0.0, 0.0],
            [0.0, 270.0],
            [0.0, 90.0],
        ][facing];
        let (sx, cx) = (x.to_radians() * 0.5).sin_cos();
        let (sy, cy) = (y.to_radians() * 0.5).sin_cos();
        let [x, y, z, w] = [sx * cy, cx * sy, -sx * sy, cx * cy];
        let position = [pos.x, pos.y, pos.z].map(f64::from);
        let mut display = ItemDisplay {
            position: std::array::from_fn(|i| {
                position[i] + 0.5 + f64::from(FACES[facing][i]) * f64::from(pose.distance)
            }),
            rotation: [x, y, z, w],
            scale: 1.0,
            context: 0,
            interpolation: 1,
        };
        let hand_model = if pose.holding {
            HOLDING_MODEL
        } else {
            POINTING_MODEL
        };
        self.hand.show(world, hand_model, &display);
        if pose.along_first ^ (face_axis(pose.facing) == 2) {
            let half = std::f32::consts::FRAC_1_SQRT_2;
            display.rotation = [
                (x + y) * half,
                (y - x) * half,
                (z + w) * half,
                (w - z) * half,
            ];
        }
        self.pole.show(world, POLE_MODEL, &display);
        self.pose = Some(pose);
    }

    pub(crate) fn clear(&mut self, world: World<'_>) {
        self.hand.clear(world);
        self.pole.clear(world);
    }
}

pub fn tick(world: World<'_>, time: u64, kinetics: &mut Kinetics) {
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
    let recipes = processing_recipes::current();
    kinetics.recipes.clone_from(&recipes);
    for &pos in &positions {
        let Some(Kind::Deployer { facing, .. }) = kinetics.kind(pos) else {
            continue;
        };
        let target = offset(pos, FACES[usize::from(facing)].map(|step| step * 2));
        let has_transport = facing == 0
            && (kinetics.depots.contains_key(&target)
                || matches!(kinetics.kind(target), Some(Kind::Belt(_))));
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
            let application = || {
                if has_transport {
                    return None;
                }
                let ingredient = deployer.held_item.as_ref().and_then(item_id)?;
                manual_application::recipe_at(world, target, recipes.as_deref()?, ingredient)
            };
            match deployer.phase {
                Phase::Waiting => {
                    if application().is_none() || world.has_neighbor_signal(pos) {
                        continue;
                    }
                    start(deployer, world, target, facing);
                }
                Phase::Expanding => {
                    if let Some(recipe) = application()
                        && manual_application::apply(world, target, recipe)
                        && !recipe.keep_held_item
                    {
                        deployer.consume_held();
                    }
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
    for pos in positions {
        let pose = kinetics.get(pos).and_then(|be| {
            let Kind::Deployer {
                facing,
                along_first,
            } = be.kind
            else {
                return None;
            };
            Some(be.deployer.as_ref()?.hand_pose(facing, along_first))
        });
        if let Some(pose) = pose {
            kinetics
                .deployer_displays
                .entry(pos)
                .or_default()
                .show(world, pos, time, pose);
        }
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
        start(deployer, world, offset(pos, [0, -2, 0]), 0);
        be.dirty = true;
        return Hold;
    }
    if !item.locked || deployer.phase != Phase::Retracting || deployer.timer != RETRACT {
        return Hold;
    }
    if matches!(recipe, Deployment::Polishing(_)) {
        let Some(Value::Compound(held)) = &mut deployer.held_item else {
            return Pass;
        };
        match crate::wear_stack(held, i32::from(crate::sandpaper::DURABILITY)) {
            Some(true) => {}
            Some(false) => deployer.held_item = None,
            None => return Pass,
        }
        be.dirty = true;
        if let Some(effects) = crate::EFFECTS.get() {
            effects.play_sound(
                world,
                [pos.x, pos.y, pos.z].map(|coordinate| f64::from(coordinate) + 0.5),
                "minecraft:item.brush.brushing_generic",
                SOUND_BLOCKS,
                0.35,
                1.0,
            );
        }
    } else if !recipe.keep_held_item() {
        deployer.consume_held();
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
