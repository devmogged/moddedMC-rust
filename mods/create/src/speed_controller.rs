//! Rotation Speed Controller placement, adjustment and the large-cog connection.

use moddedmc_api::{
    Pos, RegistrationError, Registry,
    content::{Block, Content, ShapeBox, State},
    interaction::{BlockUseResult, UseBlock},
};

use crate::{
    AXES, LARGE, MAX_SPEED, StateIds, WorldLevel,
    kinetics::{Be, FACES, Kind, face_axis, offset},
    processing_recipes, relay_orientation, with_world,
};

pub(crate) const ID: &str = "create:rotation_speed_controller";
pub(crate) const DEFAULT_SPEED: i32 = 16;

const SHAPE: [ShapeBox; 3] = [
    ShapeBox {
        min: [0, 0, 0],
        max: [16, 4, 16],
    },
    ShapeBox {
        min: [1, 1, 1],
        max: [15, 13, 15],
    },
    ShapeBox {
        min: [0, 8, 0],
        max: [16, 14, 16],
    },
];

pub(crate) fn register(
    content: Content,
    registry: &mut Registry<'_>,
) -> Result<(), RegistrationError> {
    let states = [0_u8, 2].map(|axis| State {
        properties: if axis == 0 { "axis=x" } else { "axis=z" },
        physical: "minecraft:iron_block",
        model: "create:block/rotation_speed_controller/block",
        rotating_model: "create:block/shaft",
        x: 0,
        y: if axis == 0 { 90 } else { 0 },
        orientation: relay_orientation(axis),
        shape: &SHAPE,
    });
    content.add_block(
        registry,
        &Block {
            id: ID,
            display_name: "Rotation Speed Controller",
            entity: ID,
            item_model: "create:item/rotation_speed_controller",
            states: &states,
        },
        Some(crate::placed_state),
    )
}

pub(crate) fn add_states(content: Content, states: &mut StateIds) {
    for (axis, properties) in [(0, "axis=x"), (2, "axis=z")] {
        states.add(content, ID, properties, Some(Kind::SpeedController(axis)));
    }
}

pub(crate) fn placed(states: &StateIds, above: u32, preferred: u8) -> Option<u32> {
    let axis = match states.kind(above) {
        Some(Kind::Large(axis)) if axis != 1 => axis ^ 2,
        _ => preferred,
    };
    states.lookup(&format!("{ID}[axis={}]", AXES[usize::from(axis)]))
}

/// RotationPropagator.isLargeCogToSpeedController.
pub(crate) fn is_pair(wheel_pos: Pos, wheel: Kind, pos: Pos, controller: Kind) -> bool {
    matches!((wheel, controller), (Kind::Large(wheel), Kind::SpeedController(axis))
        if wheel != 1 && wheel != axis)
        && wheel_pos.x == pos.x
        && wheel_pos.y - 1 == pos.y
        && wheel_pos.z == pos.z
}

pub(crate) fn conveyed(
    wheel_pos: Pos,
    wheel: &Be,
    controller: &Be,
    toward_controller: bool,
) -> f32 {
    let desired = if controller.generated == 0 || toward_controller && wheel.speed == 0.0 {
        0.0
    } else if let Some(source) = controller.source {
        match (source == wheel_pos, toward_controller) {
            (true, false) => wheel.speed,
            (false, true) => controller.speed,
            _ => controller.generated as f32,
        }
    } else if toward_controller {
        controller.generated as f32
    } else {
        0.0
    };

    let current = if toward_controller {
        controller.speed
    } else {
        wheel.speed
    };
    if desired >= 0.0 && current >= 0.0 {
        desired.max(current)
    } else if desired < 0.0 && current < 0.0 {
        desired.min(current)
    } else {
        desired
    }
}

pub(crate) fn use_block(
    context: &mut UseBlock<'_>,
    states: &'static StateIds,
    axis: u8,
) -> BlockUseResult {
    if !context.main_hand {
        return BlockUseResult::Pass;
    }
    if let Some(bytes) = context.stack.read() {
        let id = fastnbt::from_bytes::<fastnbt::Value>(&bytes).ok();
        if id.as_ref().and_then(processing_recipes::item_id) != Some(LARGE) || context.sneaking {
            return BlockUseResult::Pass;
        }
        let above = offset(context.position, FACES[1]);
        let mut positions = [above; 7];
        for (pos, step) in positions.iter_mut().zip(FACES) {
            *pos = offset(above, step);
        }
        let mut ids = [0; 7];
        if !context.world.read(&positions, &mut ids)
            || !states.replaceable.contains(&ids[6])
            || (0..6_u8).any(|face| {
                let side_axis = face_axis(face);
                side_axis != axis ^ 2
                    && matches!(states.kind(ids[usize::from(face)]),
                        Some(Kind::Small(cog_axis) | Kind::Large(cog_axis)) if side_axis != cog_axis)
            })
        {
            // Consume the click so ordinary placement cannot put the cog beside the controller.
            return BlockUseResult::Consume;
        }
        if !context.place_held_block(context.position, 1) {
            return BlockUseResult::Consume;
        }
        if !context.creative {
            context.stack.shrink(1);
        }
        return BlockUseResult::Success;
    }
    if context.face < 2 || face_axis(context.face) == axis {
        return BlockUseResult::Pass;
    }

    // Stock clients can use either side of the value plate to adjust the RPM.
    let coordinate = context.hit[usize::from(axis)];
    let right_positive = matches!(context.face, 3 | 4);
    let increase = (coordinate >= 0.5) == right_positive;
    let step = if context.sneaking { 16 } else { 1 };
    let delta = if increase { step } else { -step };
    let world = context.world;
    with_world(world, |kinetics| {
        let Some(be) = kinetics.get(context.position) else {
            return BlockUseResult::Fail;
        };
        let rpm = (be.generated + delta).clamp(-MAX_SPEED, MAX_SPEED);
        kinetics.set_controller_speed(&mut WorldLevel { world, states }, context.position, rpm);
        context.action_bar(&format!("Rotation speed {rpm} RPM"));
        BlockUseResult::Success
    })
}
