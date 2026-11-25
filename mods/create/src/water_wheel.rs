//! The Water Wheel's fluid-driven rotation and placement.
use std::{collections::HashSet, sync::OnceLock};

use moddedmc_api::{
    InterfaceUnavailable, Pos, RegistrationError, Registry,
    content::{self, Block, Content, ContentEvent, State, World},
    fluid_world::{self, FluidFlow},
};

use crate::{
    CONTENT, DIRECTIONS, Kinetics, StateIds,
    kinetics::{FACES, Kind, Level, face_axis},
    offset, relay_orientation,
};

pub(crate) const ID: &str = "create:water_wheel";
pub(crate) const SPEED: i32 = 8;
const LAZY_TICKS: u64 = 60;
static FLOW: OnceLock<FluidFlow> = OnceLock::new();
static BUBBLE_COLUMNS: OnceLock<[Option<u32>; 2]> = OnceLock::new();

pub(crate) fn require() -> Result<(), InterfaceUnavailable> {
    let _ = FLOW.set(fluid_world::require_flow()?);
    Ok(())
}

pub(crate) fn register(
    content: Content,
    registry: &mut Registry<'_>,
) -> Result<(), RegistrationError> {
    let properties = DIRECTIONS.map(|facing| format!("facing={facing}"));
    let states = std::array::from_fn::<_, 6, _>(|i| State {
        properties: &properties[i],
        physical: "minecraft:spruce_planks",
        model: "",
        rotating_model: "create:block/water_wheel/stock_rotor",
        x: 0,
        y: 0,
        orientation: relay_orientation(face_axis(i as u8)),
        shape: &[],
    });
    content.add_block(
        registry,
        &Block {
            id: ID,
            display_name: "Water Wheel",
            entity: ID,
            item_model: "create:item/stock_water_wheel",
            states: &states,
        },
        Some(crate::placed_state),
    )
}

pub(crate) fn add_states(content: Content, states: &mut StateIds) {
    for (i, facing) in DIRECTIONS.into_iter().enumerate() {
        states.add(
            content,
            ID,
            &format!("facing={facing}"),
            Some(Kind::WaterWheel(i as u8)),
        );
    }
}

pub(crate) fn can_place(states: &StateIds, neighbors: &[u32; 7], axis: u8) -> bool {
    (0..6u8).all(|side| {
        !matches!(states.kind(neighbors[usize::from(side)]),
            Some(Kind::WaterWheel(other)) if face_axis(other) != axis || face_axis(side) != axis)
    })
}

pub(crate) fn tick(
    world: World<'_>,
    time: u64,
    events: &[ContentEvent],
    kinetics: &mut Kinetics,
    level: &mut dyn Level,
) {
    if time.is_multiple_of(LAZY_TICKS) {
        for pos in kinetics.positions(|kind| matches!(kind, Kind::WaterWheel(_))) {
            update(world, pos, kinetics, level);
        }
    } else {
        let mut checked = HashSet::new();
        for event in events {
            if matches!(
                event.reason,
                content::STATE_CHANGED | content::NEIGHBOR_CHANGED | content::LOADED
            ) && matches!(kinetics.kind(event.position), Some(Kind::WaterWheel(_)))
                && checked.insert(event.position)
            {
                update(world, event.position, kinetics, level);
            }
        }
    }
}

fn update(world: World<'_>, pos: Pos, kinetics: &mut Kinetics, level: &mut dyn Level) {
    let (Some(content), Some(&flow), Some(be)) = (CONTENT.get(), FLOW.get(), kinetics.get(pos))
    else {
        return;
    };
    let Kind::WaterWheel(facing) = be.kind else {
        return;
    };
    let axis = face_axis(facing);
    let bubbles = BUBBLE_COLUMNS.get_or_init(|| {
        [
            "minecraft:bubble_column[drag=false]",
            "minecraft:bubble_column[drag=true]",
        ]
        .map(|state| content.state(state))
    });
    let mut score = 0;
    for side in 0..6u8 {
        if face_axis(side) == axis {
            continue;
        }
        let normal = FACES[usize::from(side)];
        let at = offset(pos, normal);
        let Some(mut velocity) = flow.velocity(world, at) else {
            return;
        };
        let mut state = [content::UNLOADED_STATE];
        if world.read(&[at], &mut state) {
            if Some(state[0]) == bubbles[0] {
                velocity = [0.0, 1.0, 0.0];
            } else if Some(state[0]) == bubbles[1] {
                velocity = [0.0, -1.0, 0.0];
            }
        }
        score += score_flow(usize::from(axis), normal, velocity);
    }
    if score != be.generated {
        kinetics.set_generated(level, pos, score);
    }
}

/// WaterWheelBlockEntity.determineAndApplyFlowScore projects onto the wheel plane first.
fn score_flow(axis: usize, normal: [i32; 3], flow: [f64; 3]) -> i32 {
    let u = (axis + 1) % 3;
    let v = (axis + 2) % 3;
    let length = flow[u].hypot(flow[v]);
    let tangent = flow[v] * f64::from(normal[u]) - flow[u] * f64::from(normal[v]);
    if tangent.abs() > length * 0.5 {
        tangent.signum() as i32
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tangential_current_turns_the_wheel_and_axial_current_is_ignored() {
        assert_eq!(score_flow(0, [0, 0, 1], [20.0, -1.0, 0.0]), 1);
        assert_eq!(score_flow(0, [0, 0, -1], [20.0, -1.0, 0.0]), -1);
        assert_eq!(score_flow(0, [0, 0, 1], [20.0, 0.0, 1.0]), 0);
        assert_eq!(score_flow(0, [0, 0, 1], [20.0, 0.0, 0.0]), 0);
    }
}
